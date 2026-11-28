//! 引擎：把「账号元数据 + 加密凭据 + 平台适配 + 历史」串起来。
//!
//! 这一层是唯一知道「一个动作要走哪几步」的地方：
//!
//! ```text
//! 取凭据 -> 刷新令牌 -> 调平台 -> 回写轮换后的令牌 -> 复核状态 -> 落快照 -> 记账
//! ```
//!
//! UI 只调这里的方法，不直接碰 vault / providers / store。

use ph_core::{
    Account, AccountSchedule, ActivityEvent, CheckinOutcome, CheckinStatus, Credential, DeviceInfo,
    Error, MergeReport, Result, Settings, SyncPayload, Tombstone, TravelSnapshot, Vendor,
    VendorSnapshot, SYNC_PROTOCOL,
};
use ph_providers::{provider_for, TravelAction};
use ph_store::Store;
use ph_vault::Vault;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 汇总视图，给悬浮球与总览页用。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Aggregate {
    pub total_balance: f64,
    pub account_count: usize,
    /// 今天还没签到的账号数。
    pub pending_count: usize,
    /// 有余额数据的账号数（用于说明「总数覆盖了几个账号」）。
    pub balance_known: usize,
    /// 最近一次采样时间。
    pub last_sampled_at: Option<i64>,
    /// 本月签到实际到账的积分之和；`None` 表示这份数据来源没有提供（例如旧版电脑端）。
    ///
    /// 只看 `checkin` 且 `delta > 0` 的流水：旅行、失败、负向调整都不算「领取」。
    pub claimed_this_month: Option<f64>,
}

/// 今日签到异常，供总览页提示「还有账号没签成功」。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CheckinAlert {
    pub account_id: String,
    pub label: String,
    pub message: String,
}

/// 近 N 天里某一天的余额合计。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DayPoint {
    /// 本地日期，形如 `11-25`。
    pub date: String,
    /// 当天最后一次采样时各账号余额之和。
    pub total: f64,
    /// 当天有余额数据的账号数（用于提示合计覆盖了几个号）。
    pub known: usize,
    /// 相对前一天的新增（余额差）。前一天没有数据时为 None，**不要填 0**。
    pub gain: Option<f64>,
    /// 当天签到实际领取的积分之和（口径：`checkin` 且 `delta > 0` 的流水）。
    ///
    /// 这是「今日新增」的唯一来源——不看余额差，不然签到 +100 但当天有消耗时，
    /// 新增会被抵小。
    pub up: Option<f64>,
    /// 当天消耗的总积分（正数）。
    ///
    /// 按「昨日余额 + 今日领取 − 今日余额」反推：签到 +300、余额从 100 掉到 50 时，
    /// 消耗是 350 而不是 50——消耗掉的 300 也要算进去。
    /// 无法与前一天比较时为 None；反推成负数时取 0（说明还有别的加分来源）。
    pub down: Option<f64>,
}

/// 某个月的积分汇总（年度视图里的一格）。
///
/// 月度里的 `up`/`down` 是当月逐日之和：日口径改了，月口径跟着改，不用另写一套。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MonthPoint {
    /// 形如 `2026-11`。
    pub month: String,
    /// 该月最后一个有数据日的余额合计。
    pub total: f64,
    /// 该月有余额数据的天数（0 表示整月没采样，这种月在年度视图里占位、三项为 0）。
    pub days: usize,
    /// 该月签到领取之和。
    pub up: f64,
    /// 该月消耗之和（正数）。
    pub down: f64,
}

pub struct Engine {
    store: Store,
    vault: Vault,
    settings: Settings,
    accounts: Vec<Account>,
    /// 本机身份。双端同步与执行端判定都以它为准。
    device: DeviceInfo,
}

/// 设备类型：安卓壳编译出来就是 `android`，其余按 `pc` 处理。
/// 老数据不记导入来源：从默认命名（`平台 本机` / `平台 网页` / `平台 账号`）推断一次。
///
/// 只认「平台名开头 + 三种后缀」，用户改过的名字认不出来就保持 `None`，
/// 界面只是不显示来源，不会乱猜。
fn infer_source(a: &Account) -> Option<String> {
    let label = a.label.trim();
    let display = a.vendor.display();
    let rest = label.strip_prefix(display)?.trim();
    match rest.split_whitespace().next().unwrap_or("") {
        "本机" => Some("local".to_string()),
        "网页" => Some("browser".to_string()),
        "账号" => Some("token".to_string()),
        _ => None,
    }
}

fn device_kind() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else {
        "pc"
    }
}

/// 本地时间转 epoch 秒；时区解析失败时退回当前时间。
fn local_epoch(now: chrono::NaiveDateTime) -> i64 {
    use chrono::TimeZone;
    chrono::Local
        .from_local_datetime(&now)
        .single()
        .map(|d| d.timestamp())
        .unwrap_or_else(|| chrono::Local::now().timestamp())
}

/// 今天 0 点的 epoch 秒（用于「每天只通知一次失败」的标记）。
fn today_start_epoch() -> Option<i64> {
    use chrono::TimeZone;
    let today = chrono::Local::now().date_naive();
    let start = today.and_hms_opt(0, 0, 0)?;
    chrono::Local
        .from_local_datetime(&start)
        .single()
        .map(|d| d.timestamp())
}

/// 本月 1 日 0 点的 epoch 秒（用于「本月领取总积分」的统计起点）。
fn month_start_epoch() -> i64 {
    use chrono::Datelike;
    let now = chrono::Local::now();
    chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(local_epoch)
        .unwrap_or(0)
}

/// 失败后的自动重试间隔。
///
/// 凭据失效重试也没用，直接等到次日 0 点；普通失败 30 分钟后再试。
/// 手动签到不读这个间隔，用户点按钮就会立刻执行。
fn retry_delay_secs(status: CheckinStatus, at: i64) -> i64 {
    match status {
        CheckinStatus::AuthFailed => {
            use chrono::TimeZone;
            let next_day = chrono::Local
                .timestamp_opt(at, 0)
                .single()
                .map(|n| n.date_naive() + chrono::Duration::days(1))
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .and_then(|t| chrono::Local.from_local_datetime(&t).single())
                .map(|d| d.timestamp());
            next_day.map(|t| (t - at).max(60)).unwrap_or(24 * 3600)
        }
        _ => 30 * 60,
    }
}

/// 默认设备名：优先用主机名，读不到就给一个人能看懂的兜底。
fn default_device_name() -> String {
    if cfg!(target_os = "android") {
        return "安卓手机".into();
    }
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "我的电脑".into())
}

impl Engine {
    /// 打开引擎。`data_dir` 为 None 时按「指针文件 -> 系统默认」定位。
    pub fn open(data_dir: Option<PathBuf>) -> Result<Self> {
        let root = ph_store::locate::resolve(data_dir);
        let store = Store::open(&root)?;
        let vault = Vault::open(store.path_of(ph_core::files::VAULT))?;
        // 系统密钥丢失时凭据仓会被按空仓打开，这里把原因落到日志里，
        // 界面上的账号会显示「缺凭据」，用户重新导入即可。
        if let Some(notice) = vault.notice() {
            store.log(notice);
        }
        let mut settings = store.load_settings().unwrap_or_else(|e| {
            store.log(&format!("设置读取失败，使用默认值：{e}"));
            Settings::default()
        });
        // 老 `settings.json` 没有「各平台签到时间」这一段：补默认值并回写，
        // 否则后面的重算只能拿到空表、每个平台都得现算一遍默认值。
        // 回写失败不算致命（内存里已经补齐，本轮照常跑），不能因此让程序起不来。
        if settings.ensure_platform_schedules() {
            let _ = store.save_settings(&settings);
        }
        let mut accounts = store.load_accounts().unwrap_or_else(|e| {
            store.log(&format!("账号读取失败：{e}"));
            Vec::new()
        });
        let device = match store.load_device()? {
            Some(d) => d,
            None => {
                let d = DeviceInfo::new(default_device_name(), device_kind());
                store.save_device(&d)?;
                d
            }
        };
        // 老数据没有 executor / origin_device：补成本机，避免同步时被当成「无主账号」。
        //
        // 另外把「执行端指向一台已经不存在的设备」的账号收回本机：本机换过
        // device.json（例如数据目录搬迁后重新生成）时，账号仍记着旧设备 id，
        // 结果是任何设备都无法执行——调度器每 45 秒失败一次并弹通知。
        // 配对清单读取失败时不做这个判断，避免把所有手机账号误收回。
        let paired_ids: Option<Vec<String>> = store
            .load_devices()
            .ok()
            .map(|v| v.into_iter().map(|d| d.id).collect());
        let mut dirty = false;
        let mut reclaimed = Vec::new();
        for a in &mut accounts {
            if a.source.is_none() {
                a.source = infer_source(a);
                if a.source.is_some() {
                    dirty = true;
                }
            }
            if a.executor.is_none() {
                a.executor = Some(device.id.clone());
                dirty = true;
            }
            if a.origin_device.is_none() {
                a.origin_device = Some(device.id.clone());
                dirty = true;
            }
            let orphan = match (a.executor.as_deref(), &paired_ids) {
                (Some(ex), Some(ids)) => ex != device.id && !ids.iter().any(|id| id == ex),
                _ => false,
            };
            if orphan {
                a.executor = Some(device.id.clone());
                reclaimed.push(a.label.clone());
                dirty = true;
            }
        }
        if dirty {
            store.save_accounts(&accounts)?;
        }
        for label in reclaimed {
            store.log(&format!("账号「{label}」的执行端设备已不存在，已收回本机"));
        }
        let mut engine = Self {
            store,
            vault,
            settings,
            accounts,
            device,
        };
        // 老数据迁移：账号上带的是「按账号 id + 日期」的旧抖动，
        // 统一收敛到「平台基准 + 同平台序号错峰」，同一平台多账号才真正错开。
        engine.reconcile_schedules()?;
        Ok(engine)
    }

    // ------------------------------------------------------------ 只读视图

    pub fn data_dir(&self) -> &Path {
        self.store.root()
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    pub fn vault_kind(&self) -> &'static str {
        self.vault.kind().name()
    }

    /// 历史快照条数（用于自检与体积提示）。
    pub fn history_count(&self) -> usize {
        self.store.history_line_count()
    }

    /// 本机身份（设备 id / 显示名 / 类型）。
    pub fn local_device(&self) -> &DeviceInfo {
        &self.device
    }

    pub fn local_device_id(&self) -> &str {
        &self.device.id
    }

    /// 已配对设备清单。令牌只以哈希形式保存。
    pub fn devices(&self) -> Result<Vec<DeviceInfo>> {
        self.store.load_devices()
    }

    /// 配对一台新设备。`token_hash` 由调用方对令牌做摘要后传入。
    pub fn pair_device(&mut self, name: &str, kind: &str, token_hash: String) -> Result<DeviceInfo> {
        let mut d = DeviceInfo::new(
            if name.trim().is_empty() {
                "已配对设备"
            } else {
                name.trim()
            },
            if kind.trim().is_empty() { "android" } else { kind.trim() },
        );
        d.token_hash = Some(token_hash);
        self.store.upsert_device(&d)?;
        self.store
            .log(&format!("设备配对成功：{}（{}）", d.name, d.kind));
        Ok(d)
    }

    /// 撤销一台设备的配对。被撤销设备持有的令牌立即失效（服务层重启校验用）。
    pub fn revoke_device(&mut self, id: &str) -> Result<()> {
        if self.store.remove_device(id)? {
            self.store.log(&format!("撤销设备配对：{id}"));
        }
        Ok(())
    }

    pub fn touch_device(&mut self, id: &str) {
        let _ = self
            .store
            .touch_device(id, chrono::Local::now().timestamp());
    }

    /// 已配对设备的令牌摘要清单，供局域网服务做鉴权。
    pub fn paired_token_hashes(&self) -> Vec<String> {
        self.store
            .load_devices()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|d| d.token_hash)
            .collect()
    }

    /// 某账号的执行端 id（老数据为空时视为本机）。
    pub fn account_executor<'a>(&'a self, a: &'a Account) -> &'a str {
        a.executor.as_deref().unwrap_or(self.device.id.as_str())
    }

    /// 这个账号是否由本机执行（只有执行端能签到、采样、派猫猫）。
    pub fn is_local_executor(&self, a: &Account) -> bool {
        self.account_executor(a) == self.device.id
    }

    /// 把账号的执行端指派给本机。
    ///
    /// 只允许「认领到自己」：凭据不跨设备搬家，所以不能隔空把执行端塞给别的设备。
    pub fn assign_executor(&mut self, account_id: &str, device_id: Option<&str>) -> Result<()> {
        let target = device_id.unwrap_or(self.device.id.as_str());
        if target != self.device.id {
            return Err(Error::Unsupported(
                "暂不支持把执行端指派给其它设备，请在目标设备上认领".into(),
            ));
        }
        let label = {
            let acc = self
                .accounts
                .iter_mut()
                .find(|a| a.id == account_id)
                .ok_or_else(|| Error::NotFound(format!("账号 {account_id}")))?;
            acc.executor = Some(self.device.id.clone());
            acc.updated_at = chrono::Local::now().timestamp();
            acc.label.clone()
        };
        self.store.save_accounts(&self.accounts)?;
        self.store
            .log(&format!("账号 {label} 的执行端设为本机"));
        Ok(())
    }

    pub fn find(&self, id_or_label: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|a| a.id == id_or_label || a.label == id_or_label)
    }

    /// 某账号是否已导入凭据（有凭据才谈得上签到）。
    pub fn has_credential(&self, account_id: &str) -> bool {
        self.vault.get(account_id).is_some()
    }

    // ------------------------------------------------------------ 设置

    pub fn update_settings(&mut self, mut s: Settings) -> Result<()> {
        // 前端可能提交一份缺平台行的表（例如老客户端的默认值），先补齐再落盘。
        s.ensure_platform_schedules();
        self.store.save_settings(&s)?;
        self.settings = s;
        // 签到时间设置变了，该平台所有账号都要按新规则重排；
        // 没有任何改动时 reconcile 不落盘，改别的设置不会白白写一次 accounts.json。
        self.reconcile_schedules()?;
        Ok(())
    }

    /// 按每个平台的签到时间设置重算全部账号的 schedule；有变化才落盘。
    ///
    /// 返回被改动的账号数。组内顺序固定按 `(created_at, accounts.json 下标)` 升序：
    /// 用户看到的就是「先导入的账号排在前面、时间也更靠前」，不会今天一个序明天另一个序。
    /// `created_at` 只有秒级精度，同秒导入时用落盘下标兜底；不能用 uuid，
    /// 否则随机 id 会让「谁排第一」每天看起来都像随机的。
    pub fn reconcile_schedules(&mut self) -> Result<usize> {
        // 先按平台分桶（vendor -> 下标），桶内排序后再逐组重写。
        let mut groups: HashMap<Vendor, Vec<usize>> = HashMap::new();
        for (i, a) in self.accounts.iter().enumerate() {
            groups.entry(a.vendor).or_default().push(i);
        }
        let mut changed = 0usize;
        for (vendor, mut idxs) in groups {
            // 排序键用「创建时间 + accounts.json 里的下标」：`created_at` 只精确到秒，
            // 同一秒批量导入时若再拿 uuid 兜底，谁排第一就是随机的，
            // 界面上会出现「后导入的账号时间反而更早」。下标是落盘顺序，跨重启稳定。
            idxs.sort_by_key(|&i| (self.accounts[i].created_at, i));
            let total = idxs.len();
            let plan = self.settings.platform_schedule(vendor);
            for (pos, &i) in idxs.iter().enumerate() {
                let want = AccountSchedule::from_platform(
                    plan.hour,
                    plan.minute,
                    pos,
                    total,
                    plan.spread_minutes,
                );
                let cur = &self.accounts[i].schedule;
                // 三个字段全等才算没变：只比时分会让旧的 jitter_minutes 残留下来。
                if cur.hour == want.hour
                    && cur.minute == want.minute
                    && cur.jitter_minutes == want.jitter_minutes
                {
                    continue;
                }
                self.accounts[i].schedule = want;
                changed += 1;
            }
        }
        if changed > 0 {
            self.store.save_accounts(&self.accounts)?;
        }
        Ok(changed)
    }

    // ------------------------------------------------------------ 账号

    pub fn add_account(
        &mut self,
        vendor: Vendor,
        label: impl Into<String>,
        cred: Credential,
    ) -> Result<Account> {
        let label = label.into();
        let label = if label.trim().is_empty() {
            format!("{} 账号", vendor.display())
        } else {
            label.trim().to_string()
        };
        // 同平台同标签视为重复，避免用户误点两次建出两个一样的卡片。
        if self
            .accounts
            .iter()
            .any(|a| a.vendor == vendor && a.label == label)
        {
            return Err(Error::Invalid(format!(
                "{} 下已经有名为「{}」的账号",
                vendor.display(),
                label
            )));
        }
        let mut account = Account::new(vendor, label, cred.hint());
        account.travel_enabled = vendor.has_travel();
        // 导入它的设备就是执行端：凭据落在本机，签到也只能在本机跑。
        account.executor = Some(self.device.id.clone());
        account.origin_device = Some(self.device.id.clone());
        self.vault.put(&account.id, cred);
        self.vault.flush()?;
        self.accounts.push(account.clone());
        self.store.save_accounts(&self.accounts)?;
        // 新账号改变了该平台的账号总数，同平台其它账号的错峰位置跟着变，
        // 必须立刻重算，否则三号变四号时前三个的时间还停留在旧的平均分布上。
        self.reconcile_schedules()?;
        if let Some(updated) = self.accounts.iter().find(|a| a.id == account.id) {
            account = updated.clone();
        }
        // 重新导入同一账号时，旧的删除标记要清掉。
        self.store.clear_tombstone(&account.id)?;
        self.store
            .log(&format!("新增账号 {} / {}", vendor.display(), account.label));
        Ok(account)
    }

    /// 按平台账号身份 upsert：同一平台、同一用户只保留一条记录。
    ///
    /// 身份来自凭据 JWT 的 `sub`（见 `ph_providers::identity`），不按令牌片段猜。
    /// 已有账号时**更新凭据**而不是新建；老账号没有身份的，会先用本机凭据补齐再比对。
    /// 返回 `(账号, 是否更新了已有账号, 合并掉的重复条数)`。
    pub fn upsert_account(
        &mut self,
        vendor: Vendor,
        label: impl Into<String>,
        cred: Credential,
    ) -> Result<(Account, bool, usize)> {
        let uid = ph_providers::identity::platform_uid(&cred);
        if let Some(uid) = uid.as_deref() {
            self.backfill_platform_uid(vendor)?;
            let mut matches: Vec<Account> = self
                .accounts
                .iter()
                .filter(|a| a.vendor == vendor && a.platform_uid.as_deref() == Some(uid))
                .cloned()
                .collect();
            // 旧版本用「本机导入」建的记录没有 platform_uid，但令牌和新凭据是同一串；
            // 顺手合并掉，避免同一账号留两条。
            for account in &self.accounts {
                if account.vendor != vendor || matches.iter().any(|m| m.id == account.id) {
                    continue;
                }
                if self
                    .vault
                    .get(&account.id)
                    .map_or(false, |c| same_secret(&c, &cred))
                {
                    matches.push(account.clone());
                }
            }
            if !matches.is_empty() {
                let Some(keeper) = matches
                    .iter()
                    .find(|a| self.is_local_executor(a))
                    .cloned()
                else {
                    return Err(Error::Unsupported(format!(
                        "账号「{}」由另一台设备执行，本机不能同时持有一份凭据。请先在手机上处理该账号后再导入。",
                        matches[0].label
                    )));
                };
                self.update_credential(&keeper.id, cred)?;
                // 此前重复导入留下的其它记录：同一平台同一用户，合并成一条。
                let mut merged = 0;
                for dup in matches.iter().filter(|a| a.id != keeper.id) {
                    self.remove_account(&dup.id)?;
                    merged += 1;
                }
                let acc = self
                    .accounts
                    .iter()
                    .find(|a| a.id == keeper.id)
                    .cloned()
                    .unwrap_or(keeper);
                return Ok((acc, true, merged));
            }
        } else {
            // 没有 JWT 身份（例如 Trae / Qoder 的普通令牌）时，退一步用**整串令牌完全相同**
            // 判断：同一串令牌不可能属于两个账号。仍然禁止按令牌片段猜（D15）。
            let dup_ids: Vec<String> = self
                .accounts
                .iter()
                .filter(|a| {
                    a.vendor == vendor
                        && self
                            .vault
                            .get(&a.id)
                            .map_or(false, |c| same_secret(c, &cred))
                })
                .map(|a| a.id.clone())
                .collect();
            if !dup_ids.is_empty() {
                let keeper_id = dup_ids.iter().find_map(|id| {
                    let a = self.accounts.iter().find(|a| &a.id == id)?;
                    self.is_local_executor(a).then(|| id.clone())
                });
                let Some(keeper_id) = keeper_id else {
                    return Err(Error::Unsupported(
                        "该账号由另一台设备执行，本机不能同时持有一份凭据。请先在手机上处理该账号后再导入。"
                            .into(),
                    ));
                };
                self.update_credential(&keeper_id, cred)?;
                let mut merged = 0;
                for id in dup_ids.iter().filter(|id| **id != keeper_id) {
                    self.remove_account(id)?;
                    merged += 1;
                }
                let acc = self
                    .accounts
                    .iter()
                    .find(|a| a.id == keeper_id)
                    .cloned()
                    .expect("刚更新过的账号一定还在");
                return Ok((acc, true, merged));
            }
        }

        let mut account = self.add_account(vendor, label, cred)?;
        if let Some(uid) = uid {
            account.platform_uid = Some(uid);
            if let Some(a) = self.accounts.iter_mut().find(|a| a.id == account.id) {
                a.platform_uid = account.platform_uid.clone();
            }
            self.store.save_accounts(&self.accounts)?;
        }
        // `add_account` 已经重算过一次；这里再跑一遍是幂等的（无改动不落盘），
        // 保证「upsert 之后该平台的时间一定是最新规则」这个契约不依赖调用顺序。
        self.reconcile_schedules()?;
        Ok((account, false, 0))
    }

    /// 老数据补齐身份：早期导入的账号没有 `platform_uid`，但凭据里其实带 JWT。
    /// 补不齐就保持 `None`，不猜。
    fn backfill_platform_uid(&mut self, vendor: Vendor) -> Result<()> {
        let pending: Vec<String> = self
            .accounts
            .iter()
            .filter(|a| a.vendor == vendor && a.platform_uid.is_none())
            .map(|a| a.id.clone())
            .collect();
        let mut found: Vec<(String, String)> = Vec::new();
        for id in pending {
            if let Some(uid) = self
                .vault
                .get(&id)
                .and_then(ph_providers::identity::platform_uid)
            {
                found.push((id, uid));
            }
        }
        if found.is_empty() {
            return Ok(());
        }
        for (id, uid) in found {
            if let Some(a) = self.accounts.iter_mut().find(|a| a.id == id) {
                a.platform_uid = Some(uid);
            }
        }
        self.store.save_accounts(&self.accounts)
    }

    /// 记录平台昵称与导入来源，并在用户没改过名时用昵称刷新账号名。
    ///
    /// 规则：
    /// - `label_custom == false` 且拿到昵称 → `label` 换成昵称；
    /// - 用户改过名（`label_custom == true`）→ 名字不动，只更新 nickname 字段；
    /// - 昵称为空（例如手动令牌、平台没返回）→ 名字保持导入时的「平台 账号」。
    pub fn apply_identity_meta(
        &mut self,
        id: &str,
        nickname: Option<String>,
        source: &str,
    ) -> Result<()> {
        let Some(idx) = self.accounts.iter().position(|a| a.id == id) else {
            return Ok(());
        };
        let nickname = nickname
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty());
        let mut changed = false;
        {
            let acc = &mut self.accounts[idx];
            if acc.source.as_deref() != Some(source) {
                acc.source = Some(source.to_string());
                changed = true;
            }
            if acc.nickname != nickname {
                acc.nickname = nickname.clone();
                changed = true;
            }
            if !acc.label_custom {
                if let Some(name) = nickname {
                    if acc.label != name {
                        acc.label = name;
                        changed = true;
                    }
                }
            }
            if changed {
                acc.updated_at = chrono::Local::now().timestamp();
            }
        }
        if changed {
            self.store.save_accounts(&self.accounts)?;
        }
        Ok(())
    }

    /// 用户手动改名：写入 `label` 并标记为自定义，之后重复导入不会覆盖。
    pub fn rename_account(&mut self, id: &str, label: &str) -> Result<Account> {
        let label = label.trim();
        if label.is_empty() {
            return Err(Error::Invalid("账号名不能为空。".into()));
        }
        if label.chars().count() > 24 {
            return Err(Error::Invalid("账号名最多 24 个字。".into()));
        }
        let Some(idx) = self.accounts.iter().position(|a| a.id == id) else {
            return Err(Error::NotFound("找不到该账号，可能已被删除。".into()));
        };
        {
            let acc = &mut self.accounts[idx];
            acc.label = label.to_string();
            acc.label_custom = true;
            acc.updated_at = chrono::Local::now().timestamp();
        }
        self.store.save_accounts(&self.accounts)?;
        Ok(self.accounts[idx].clone())
    }
    ///
    /// 本机导入专用。凭据来自客户端存储，先验一次能避免「导入成功但根本用不了」，
    /// 也符合 ADR-0003「验证通过才落库」。同一账号已存在时更新凭据，不新建。
    pub async fn upsert_account_verified(
        &mut self,
        vendor: Vendor,
        label: impl Into<String>,
        mut cred: Credential,
    ) -> Result<(Account, VendorSnapshot, bool, usize)> {
        let label = label.into();
        let label = if label.trim().is_empty() {
            format!("{} 账号", vendor.display())
        } else {
            label.trim().to_string()
        };
        // 只用于验证，不落库；id 与真实账号无关。
        let probe = Account::new(vendor, label.clone(), cred.hint());
        let provider = provider_for(vendor);
        if provider.capabilities().token_refresh && refresh_due(&cred) {
            provider.refresh(&mut cred).await?;
        }
        // 本机导入 / 手动令牌的凭据里通常没有用户 id；验证前让适配器问一次平台，
        // 这样和浏览器授权导入的同一账号能命中同一条记录，不会各存一份。
        if cred.platform_uid.is_none() {
            if let Ok(Some(uid)) = provider.identity(&cred).await {
                cred.platform_uid = Some(uid);
            }
        }
        // 以前用本机导入建的记录没有 platform_uid（令牌轮换过就对不上整串）。
        // 借这次导入顺手问一次平台：能解析出同一个 uid 就先补上，随后 upsert 会合并。
        if let Some(uid) = cred.platform_uid.clone() {
            self.adopt_identities(vendor, &uid).await?;
        }
        let snap = provider.snapshot(&probe, &cred).await?;
        let (account, updated, merged) = self.upsert_account(vendor, label, cred)?;
        Ok((account, snap, updated, merged))
    }

    /// 把「同平台、还没有身份」的旧账号按 `uid` 补齐身份，供随后的 upsert 合并。
    ///
    /// 旧版本的本机导入拿不到平台用户 id，令牌轮换后又对不上整串令牌，
    /// 于是同一账号会留下两条。这里用旧凭据问一次平台（Trae 直接读 JWT 的 `data.id`，
    /// Qoder 走 userinfo），能对上同一个 uid 就先写上。
    pub async fn adopt_identities(&mut self, vendor: Vendor, uid: &str) -> Result<()> {
        let uid = uid.trim();
        if uid.is_empty() {
            return Ok(());
        }
        let pending: Vec<(String, Credential)> = self
            .accounts
            .iter()
            .filter(|a| a.vendor == vendor && a.platform_uid.is_none())
            .filter_map(|a| self.vault.get(&a.id).map(|c| (a.id.clone(), c.clone())))
            .collect();
        if pending.is_empty() {
            return Ok(());
        }

        let provider = provider_for(vendor);
        let mut matched = Vec::new();
        for (id, old) in pending {
            if let Ok(Some(other)) = provider.identity(&old).await {
                if other == uid {
                    matched.push(id);
                }
            }
        }
        if matched.is_empty() {
            return Ok(());
        }
        for id in &matched {
            if let Some(a) = self.accounts.iter_mut().find(|a| &a.id == id) {
                a.platform_uid = Some(uid.to_string());
            }
        }
        self.store.save_accounts(&self.accounts)
    }

    /// 替换某账号的凭据（重新导入 / 换 token）。
    pub fn update_credential(&mut self, account_id: &str, cred: Credential) -> Result<()> {
        let hint = cred.hint();
        let acc = self
            .accounts
            .iter_mut()
            .find(|a| a.id == account_id)
            .ok_or_else(|| Error::NotFound(format!("账号 {account_id}")))?;
        acc.hint = hint;
        acc.updated_at = chrono::Local::now().timestamp();
        acc.last_error = None;
        self.vault.put(account_id, cred);
        self.vault.flush()?;
        self.store.save_accounts(&self.accounts)?;
        Ok(())
    }

    pub fn remove_account(&mut self, account_id: &str) -> Result<()> {
        let before = self.accounts.len();
        self.accounts.retain(|a| a.id != account_id);
        if self.accounts.len() == before {
            return Err(Error::NotFound(format!("账号 {account_id}")));
        }
        // 删除会传播到对端：没有墓碑的话，对端下一次同步就把它带回来了。
        self.store.add_tombstone(&Tombstone {
            account_id: account_id.to_string(),
            deleted_at: chrono::Local::now().timestamp(),
            device_id: self.device.id.clone(),
        })?;
        self.vault.remove(account_id);
        self.vault.flush()?;
        self.store.save_accounts(&self.accounts)?;
        // 少了一个账号，该平台的错峰分布也变了：3 个号删到 2 个时，
        // 剩下两个要重新铺满窗口，否则界面上会出现「2 个账号 · 00:10 / 00:13」
        // 这种与「窗口内均匀错开」自相矛盾的预览。
        self.reconcile_schedules()?;
        self.store.log(&format!("删除账号 {account_id}"));
        Ok(())
    }

    pub fn set_account_enabled(&mut self, account_id: &str, enabled: bool) -> Result<()> {
        let acc = self
            .accounts
            .iter_mut()
            .find(|a| a.id == account_id)
            .ok_or_else(|| Error::NotFound(format!("账号 {account_id}")))?;
        acc.enabled = enabled;
        acc.updated_at = chrono::Local::now().timestamp();
        self.store.save_accounts(&self.accounts)?;
        Ok(())
    }

    // ------------------------------------------------------------ 采样

    /// 单个账号采样。凭据缺失时返回 NotFound，而不是编一个 0。
    pub async fn snapshot_one(&mut self, account_id: &str) -> Result<VendorSnapshot> {
        let account = self
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("账号 {account_id}")))?;
        if !self.is_local_executor(&account) {
            return Err(Error::Unsupported(format!(
                "账号「{}」由另一台设备执行，本机不重复签到",
                account.label
            )));
        }
        let cred = self
            .vault
            .get(account_id)
            .cloned()
            .ok_or_else(|| Error::Auth("该账号还没有导入凭据".into()))?;

        let provider = provider_for(account.vendor);
        let mut cred2 = cred.clone();
        // 只在令牌临近过期时续期：Qoder/Trae 续期要换令牌甚至做设备签名，
        // 每个采样周期都刷既没必要也更容易触发风控。
        if provider.capabilities().token_refresh && refresh_due(&cred2) {
            if let Err(e) = provider.refresh(&mut cred2).await {
                if cred2.access_token.is_none() {
                    self.note_error(account_id, &e.user_message());
                    return Err(e);
                }
                self.store.log(&format!(
                    "{} 刷新令牌失败但沿用旧令牌：{}",
                    account.label,
                    e.user_message()
                ));
            }
        }
        let result = provider.snapshot(&account, &cred2).await;

        // 令牌可能被轮换，必须回写。
        if cred2.refresh_token != cred.refresh_token || cred2.access_token != cred.access_token {
            self.vault.put(account_id, cred2);
            self.vault.flush()?;
        }

        match result {
            Ok(mut snap) => {
                snap.today_gain = self.estimate_gain(account_id, snap.balance);
                self.store.append_snapshot(&snap)?;
                self.clear_error(account_id);
                Ok(snap)
            }
            Err(e) => {
                self.note_error(account_id, &e.user_message());
                Err(e)
            }
        }
    }

    /// 全量采样。一个失败不影响其它，返回 (成功列表, 失败列表)。
    pub async fn snapshot_all(&mut self) -> (Vec<VendorSnapshot>, Vec<(String, String)>) {
        let ids: Vec<String> = self
            .accounts
            .iter()
            .filter(|a| a.enabled && self.is_local_executor(a))
            .map(|a| a.id.clone())
            .collect();
        let mut ok = Vec::new();
        let mut errs = Vec::new();
        for id in ids {
            match self.snapshot_one(&id).await {
                Ok(s) => ok.push(s),
                Err(e) => {
                    let label = self
                        .find(&id)
                        .map(|a| a.label.clone())
                        .unwrap_or_else(|| id.clone());
                    errs.push((label, e.user_message()));
                }
            }
        }
        (ok, errs)
    }

    /// 用「上一次余额」推算今日新增。拿不到就是 None，不硬算。
    fn estimate_gain(&self, account_id: &str, now_balance: Option<f64>) -> Option<f64> {
        let now = now_balance?;
        let history = self.store.load_snapshots(4000).ok()?;
        // 找该账号今天之前的最后一条
        let today = chrono::Local::now().date_naive();
        let prev = history
            .iter()
            .rev()
            .find(|s| {
                s.account_id == account_id
                    && ph_core::schedule::local_from_epoch(s.captured_at)
                        .map(|d| d.date() < today)
                        .unwrap_or(false)
                    && s.balance.is_some()
            })
            .and_then(|s| s.balance)?;
        let diff = now - prev;
        if diff >= 0.0 {
            Some(diff)
        } else {
            None
        }
    }

    // ------------------------------------------------------------ 签到

    pub async fn checkin_one(&mut self, account_id: &str) -> Result<CheckinOutcome> {
        let account = self
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("账号 {account_id}")))?;
        if !self.is_local_executor(&account) {
            return Err(Error::Unsupported(format!(
                "账号「{}」由另一台设备执行，本机不签到",
                account.label
            )));
        }
        let mut cred = self
            .vault
            .get(account_id)
            .cloned()
            .ok_or_else(|| Error::Auth("该账号还没有导入凭据".into()))?;

        let provider = provider_for(account.vendor);
        if provider.capabilities().token_refresh && refresh_due(&cred) {
            if let Err(e) = provider.refresh(&mut cred).await {
                if cred.access_token.is_none() {
                    let outcome = CheckinOutcome {
                        account_id: account.id.clone(),
                        vendor: account.vendor,
                        status: CheckinStatus::AuthFailed,
                        credit: None,
                        streak_days: None,
                        message: e.user_message(),
                        at: chrono::Local::now().timestamp(),
                    };
                    self.note_error(account_id, &outcome.message);
                    self.set_retry_after(account_id, outcome.status, outcome.at);
                    return Ok(outcome);
                }
            }
        }

        let outcome = provider.checkin(&account, &cred).await;

        // 无论成败，轮换后的令牌都要落盘。
        self.vault.put(account_id, cred);
        self.vault.flush()?;

        let outcome = match outcome {
            Ok(o) => o,
            Err(e) => CheckinOutcome {
                account_id: account.id.clone(),
                vendor: account.vendor,
                status: if matches!(e, Error::Auth(_)) {
                    CheckinStatus::AuthFailed
                } else if matches!(e, Error::NotEligible(_)) {
                    CheckinStatus::NotEligible
                } else {
                    CheckinStatus::Failed
                },
                credit: None,
                streak_days: None,
                message: e.user_message(),
                at: chrono::Local::now().timestamp(),
            },
        };

        // 复核一次状态，把余额刷新（顺便验证「领到了」是否真的到账）。
        let cred_after = self.vault.get(account_id).cloned().unwrap_or_default();
        match provider.snapshot(&account, &cred_after).await {
            Ok(mut snap) => {
                snap.today_gain = self.estimate_gain(account_id, snap.balance);
                let _ = self.store.append_snapshot(&snap);
            }
            Err(e) => {
                self.store
                    .log(&format!("{} 签到后复核状态失败：{}", account.label, e.user_message()));
            }
        }

        // 记账
        if let Some(acc) = self.accounts.iter_mut().find(|a| a.id == account_id) {
            if outcome.status.is_ok() {
                acc.last_checkin_at = Some(outcome.at);
                acc.last_error = None;
                acc.retry_after = None;
                acc.last_failure_notice_at = None;
            } else {
                acc.last_error = Some(outcome.message.clone());
                acc.retry_after = Some(outcome.at + retry_delay_secs(outcome.status, outcome.at));
            }
            acc.updated_at = chrono::Local::now().timestamp();
        }
        self.store.save_accounts(&self.accounts)?;
        self.store.log(&format!(
            "签到 {} / {} -> {} {}",
            account.vendor.display(),
            account.label,
            outcome.status.label(),
            outcome.message
        ));
        // 记流水：界面「最近活动」直接读它，不再从日志里正则捞。
        let _ = self.store.append_event(&ActivityEvent::checkin(
            &account,
            outcome.at,
            outcome.status.label(),
            outcome.credit,
        ));
        Ok(outcome)
    }

    /// 一键全部签到。逐个串行 + 随机间隔，避免同秒打上游触发风控。
    pub async fn checkin_all(&mut self) -> Vec<CheckinOutcome> {
        let ids: Vec<String> = self
            .accounts
            .iter()
            .filter(|a| a.enabled && a.checkin_enabled && self.is_local_executor(a))
            .map(|a| a.id.clone())
            .collect();
        let mut out = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                // 300~900ms 抖动：既不像脚本轰炸，也不会让用户等太久。
                let ms = 300 + (rand::random::<u64>() % 600);
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            }
            match self.checkin_one(id).await {
                Ok(o) => out.push(o),
                Err(e) => {
                    let acc = self.find(id).cloned();
                    out.push(CheckinOutcome {
                        account_id: id.clone(),
                        vendor: acc.as_ref().map(|a| a.vendor).unwrap_or(Vendor::WorkBuddy),
                        status: CheckinStatus::Failed,
                        credit: None,
                        streak_days: None,
                        message: e.user_message(),
                        at: chrono::Local::now().timestamp(),
                    });
                }
            }
        }
        out
    }

    // ------------------------------------------------------------ 派猫猫旅行

    pub async fn travel_one(
        &mut self,
        account_id: &str,
        action: TravelAction,
        location_id: Option<u32>,
    ) -> Result<TravelSnapshot> {
        let account = self
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("账号 {account_id}")))?;
        if !self.is_local_executor(&account) {
            return Err(Error::Unsupported(format!(
                "账号「{}」由另一台设备执行，本机不派猫猫",
                account.label
            )));
        }
        if !account.vendor.has_travel() {
            return Err(Error::Unsupported(format!(
                "{} 没有附件活动",
                account.vendor.display()
            )));
        }
        let mut cred = self
            .vault
            .get(account_id)
            .cloned()
            .ok_or_else(|| Error::Auth("该账号还没有导入凭据".into()))?;
        let provider = provider_for(account.vendor);
        let _ = provider.refresh(&mut cred).await;
        let result = provider.travel(&account, &cred, action, location_id).await;
        self.vault.put(account_id, cred);
        self.vault.flush()?;
        if let Ok(snap) = &result {
            let text = match (&snap.location, snap.note.as_deref()) {
                (Some(loc), Some(note)) => format!("{} {loc}（{note}）", snap.state.label()),
                (Some(loc), None) => format!("{} {loc}", snap.state.label()),
                (None, Some(note)) => format!("{}（{note}）", snap.state.label()),
                (None, None) => snap.state.label().to_string(),
            };
            let _ = self.store.append_event(&ActivityEvent::travel(
                &account,
                chrono::Local::now().timestamp(),
                text,
                snap.reward_credit,
            ));
        }
        result
    }

    // ------------------------------------------------------------ 汇总

    pub fn aggregate(&self) -> Aggregate {
        let latest = self.store.latest_by_account().unwrap_or_default();
        let mut agg = Aggregate {
            account_count: self.accounts.len(),
            ..Default::default()
        };
        let mut total = 0f64;
        for a in &self.accounts {
            if !a.enabled {
                continue;
            }
            if let Some(s) = latest.get(&a.id) {
                if let Some(b) = s.balance {
                    total += b;
                    agg.balance_known += 1;
                }
                if s.checked_in_today == Some(false) {
                    agg.pending_count += 1;
                }
                if agg.last_sampled_at.map(|t| s.captured_at > t).unwrap_or(true) {
                    agg.last_sampled_at = Some(s.captured_at);
                }
            } else {
                agg.pending_count += 1;
            }
        }
        agg.total_balance = total;
        agg.claimed_this_month = Some(self.claimed_this_month());
        agg
    }

    /// 本月（本地时区）签到实际到账的积分。
    ///
    /// 口径是「领取流水」而不是余额差：只累加 `kind == "checkin"` 且 `delta > 0`
    /// 的事件。派猫猫旅行花掉的积分、失败记录、负向调整都不计入，
    /// 这样「本月领取」不会因为消费而变小。
    pub fn claimed_this_month(&self) -> f64 {
        let from = month_start_epoch();
        self.store
            .load_all_events()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.at >= from && e.kind == "checkin")
            .filter_map(|e| e.delta)
            .filter(|d| *d > 0)
            .sum::<i64>() as f64
    }

    pub fn latest_snapshots(&self) -> std::collections::BTreeMap<String, VendorSnapshot> {
        self.store.latest_by_account().unwrap_or_default()
    }

    /// 近 `days` 天的每日余额合计（含今天，按日期升序）。
    ///
    /// 规则：取每天最后一次采样；只统计**当前仍纳管**的账号——
    /// 删掉的账号不该继续占着历史合计，否则曲线会莫名掉一截。
    pub fn daily_series(&self, days: i64) -> Vec<DayPoint> {
        let today = chrono::Local::now().date_naive();
        let from = today - chrono::Duration::days(days.max(1) - 1);
        self.series(None, from, today)
    }

    /// 指定平台（`None` 为全部）与日期区间的每日序列。
    ///
    /// 会多读一天作为首日基线：没有前一天的总额就算不出「新增 / 消耗」，
    /// 那样月度视图的第一天就会凭空缺一格。
    pub fn series(
        &self,
        vendor: Option<Vendor>,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Vec<DayPoint> {
        self.series_dated(vendor, from, to)
            .into_iter()
            .map(|(_, p)| p)
            .collect()
    }

    /// 每日序列的带日期版本（月度汇总要按真实年月分组，`MM-DD` 跨年会有歧义）。
    fn series_dated(
        &self,
        vendor: Option<Vendor>,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Vec<(chrono::NaiveDate, DayPoint)> {
        use chrono::Datelike;
        use std::collections::BTreeMap;

        if to < from {
            return Vec::new();
        }
        let scan_from = from - chrono::Duration::days(1);
        // 只看这个平台（或全部）现存账号，与合计口径保持一致。
        let ids: Vec<&str> = self
            .accounts
            .iter()
            .filter(|a| vendor.is_none_or(|v| a.vendor == v))
            .map(|a| a.id.as_str())
            .collect();

        // date -> account_id -> 当天最后余额
        let mut per_day: BTreeMap<chrono::NaiveDate, BTreeMap<String, f64>> = BTreeMap::new();
        if let Ok(snaps) = self.store.load_snapshots(20000) {
            for s in snaps {
                if !ids.iter().any(|id| *id == s.account_id) {
                    continue;
                }
                let Some(dt) = ph_core::schedule::local_from_epoch(s.captured_at) else {
                    continue;
                };
                let d = dt.date();
                if d < scan_from || d > to {
                    continue;
                }
                if let Some(b) = s.balance {
                    // 顺序遍历，后写覆盖前写，天然得到「当天最后一次」。
                    per_day.entry(d).or_default().insert(s.account_id.clone(), b);
                }
            }
        }

        // 每天签到实际领取之和：只认 checkin 且 delta>0，重复签到记的是 None，不进这里。
        let mut claimed: BTreeMap<chrono::NaiveDate, f64> = BTreeMap::new();
        if let Ok(events) = self.store.load_all_events() {
            for e in events {
                if e.kind != "checkin" {
                    continue;
                }
                if vendor.is_some_and(|v| e.vendor != v) {
                    continue;
                }
                let Some(delta) = e.delta.filter(|d| *d > 0) else {
                    continue;
                };
                let Some(dt) = ph_core::schedule::local_from_epoch(e.at) else {
                    continue;
                };
                let d = dt.date();
                if d < from || d > to {
                    continue;
                }
                *claimed.entry(d).or_default() += delta as f64;
            }
        }

        // 首日基线：区间前一天的合计。
        let mut prev: Option<f64> = per_day.get(&scan_from).map(|m| {
            ids.iter()
                .filter_map(|id| m.get(*id))
                .fold(0f64, |acc, b| acc + *b)
        });

        let mut out = Vec::new();
        let mut d = from;
        while d <= to {
            let mut total = 0f64;
            let mut known = 0usize;
            if let Some(m) = per_day.get(&d) {
                for id in &ids {
                    if let Some(b) = m.get(*id) {
                        total += *b;
                        known += 1;
                    }
                }
            }
            // 「新增」直接取当天领取流水，与余额差无关。
            let claimed_today = claimed.get(&d).copied().unwrap_or(0.0);
            // 「消耗」用 昨日余额 + 今日领取 − 今日余额 反推；没有前一天数据就算不出来。
            let (gain, down) = match prev {
                Some(pt) if known > 0 => {
                    let net = total - pt;
                    (Some(net), Some((claimed_today - net).max(0.0)))
                }
                _ => (None, None),
            };
            if known > 0 {
                prev = Some(total);
            }
            out.push((
                d,
                DayPoint {
                    date: format!("{:02}-{:02}", d.month(), d.day()),
                    total,
                    known,
                    gain,
                    up: Some(claimed_today),
                    down,
                },
            ));
            d += chrono::Duration::days(1);
        }
        out
    }

    /// 某一年 1~12 月的汇总，**固定返回 12 条**，按月份升序。
    ///
    /// 年度视图必须能画出完整的 12 格，所以没有采样的月也占位（`days == 0`、
    /// `total`/`up`/`down` 为 0），由前端决定是断线还是提示「还没有数据」。
    /// 未来的月份不扫：区间右端取「该年最后一天」与今天中较小的那个。
    ///
    /// `series_dated` 会从 `from - 1` 取基线，所以 1 月的「消耗」还能看到去年
    /// 12 月 31 日的余额，跨年不串月。
    pub fn monthly_series_of_year(&self, vendor: Option<Vendor>, year: i32) -> Vec<MonthPoint> {
        use chrono::Datelike;

        let today = chrono::Local::now().date_naive();
        let Some(first) = chrono::NaiveDate::from_ymd_opt(year, 1, 1) else {
            return Vec::new();
        };
        let last = chrono::NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or(first);
        // 整年都在未来：12 个空月份照样返回，前端不用为「未来年份」写特例。
        let mut out: Vec<MonthPoint> = (1..=12)
            .map(|m| MonthPoint {
                month: format!("{year:04}-{m:02}"),
                ..Default::default()
            })
            .collect();
        let to = last.min(today);
        if to < first {
            return out;
        }

        for (d, p) in self.series_dated(vendor, first, to) {
            // 整天没采样：不建条目，该月保持 days == 0。
            if p.known == 0 {
                continue;
            }
            let slot = &mut out[d.month() as usize - 1];
            // 日期升序覆盖写，最后一次写到的就是「该月最后一个有数据日」。
            slot.total = p.total;
            slot.days += 1;
            slot.up += p.up.unwrap_or(0.0);
            slot.down += p.down.unwrap_or(0.0);
        }
        out
    }

    /// 最近的活动流水（新的在前）。
    pub fn recent_events(&self, limit: usize) -> Vec<ActivityEvent> {
        self.store.load_events(limit).unwrap_or_default()
    }

    // ------------------------------------------------------------ 调度支持

    /// 今天还没执行、且已过执行时间点的账号（用于「启动补签」）。
    pub fn due_accounts(&self, now: chrono::NaiveDateTime) -> Vec<Account> {
        let today = now.date();
        let now_ts = local_epoch(now);
        self.accounts
            .iter()
            // 不是本机执行的账号签不了，选中只会制造「失败」与通知噪音。
            .filter(|a| a.enabled && a.checkin_enabled && self.is_local_executor(a))
            .filter(|a| a.schedule.due_today(now, &a.id))
            .filter(|a| {
                // 今天已经签过的就不再补
                a.last_checkin_at
                    .and_then(ph_core::schedule::local_from_epoch)
                    .map(|d| d.date() < today)
                    .unwrap_or(true)
            })
            // 失败后的退避窗口内不自动重试；手动签到不走这里。
            .filter(|a| a.retry_after.map(|t| t <= now_ts).unwrap_or(true))
            .cloned()
            .collect()
    }

    /// 今天到点、仍未签成功、且最近一次有错误信息的账号。
    ///
    /// 给总览页做「今日签到异常」提示用：失败被退避后不会一直弹通知，
    /// 但用户仍需要在界面上看到「今天还有账号没签成功」。
    pub fn checkin_alerts(&self, now: chrono::NaiveDateTime) -> Vec<CheckinAlert> {
        let today = now.date();
        self.accounts
            .iter()
            .filter(|a| a.enabled && a.checkin_enabled && self.is_local_executor(a))
            .filter(|a| a.schedule.due_today(now, &a.id))
            .filter(|a| a.last_error.is_some())
            .filter(|a| {
                !a.last_checkin_at
                    .and_then(ph_core::schedule::local_from_epoch)
                    .map(|d| d.date() == today)
                    .unwrap_or(false)
            })
            .map(|a| CheckinAlert {
                account_id: a.id.clone(),
                label: a.label.clone(),
                message: a.last_error.clone().unwrap_or_default(),
            })
            .collect()
    }

    /// 标记「这个账号今天的失败通知已经发过」。
    ///
    /// 返回 `true` 表示本次是今天第一次失败，调用方应当发系统通知；
    /// 同一天后续失败返回 `false`，只在总览页提示，不重复弹窗。
    pub fn mark_failure_notice(&mut self, account_id: &str) -> bool {
        let Some(today) = today_start_epoch() else {
            return false;
        };
        let Some(acc) = self.accounts.iter_mut().find(|a| a.id == account_id) else {
            return false;
        };
        if acc.last_failure_notice_at == Some(today) {
            return false;
        }
        acc.last_failure_notice_at = Some(today);
        acc.updated_at = chrono::Local::now().timestamp();
        let _ = self.store.save_accounts(&self.accounts);
        true
    }

    /// 下一次需要醒来的时间点（None 表示没有启用任何账号）。
    pub fn next_wakeup(&self, now: chrono::NaiveDateTime) -> Option<chrono::NaiveDateTime> {
        self.accounts
            .iter()
            .filter(|a| a.enabled && a.checkin_enabled)
            .map(|a| a.schedule.next_run_after(now, &a.id))
            .min()
    }

    /// 下一次自动签到的时刻（epoch 秒），给界面直接显示。
    pub fn next_run_at(&self) -> Option<i64> {
        use chrono::TimeZone;
        let now = chrono::Local::now().naive_local();
        let next = self.next_wakeup(now)?;
        chrono::Local
            .from_local_datetime(&next)
            .single()
            .map(|d| d.timestamp())
    }

    // ------------------------------------------------------------ 双端同步

    /// 打包一份同步载荷。`since` 为 0 表示全量。
    ///
    /// 只含账号元数据、快照、活动与墓碑；凭据永远不在这里（类型上就没有字段）。
    pub fn sync_export(&self, since: i64) -> SyncPayload {
        let mut p = SyncPayload::new(&self.device, since);
        p.accounts = self.accounts.clone();
        p.snapshots = self
            .store
            .load_all_snapshots()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.captured_at > since)
            .collect();
        p.events = self
            .store
            .load_all_events()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.at > since)
            .collect();
        p.tombstones = self.store.load_tombstones().unwrap_or_default();
        p.credential_ids = self
            .accounts
            .iter()
            .filter(|a| self.vault.get(&a.id).is_some())
            .map(|a| a.id.clone())
            .collect();
        p.next_run_at = self.next_run_at();
        p.claimed_this_month = Some(self.claimed_this_month());
        p
    }

    /// 合并对端载荷。幂等：同一份载荷重复合并不会产生重复数据。
    pub fn sync_merge(&mut self, payload: &SyncPayload) -> Result<MergeReport> {
        if payload.protocol != SYNC_PROTOCOL {
            return Err(Error::Unsupported(format!(
                "同步协议不一致：对端 v{}，本机 v{SYNC_PROTOCOL}",
                payload.protocol
            )));
        }
        let now = chrono::Local::now().timestamp();
        let mut report = MergeReport::default();

        // 记录对端设备；已配对设备保留原有令牌哈希，不被载荷覆盖。
        if payload.from_device != self.device.id {
            let mut peer = self
                .store
                .load_devices()?
                .into_iter()
                .find(|d| d.id == payload.from_device)
                .unwrap_or_else(|| DeviceInfo {
                    id: payload.from_device.clone(),
                    name: payload.from_name.clone(),
                    kind: payload.from_kind.clone(),
                    token_hash: None,
                    paired_at: now,
                    last_seen_at: None,
                });
            if !payload.from_name.trim().is_empty() {
                peer.name = payload.from_name.clone();
            }
            peer.last_seen_at = Some(now);
            self.store.upsert_device(&peer)?;
        }

        // 1. 墓碑：对端删掉的账号，本机也删（除非本机在删除之后还改过）。
        let mut tombstones = self.store.load_tombstones()?;
        let mut cred_dirty = false;
        for t in &payload.tombstones {
            if let Some(local) = self
                .accounts
                .iter()
                .find(|a| a.id == t.account_id)
                .cloned()
            {
                if local.updated_at > t.deleted_at {
                    report.conflicts.push(format!(
                        "账号「{}」在删除后又被修改，已保留",
                        local.label
                    ));
                } else {
                    self.accounts.retain(|a| a.id != t.account_id);
                    self.vault.remove(&t.account_id);
                    cred_dirty = true;
                    report.accounts_removed += 1;
                }
            }
            match tombstones.iter_mut().find(|x| x.account_id == t.account_id) {
                Some(slot) => {
                    if t.deleted_at > slot.deleted_at {
                        *slot = t.clone();
                    }
                }
                None => tombstones.push(t.clone()),
            }
        }
        if cred_dirty {
            self.vault.flush()?;
        }
        self.store.save_tombstones(&tombstones)?;

        let deleted: HashMap<String, i64> = tombstones
            .iter()
            .map(|t| (t.account_id.clone(), t.deleted_at))
            .collect();

        // 2. 账号：id 相同直接用；否则按平台稳定 id 认人（禁止按令牌片段猜）。
        let mut changed = false;
        let mut id_map: HashMap<String, String> = HashMap::new();
        for remote in &payload.accounts {
            if deleted
                .get(&remote.id)
                .map(|d| remote.updated_at <= *d)
                .unwrap_or(false)
            {
                continue;
            }
            let local_idx = self
                .accounts
                .iter()
                .position(|a| a.id == remote.id)
                .or_else(|| {
                    remote.platform_uid.as_ref().and_then(|uid| {
                        self.accounts.iter().position(|a| {
                            a.vendor == remote.vendor
                                && a.platform_uid.as_deref() == Some(uid.as_str())
                        })
                    })
                });

            match local_idx {
                None => {
                    let mut a = remote.clone();
                    a.executor = Some(
                        remote
                            .executor
                            .clone()
                            .unwrap_or_else(|| payload.from_device.clone()),
                    );
                    a.origin_device = Some(
                        remote
                            .origin_device
                            .clone()
                            .unwrap_or_else(|| payload.from_device.clone()),
                    );
                    self.accounts.push(a);
                    report.accounts_added += 1;
                    changed = true;
                }
                Some(i) => {
                    let local = self.accounts[i].clone();
                    if remote.id != local.id {
                        id_map.insert(remote.id.clone(), local.id.clone());
                    }
                    if remote.updated_at > local.updated_at {
                        let mut merged = remote.clone();
                        // 本机 id 不漂：对端快照/活动随后会按 id_map 改写到这个 id 上。
                        merged.id = local.id.clone();
                        // 执行端以本机已指派的为准；两端不一致时只报冲突，不悄悄改。
                        if let (Some(le), Some(re)) = (&local.executor, &remote.executor) {
                            if le != re {
                                report.conflicts.push(format!(
                                    "账号「{}」两端执行端不同（本机 {} / 对端 {}），已保留本机",
                                    local.label, le, re
                                ));
                            }
                        }
                        merged.executor = local.executor.clone().or_else(|| remote.executor.clone());
                        merged.updated_at = remote.updated_at;
                        self.accounts[i] = merged;
                        report.accounts_updated += 1;
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.store.save_accounts(&self.accounts)?;
            // 对端带来的 `schedule` 可能是它那边的旧规则（甚至还是按日哈希的老数据），
            // 合并后立刻按本机平台设置重算，避免「界面显示的时间」与「实际调度」错位到下次启动。
            self.reconcile_schedules()?;
        }

        // 3. 快照 / 活动：先按 id_map 改写账号 id，再做幂等合并。
        let snapshots: Vec<VendorSnapshot> = payload
            .snapshots
            .iter()
            .map(|s| {
                let mut s = s.clone();
                if let Some(nid) = id_map.get(&s.account_id) {
                    s.account_id = nid.clone();
                }
                s
            })
            .collect();
        let events: Vec<ActivityEvent> = payload
            .events
            .iter()
            .map(|e| {
                let mut e = e.clone();
                if let Some(nid) = id_map.get(&e.account_id) {
                    e.account_id = nid.clone();
                }
                e
            })
            .collect();
        report.snapshots_added = self.store.merge_snapshots(&snapshots)?;
        report.events_added = self.store.merge_events(&events)?;

        self.store.log(&format!(
            "同步合并 {}：账号 +{} ~{} -{}，快照 +{}，活动 +{}，冲突 {}",
            payload.from_name,
            report.accounts_added,
            report.accounts_updated,
            report.accounts_removed,
            report.snapshots_added,
            report.events_added,
            report.conflicts.len()
        ));
        Ok(report)
    }

    fn note_error(&mut self, account_id: &str, msg: &str) {
        if let Some(acc) = self.accounts.iter_mut().find(|a| a.id == account_id) {
            acc.last_error = Some(msg.to_string());
        }
        let _ = self.store.save_accounts(&self.accounts);
    }

    /// 记录失败后的退避截止时间，避免调度器每 45 秒重试同一件事。
    fn set_retry_after(&mut self, account_id: &str, status: CheckinStatus, at: i64) {
        if let Some(acc) = self.accounts.iter_mut().find(|a| a.id == account_id) {
            acc.retry_after = Some(at + retry_delay_secs(status, at));
        }
        let _ = self.store.save_accounts(&self.accounts);
    }

    fn clear_error(&mut self, account_id: &str) {
        if let Some(acc) = self.accounts.iter_mut().find(|a| a.id == account_id) {
            if acc.last_error.is_some() {
                acc.last_error = None;
                let _ = self.store.save_accounts(&self.accounts);
            }
        }
    }
}

/// 两份额度的凭据是否持有**完全相同**的令牌。
/// 只比整串，不做任何片段匹配（D15 明确禁止按令牌片段猜身份）。
fn same_secret(a: &Credential, b: &Credential) -> bool {
    fn eq(x: Option<&str>, y: Option<&str>) -> bool {
        match (x, y) {
            (Some(x), Some(y)) => !x.is_empty() && x == y,
            _ => false,
        }
    }
    eq(a.refresh_token.as_deref(), b.refresh_token.as_deref())
        || eq(a.access_token.as_deref(), b.access_token.as_deref())
}

/// 是否该续期：有 refresh token，且 access token 已过期或 5 分钟内过期。
///
/// 本机导入的凭据没有 refresh token，这里直接返回 false；没有过期时间的
/// （老数据）保守地每次都续期，与之前行为一致。
fn refresh_due(cred: &Credential) -> bool {
    if cred.refresh_token.is_none() {
        return false;
    }
    match cred.expires_at {
        Some(exp) => exp - chrono::Local::now().timestamp() < 300,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ph_core::Credential;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ph-engine-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// 造一个带 `sub` 的 JWT（不验签，仅测试身份识别）。
    fn jwt_sub(sub: &str) -> String {
        use base64::Engine as _;
        let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.sig",
            enc(br#"{"alg":"RS256","typ":"JWT"}"#),
            enc(serde_json::json!({ "sub": sub }).to_string().as_bytes())
        )
    }

    #[test]
    fn payload_never_carries_credentials() {
        let dir = tmpdir("nocred");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let token = "refresh-token-SHOULD-NOT-LEAK-123456";
        e.add_account(
            Vendor::WorkBuddy,
            "主号",
            Credential::from_refresh_token(token),
        )
        .unwrap();

        let json = serde_json::to_string(&e.sync_export(0)).unwrap();
        assert!(!json.contains(token));
        assert!(!json.contains("refresh_token"));
        assert!(!json.contains("access_token"));
        assert!(!json.contains("\"cookie\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn claimed_this_month_counts_only_positive_checkins_this_month() {
        let dir = tmpdir("month-claimed");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let acc = e
            .add_account(
                Vendor::WorkBuddy,
                "主号",
                Credential::from_refresh_token("tok-dddddddddddd"),
            )
            .unwrap();

        let now = chrono::Local::now().timestamp();
        let before_month = month_start_epoch() - 1;
        let ev = |at: i64, kind: &str, delta: Option<i64>| ph_core::ActivityEvent {
            at,
            account_id: acc.id.clone(),
            label: acc.label.clone(),
            vendor: Vendor::WorkBuddy,
            kind: kind.into(),
            delta,
            text: "测试".into(),
        };

        e.store.append_event(&ev(now, "checkin", Some(10))).unwrap();
        e.store.append_event(&ev(now, "checkin", Some(5))).unwrap();
        // 下面四种都不该算进「本月领取」
        e.store.append_event(&ev(now, "travel", Some(100))).unwrap();
        e.store.append_event(&ev(now, "checkin", None)).unwrap();
        e.store.append_event(&ev(now, "checkin", Some(-3))).unwrap();
        e.store
            .append_event(&ev(before_month, "checkin", Some(50)))
            .unwrap();

        assert_eq!(e.claimed_this_month(), 15.0);
        assert_eq!(e.aggregate().claimed_this_month, Some(15.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn daily_series_takes_new_from_claims_and_derives_spend() {
        // 用户例子：昨日 100，今日签到领 300，用掉一部分后余额剩 50。
        // 新增应为 300（领取），消耗应为 100 + 300 − 50 = 350，净值 −50。
        let dir = tmpdir("trend-claim");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let acc = e
            .add_account(
                Vendor::WorkBuddy,
                "主号",
                Credential::from_refresh_token("tok-claim-123456"),
            )
            .unwrap();

        let today = chrono::Local::now().date_naive();
        let yesterday = today - chrono::Duration::days(1);
        let at = |d: chrono::NaiveDate, h: u32| local_epoch(d.and_hms_opt(h, 0, 0).unwrap());

        let mut snap = ph_core::VendorSnapshot::empty(&acc, at(yesterday, 20));
        snap.balance = Some(100.0);
        e.store.append_snapshot(&snap).unwrap();
        let mut snap = ph_core::VendorSnapshot::empty(&acc, at(today, 18));
        snap.balance = Some(50.0);
        e.store.append_snapshot(&snap).unwrap();

        e.store
            .append_event(&ph_core::ActivityEvent::checkin(
                &acc,
                at(today, 10),
                "已领取",
                Some(300),
            ))
            .unwrap();
        // 重复签到记的是 None，不该被算成领取
        e.store
            .append_event(&ph_core::ActivityEvent::checkin(
                &acc,
                at(today, 11),
                "今天已签",
                None,
            ))
            .unwrap();

        let series = e.daily_series(2);
        let last = series.last().unwrap();
        assert_eq!(last.up, Some(300.0), "新增只看签到领取流水");
        assert_eq!(last.down, Some(350.0), "消耗 = 昨日余额 + 今日领取 − 今日余额");
        assert_eq!(last.gain, Some(-50.0), "净值仍是余额差");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn yearly_series_is_twelve_slots_and_respects_vendor() {
        use chrono::Datelike;

        let dir = tmpdir("trend-vendor-month");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let wb = e
            .add_account(
                Vendor::WorkBuddy,
                "WB",
                Credential::from_refresh_token("tok-wb-123456"),
            )
            .unwrap();
        let trae = e
            .add_account(
                Vendor::TraeCn,
                "Trae",
                Credential::from_refresh_token("tok-trae-123456"),
            )
            .unwrap();

        let today = chrono::Local::now().date_naive();
        let yesterday = today - chrono::Duration::days(1);
        let at = |d: chrono::NaiveDate, h: u32| local_epoch(d.and_hms_opt(h, 0, 0).unwrap());

        // 昨天：WB 100、Trae 500
        for (acc, bal) in [(&wb, 100.0), (&trae, 500.0)] {
            let mut snap = ph_core::VendorSnapshot::empty(acc, at(yesterday, 10));
            snap.balance = Some(bal);
            e.store.append_snapshot(&snap).unwrap();
        }
        // 今天：WB 领 100 后 200；Trae 不动
        for (acc, bal) in [(&wb, 200.0), (&trae, 500.0)] {
            let mut snap = ph_core::VendorSnapshot::empty(acc, at(today, 10));
            snap.balance = Some(bal);
            e.store.append_snapshot(&snap).unwrap();
        }
        e.store
            .append_event(&ph_core::ActivityEvent::checkin(
                &wb,
                at(today, 9),
                "已领取",
                Some(100),
            ))
            .unwrap();

        // 全平台：总额 700，新增 100
        let all = e.daily_series(2);
        let last = all.last().unwrap();
        assert_eq!(last.total, 700.0);
        assert_eq!(last.up, Some(100.0));

        // 只算 WorkBuddy：总额 200，Trae 的 500 不该出现
        let only = e.series(Some(Vendor::WorkBuddy), yesterday, today);
        assert_eq!(only.last().unwrap().total, 200.0);
        assert_eq!(only.last().unwrap().up, Some(100.0));

        // 只算 Trae：没有领取流水，新增为 0
        let trae_only = e.series(Some(Vendor::TraeCn), yesterday, today);
        assert_eq!(trae_only.last().unwrap().total, 500.0);
        assert_eq!(trae_only.last().unwrap().up, Some(0.0));

        // 年度视图：固定 12 条，有数据的月填数、没采样的月占位
        let months = e.monthly_series_of_year(None, today.year());
        assert_eq!(months.len(), 12, "年度视图必须固定 12 条，缺月用 days==0 占位");
        let cur_key = format!("{:04}-{:02}", today.year(), today.month());
        let m = months
            .iter()
            .find(|x| x.month == cur_key)
            .expect("本月应在这 12 条里");
        // 今天是 1 号时，昨天已经落在上个月，本月只有今天一天有数据。
        let expect_days = if yesterday.month() == today.month() {
            2
        } else {
            1
        };
        assert_eq!(m.days, expect_days, "有余额数据的天数");
        assert_eq!(m.total, 700.0, "月度总额取该月最后一个有数据日");
        assert_eq!(m.up, 100.0);
        match m.down {
            // 昨天 600 → 今天 700，领了 100：没有消耗
            d => assert!(d.abs() < 1e-9, "消耗应为 0，实际 {d}"),
        }

        // 其余月份（除今天/昨天所在月）必须是空占位
        let prev_key = format!("{:04}-{:02}", yesterday.year(), yesterday.month());
        for other in months.iter().filter(|x| x.month != cur_key && x.month != prev_key) {
            assert_eq!(other.days, 0, "{} 没有采样，应占位为 0 天", other.month);
            assert_eq!(other.total, 0.0, "{} 没有采样，总额应为 0", other.month);
            assert_eq!(other.up, 0.0);
            assert_eq!(other.down, 0.0);
        }
        if prev_key != cur_key {
            let p = months
                .iter()
                .find(|x| x.month == prev_key)
                .expect("昨天所在月应在这 12 条里");
            assert_eq!(p.days, 1, "昨天是唯一有数据的一天");
            assert_eq!(p.total, 600.0);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn monthly_series_of_year_keeps_months_apart_across_year() {
        use chrono::Datelike;

        let dir = tmpdir("trend-across-months");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let acc = e
            .add_account(
                Vendor::WorkBuddy,
                "WB",
                Credential::from_refresh_token("tok-across-123456"),
            )
            .unwrap();

        let today = chrono::Local::now().date_naive();
        // 跨年最容易串月的位置：去年 12 月 31 日 → 今年 1 月 1 日
        let prev_year = today.year() - 1;
        let dec31 = chrono::NaiveDate::from_ymd_opt(prev_year, 12, 31).unwrap();
        let jan1 = chrono::NaiveDate::from_ymd_opt(prev_year + 1, 1, 1).unwrap();
        let at = |d: chrono::NaiveDate, h: u32| local_epoch(d.and_hms_opt(h, 0, 0).unwrap());

        let mut snap = ph_core::VendorSnapshot::empty(&acc, at(dec31, 10));
        snap.balance = Some(500.0);
        e.store.append_snapshot(&snap).unwrap();

        let mut snap = ph_core::VendorSnapshot::empty(&acc, at(jan1, 10));
        snap.balance = Some(600.0);
        e.store.append_snapshot(&snap).unwrap();
        e.store
            .append_event(&ph_core::ActivityEvent::checkin(
                &acc,
                at(jan1, 9),
                "已领取",
                Some(100),
            ))
            .unwrap();

        // 去年：只有 12 月有数据，且不能把今年 1 月的数算进来
        let last = e.monthly_series_of_year(None, prev_year);
        assert_eq!(last.len(), 12, "固定 12 条");
        let dec_key = format!("{prev_year:04}-12");
        let dec = last.iter().find(|m| m.month == dec_key).expect("12 月应存在");
        assert_eq!(dec.days, 1, "12 月只有 12-31 一天有数据");
        assert_eq!(dec.total, 500.0, "去年 12 月不该混进 1 月的余额");
        assert_eq!(dec.up, 0.0, "12 月没有领取流水");
        assert_eq!(
            last.iter().filter(|m| m.month != dec_key).count(),
            11,
            "其余 11 个月都占位"
        );
        assert!(
            last.iter().all(|m| m.month == dec_key || m.days == 0),
            "没有采样的月必须 days == 0"
        );

        // 今年：1 月的总额是 1-1 的 600，不是 500+600；消耗按去年 12-31 的基线算
        let cur = e.monthly_series_of_year(None, prev_year + 1);
        assert_eq!(cur.len(), 12);
        let jan_key = format!("{:04}-01", prev_year + 1);
        let jan = cur.iter().find(|m| m.month == jan_key).expect("1 月应存在");
        assert_eq!(jan.days, 1, "1 月只有 1-1 一天有数据");
        assert_eq!(jan.total, 600.0, "月度总额取该月最后一个有数据日");
        assert_eq!(jan.up, 100.0);
        match jan.down {
            // 基线 500 + 领取 100 − 今天 600 = 0
            d => assert!(d.abs() < 1e-9, "消耗应为 0，实际 {d}"),
        }

        // 整年都在未来：仍是 12 条空占位，前端不用为未来年份写特例
        let future = e.monthly_series_of_year(None, today.year() + 1);
        assert_eq!(future.len(), 12);
        assert!(future.iter().all(|m| m.days == 0), "未来年份没有采样");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 某账号当前的签到时刻（`HH:MM`），断言用。
    fn hhmm_of(e: &Engine, account_id: &str) -> String {
        let a = e
            .accounts()
            .iter()
            .find(|a| a.id == account_id)
            .expect("账号应当存在");
        format!("{:02}:{:02}", a.schedule.hour, a.schedule.minute)
    }

    /// 把一批账号的时刻排好序再比较：`created_at` 精度到秒，
    /// 同一秒内连续导入时组内先后由 id 兜底，具体谁排第一不该被测试写死。
    fn sorted_hhmm(e: &Engine, ids: &[String]) -> Vec<String> {
        let mut v: Vec<String> = ids.iter().map(|id| hhmm_of(e, id)).collect();
        v.sort();
        v
    }

    /// 改某平台的签到设置（只动这一个平台，其余保持原样）。
    fn set_platform(e: &mut Engine, vendor: Vendor, hour: u8, minute: u8, spread: u8) {
        let mut s = e.settings().clone();
        for p in s.platform_schedules.iter_mut() {
            if p.vendor == vendor {
                p.hour = hour;
                p.minute = minute;
                p.spread_minutes = spread;
            }
        }
        e.update_settings(s).unwrap();
    }

    #[test]
    fn reconcile_schedules_spreads_accounts_within_window() {
        let dir = tmpdir("reconcile-spread");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let mut ids = Vec::new();
        for (i, label) in ["一号", "二号", "三号"].iter().enumerate() {
            let a = e
                .add_account(
                    Vendor::WorkBuddy,
                    *label,
                    Credential::from_refresh_token(format!("tok-wb-spread-{i}-0000")),
                )
                .unwrap();
            ids.push(a.id);
        }
        // 3 个账号、5 分钟窗口：窗口切成 [0,2] [2,3] [3,5]，
        // 起点是 00:10 / 00:12 / 00:13，各自再带 2/1/2 分钟的每日浮动。
        assert_eq!(sorted_hhmm(&e, &ids), vec!["00:10", "00:12", "00:13"]);
        let mut widths: Vec<u8> = ids
            .iter()
            .map(|id| {
                e.accounts()
                    .iter()
                    .find(|a| &a.id == id)
                    .unwrap()
                    .schedule
                    .jitter_minutes
            })
            .collect();
        widths.sort();
        assert_eq!(widths, vec![0, 1, 2], "左闭右开分段：前两段让出右端点，末段保留");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_second_imports_keep_import_order() {
        let dir = tmpdir("reconcile-order");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let mut ids = Vec::new();
        // 连续导入 4 个账号：`created_at` 只有秒级精度，大概率全落在同一秒。
        // 时间必须按导入顺序递增，不能因为 uuid 随机就谁先谁后。
        for (i, label) in ["A", "B", "C", "D"].iter().enumerate() {
            let a = e
                .add_account(
                    Vendor::WorkBuddy,
                    *label,
                    Credential::from_refresh_token(format!("tok-wb-order-{i}-0000")),
                )
                .unwrap();
            ids.push(a.id);
        }
        let seq: Vec<String> = ids.iter().map(|id| hhmm_of(&e, id)).collect();
        assert_eq!(
            seq,
            vec!["00:10", "00:11", "00:13", "00:14"],
            "4 个账号按导入顺序切窗口，越早导入的越靠前"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn staggered_accounts_never_share_a_minute() {
        let dir = tmpdir("reconcile-collision");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let mut ids = Vec::new();
        for (i, label) in ["A", "B", "C", "D"].iter().enumerate() {
            let a = e
                .add_account(
                    Vendor::WorkBuddy,
                    *label,
                    Credential::from_refresh_token(format!("tok-wb-collide-{i}-0000")),
                )
                .unwrap();
            ids.push(a.id);
        }
        // 4 个账号、5 分钟窗口：落点段 [0,0] [1,2] [3,3] [4,5] 互不重叠。
        // 这条是行为级断言：曾因段端点双方都能取到 + FNV 低位相关，
        // 4 个账号有 83.6% 的天数会撞在同一分钟（同一秒打上游）。
        for d in 1..=30u32 {
            let date = chrono::NaiveDate::from_ymd_opt(2026, 12, d).unwrap();
            let mut slots: Vec<_> = ids
                .iter()
                .map(|id| {
                    let a = e.accounts().iter().find(|a| &a.id == id).unwrap();
                    a.schedule.slot_on(date, &a.id)
                })
                .collect();
            slots.sort();
            assert!(
                slots.windows(2).all(|w| w[0] != w[1]),
                "第 {d} 天有账号撞在同一分钟：{slots:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reconcile_schedules_follows_spread_and_base_time() {
        let dir = tmpdir("reconcile-follow");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let mut ids = Vec::new();
        for (i, label) in ["甲", "乙", "丙"].iter().enumerate() {
            let a = e
                .add_account(
                    Vendor::WorkBuddy,
                    *label,
                    Credential::from_refresh_token(format!("tok-wb-follow-{i}-0000")),
                )
                .unwrap();
            ids.push(a.id);
        }

        // 窗口改 0：三个账号都退回基准时刻，不再错开。
        set_platform(&mut e, Vendor::WorkBuddy, 0, 10, 0);
        assert_eq!(sorted_hhmm(&e, &ids), vec!["00:10", "00:10", "00:10"]);

        // 基准改 09:30、窗口回到 5 分钟：09:30 / 09:33 / 09:35。
        set_platform(&mut e, Vendor::WorkBuddy, 9, 30, 5);
        assert_eq!(sorted_hhmm(&e, &ids), vec!["09:30", "09:32", "09:33"]);

        // 再加第 4 个账号：总数变了，前三个也要重排到 round(i*5/3)。
        let fourth = e
            .add_account(
                Vendor::WorkBuddy,
                "丁",
                Credential::from_refresh_token("tok-wb-follow-3-0000"),
            )
            .unwrap();
        ids.push(fourth.id);
        // 4 个账号重新切窗口：0 / 1 / 3 / 4。
        assert_eq!(
            sorted_hhmm(&e, &ids),
            vec!["09:30", "09:31", "09:33", "09:34"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reconcile_schedules_after_removal() {
        let dir = tmpdir("reconcile-remove");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let mut ids = Vec::new();
        for (i, label) in ["一", "二", "三"].iter().enumerate() {
            let a = e
                .add_account(
                    Vendor::WorkBuddy,
                    *label,
                    Credential::from_refresh_token(format!("tok-wb-remove-{i}-0000")),
                )
                .unwrap();
            ids.push(a.id);
        }
        assert_eq!(sorted_hhmm(&e, &ids), vec!["00:10", "00:12", "00:13"]);

        // 删掉中间那个：剩下两个要从「00:10 / 00:12」重排成铺满窗口的「00:10 / 00:13」，
        // 否则界面预览会写着「2 个账号 · 00:10 / 00:13」，与错峰规则自相矛盾。
        e.remove_account(&ids[1]).unwrap();
        let rest = vec![ids[0].clone(), ids[2].clone()];
        assert_eq!(sorted_hhmm(&e, &rest), vec!["00:10", "00:13"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reconcile_schedules_keeps_other_platforms_alone() {
        let dir = tmpdir("reconcile-iso");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let wb = e
            .add_account(
                Vendor::WorkBuddy,
                "wb",
                Credential::from_refresh_token("tok-wb-iso-000000"),
            )
            .unwrap();
        let q = e
            .add_account(
                Vendor::QoderCn,
                "q",
                Credential::from_refresh_token("tok-q-iso-0000000"),
            )
            .unwrap();
        assert_eq!(hhmm_of(&e, &q.id), "10:10", "Qoder 默认 10:10");

        set_platform(&mut e, Vendor::WorkBuddy, 3, 7, 5);
        assert_eq!(hhmm_of(&e, &wb.id), "03:07");
        assert_eq!(hhmm_of(&e, &q.id), "10:10", "改 WorkBuddy 不该动 Qoder");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_migrates_legacy_jitter_to_spread() {
        let dir = tmpdir("reconcile-migrate");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let acc = e
            .add_account(
                Vendor::TraeCn,
                "旧号",
                Credential::from_refresh_token("tok-trae-legacy-0000"),
            )
            .unwrap();
        // 模拟老数据：账号上还留着按日哈希的抖动。
        let mut accs = e.accounts().to_vec();
        accs[0].schedule.jitter_minutes = 4;
        e.store.save_accounts(&accs).unwrap();
        drop(e);

        let e2 = Engine::open(Some(dir.clone())).unwrap();
        let a = e2.accounts().iter().find(|a| a.id == acc.id).unwrap();
        // 单账号：整段 5 分钟窗口都归它，每天在 00:10~00:15 之间取一个时刻。
        assert_eq!(a.schedule.jitter_minutes, 5, "启动时应收敛到平台窗口");
        assert_eq!(
            (a.schedule.hour, a.schedule.minute),
            Vendor::TraeCn.default_checkin_time(),
            "单账号保持在平台基准时刻"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn infer_source_from_default_labels() {
        let mk = |vendor: Vendor, label: &str| Account::new(vendor, label, "h");
        assert_eq!(
            infer_source(&mk(Vendor::WorkBuddy, "WorkBuddy 本机")).as_deref(),
            Some("local")
        );
        // 多账号时默认名会带序号
        assert_eq!(
            infer_source(&mk(Vendor::WorkBuddy, "WorkBuddy 本机 2")).as_deref(),
            Some("local")
        );
        assert_eq!(
            infer_source(&mk(Vendor::TraeCn, "Trae CN 网页")).as_deref(),
            Some("browser")
        );
        assert_eq!(
            infer_source(&mk(Vendor::QoderCn, "Qoder CN 账号")).as_deref(),
            Some("token")
        );
        // 用户改过名、或平台前缀对不上：不猜
        assert_eq!(infer_source(&mk(Vendor::WorkBuddy, "我的主号")), None);
        assert_eq!(infer_source(&mk(Vendor::WorkBuddy, "Trae CN 本机")), None);
    }

    #[test]
    fn nickname_fills_label_until_user_renames() {
        let dir = tmpdir("nick");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let acc = e
            .add_account(
                Vendor::WorkBuddy,
                "WorkBuddy 本机",
                Credential::from_refresh_token("tok-nick-123456"),
            )
            .unwrap();

        // 首次导入拿到昵称：默认名换成昵称，来源记下来。
        e.apply_identity_meta(&acc.id, Some("毅One".into()), "local")
            .unwrap();
        let a = e.accounts().iter().find(|a| a.id == acc.id).unwrap();
        assert_eq!(a.label, "毅One");
        assert_eq!(a.nickname.as_deref(), Some("毅One"));
        assert_eq!(a.source.as_deref(), Some("local"));
        assert!(!a.label_custom);

        // 拿不到昵称时不把名字改回默认值。
        e.apply_identity_meta(&acc.id, None, "local").unwrap();
        let a = e.accounts().iter().find(|a| a.id == acc.id).unwrap();
        assert_eq!(a.label, "毅One", "没昵称时不该动名字");

        // 用户改名后，重导即使拿到昵称也不覆盖。
        e.rename_account(&acc.id, "我的主号").unwrap();
        e.apply_identity_meta(&acc.id, Some("毅One".into()), "browser")
            .unwrap();
        let a = e.accounts().iter().find(|a| a.id == acc.id).unwrap();
        assert_eq!(a.label, "我的主号");
        assert_eq!(a.nickname.as_deref(), Some("毅One"));
        assert_eq!(a.source.as_deref(), Some("browser"));
        assert!(a.label_custom);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_account_rejects_empty_and_too_long() {
        let dir = tmpdir("nick-bad");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let acc = e
            .add_account(
                Vendor::TraeCn,
                "Trae 本机",
                Credential::from_refresh_token("tok-nick-654321"),
            )
            .unwrap();
        assert!(e.rename_account(&acc.id, "   ").is_err(), "空名应被拒绝");
        assert!(
            e.rename_account(&acc.id, &"名".repeat(25)).is_err(),
            "超长名应被拒绝"
        );
        assert!(e.rename_account(&acc.id, "主号").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_is_idempotent() {
        let (da, db) = (tmpdir("merge-a"), tmpdir("merge-b"));
        let mut a = Engine::open(Some(da.clone())).unwrap();
        let mut b = Engine::open(Some(db.clone())).unwrap();
        a.add_account(
            Vendor::TraeCn,
            "t",
            Credential::from_refresh_token("tok-aaaaaaaaaaaa"),
        )
        .unwrap();

        let p = a.sync_export(0);
        let r1 = b.sync_merge(&p).unwrap();
        assert_eq!(r1.accounts_added, 1);
        let r2 = b.sync_merge(&p).unwrap();
        assert!(r2.is_noop());
        assert_eq!(b.accounts().len(), 1);
        // 对端账号的执行端仍是 A，本机 B 不该抢着签。
        assert!(!b.is_local_executor(&b.accounts()[0]));
        let _ = std::fs::remove_dir_all(&da);
        let _ = std::fs::remove_dir_all(&db);
    }

    #[test]
    fn tombstone_propagates_deletion() {
        let (da, db) = (tmpdir("tomb-a"), tmpdir("tomb-b"));
        let mut a = Engine::open(Some(da.clone())).unwrap();
        let mut b = Engine::open(Some(db.clone())).unwrap();
        let acc = a
            .add_account(
                Vendor::QoderCn,
                "q",
                Credential::from_refresh_token("tok-bbbbbbbbbbbb"),
            )
            .unwrap();
        b.sync_merge(&a.sync_export(0)).unwrap();
        assert_eq!(b.accounts().len(), 1);

        a.remove_account(&acc.id).unwrap();
        let r = b.sync_merge(&a.sync_export(0)).unwrap();
        assert_eq!(r.accounts_removed, 1);
        assert!(b.accounts().is_empty());

        // 对端删掉之后，旧载荷不能把它复活。
        let again = b.sync_merge(&a.sync_export(0)).unwrap();
        assert!(b.accounts().is_empty());
        assert!(again.is_noop());
        let _ = std::fs::remove_dir_all(&da);
        let _ = std::fs::remove_dir_all(&db);
    }

    #[test]
    fn platform_uid_merges_and_remaps_history() {
        let dir = tmpdir("uid");
        let mut b = Engine::open(Some(dir.clone())).unwrap();

        let phone = DeviceInfo::new("Pixel", "android");
        let mut first = Account::new(Vendor::WorkBuddy, "旧记录", "hint");
        first.id = "remote-1".into();
        first.platform_uid = Some("uid-42".into());
        first.executor = Some(phone.id.clone());
        first.origin_device = Some(phone.id.clone());
        first.updated_at = 1_000;

        let mut p1 = SyncPayload::new(&phone, 0);
        p1.accounts = vec![first];
        assert_eq!(b.sync_merge(&p1).unwrap().accounts_added, 1);

        // 同一平台 id、不同本地 id 的第二次同步：应认成同一个账号，并保留本机 id。
        let mut second = Account::new(Vendor::WorkBuddy, "新名字", "hint");
        second.id = "remote-2".into();
        second.platform_uid = Some("uid-42".into());
        second.executor = Some(phone.id.clone());
        second.origin_device = Some(phone.id.clone());
        second.updated_at = 2_000;
        let mut snap = VendorSnapshot::empty(&second, 2_001);
        snap.balance = Some(88.5);

        let mut p2 = SyncPayload::new(&phone, 0);
        p2.accounts = vec![second];
        p2.snapshots = vec![snap];
        let r = b.sync_merge(&p2).unwrap();

        assert_eq!(b.accounts().len(), 1, "同一平台 id 不该出现两个账号");
        assert_eq!(b.accounts()[0].id, "remote-1");
        assert_eq!(b.accounts()[0].label, "新名字");
        assert_eq!(r.accounts_updated, 1);
        // 对端快照按 id_map 改写到本机 id 上，否则总览统计不到。
        assert_eq!(
            b.latest_snapshots().get("remote-1").and_then(|s| s.balance),
            Some(88.5)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn adopt_identity_merges_legacy_trae_record() {
        fn fake_trae_jwt(uid: &str) -> String {
            use base64::Engine as _;
            let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
            let payload = serde_json::json!({ "data": { "id": uid, "type": "user" } });
            format!("h.{}.s", enc(&serde_json::to_vec(&payload).unwrap()))
        }

        let dir = tmpdir("adopt-trae");
        let mut e = Engine::open(Some(dir.clone())).unwrap();

        // 旧版本本机导入：账号没有 platform_uid，但 access token 是带 data.id 的 JWT。
        let token = fake_trae_jwt("123456789012345");
        let (old, _, _) = e
            .upsert_account(Vendor::TraeCn, "t 本机", Credential::from_access_token(&token))
            .unwrap();
        assert!(old.platform_uid.is_none());

        // 网页授权建的账号：有 user_id。
        let mut web = Credential::from_access_token("web-token");
        web.platform_uid = Some("123456789012345".into());
        e.upsert_account(Vendor::TraeCn, "t 网页", web).unwrap();
        assert_eq!(e.accounts().len(), 2);

        // 模拟浏览器再导入一次：先补旧记录身份，再 upsert 应把两条并成一条。
        e.adopt_identities(Vendor::TraeCn, "123456789012345")
            .await
            .unwrap();
        assert_eq!(
            e.accounts()
                .iter()
                .find(|a| a.id == old.id)
                .unwrap()
                .platform_uid
                .as_deref(),
            Some("123456789012345")
        );

        let mut again = Credential::from_access_token("web-token-2");
        again.platform_uid = Some("123456789012345".into());
        let (_, updated, merged) = e.upsert_account(Vendor::TraeCn, "t 网页", again).unwrap();
        assert!(updated);
        assert_eq!(merged, 1);
        assert_eq!(e.accounts().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_duplicate_without_uid_is_merged_on_import() {
        let dir = tmpdir("merge-legacy-dup");
        let mut e = Engine::open(Some(dir.clone())).unwrap();

        // 旧版本：本机导入，没有 platform_uid。
        let local = Credential::from_access_token("local-token");
        let (a, _, _) = e.upsert_account(Vendor::QoderCn, "q 本机", local).unwrap();
        assert!(a.platform_uid.is_none());

        // 浏览器授权：有 user_id。
        let mut browser = Credential::from_access_token("browser-token");
        browser.platform_uid = Some("user-1".into());
        let (b, updated, merged) = e
            .upsert_account(Vendor::QoderCn, "q 网页", browser)
            .unwrap();
        assert!(!updated);
        assert_eq!(merged, 0);
        assert_eq!(e.accounts().len(), 2);

        // 再本机导入同一账号（令牌和旧记录相同，且这次解析出了 user_id）：
        // 两条旧记录都要并成一条。
        let mut local_again = Credential::from_access_token("local-token");
        local_again.platform_uid = Some("user-1".into());
        let (keeper, updated, merged) = e
            .upsert_account(Vendor::QoderCn, "q 网页", local_again)
            .unwrap();
        assert!(updated);
        assert_eq!(merged, 1, "同令牌的旧记录应被合并");
        assert_eq!(e.accounts().len(), 1);
        assert_eq!(e.accounts()[0].id, b.id);
        assert_eq!(keeper.id, b.id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refresh_only_when_near_expiry() {
        let mut cred = Credential::from_access_token("access");
        assert!(!refresh_due(&cred), "没有 refresh token 不该续期");

        cred.refresh_token = Some("rt".into());
        assert!(refresh_due(&cred), "没有过期时间按老行为续期");

        let now = chrono::Local::now().timestamp();
        cred.expires_at = Some(now + 3600);
        assert!(!refresh_due(&cred), "还有一小时有效不该续期");

        cred.expires_at = Some(now + 60);
        assert!(refresh_due(&cred), "一分钟内过期必须续期");

        cred.expires_at = Some(now - 10);
        assert!(refresh_due(&cred), "已过期必须续期");
    }

    #[tokio::test]
    async fn verified_import_rolls_back_when_verification_fails() {
        let dir = tmpdir("verify-fail");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        // Trae 缺设备 id 时会在本地就拒绝（不发网络请求）：验证必然失败，账号不能落库。
        let err = e
            .upsert_account_verified(
                Vendor::TraeCn,
                "t",
                Credential::from_access_token("whatever"),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "实际错误：{err:?}");
        assert!(e.accounts().is_empty(), "验证失败不能留下账号");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_subject_updates_instead_of_duplicating() {
        let dir = tmpdir("dedup");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let token = jwt_sub("user-42");

        let (first, updated, merged) = e
            .upsert_account(
                Vendor::WorkBuddy,
                "主号",
                Credential::from_refresh_token(&token),
            )
            .unwrap();
        assert!(!updated);
        assert_eq!(merged, 0);
        assert_eq!(first.platform_uid.as_deref(), Some("user-42"));

        // 同一用户换个导入方式 / 换个别名再来一次：更新凭据，不新建。
        let (again, updated, merged) = e
            .upsert_account(
                Vendor::WorkBuddy,
                "另一个名字",
                Credential::from_refresh_token(&token),
            )
            .unwrap();
        assert!(updated, "同一平台同一用户应识别为已存在");
        assert_eq!(merged, 0);
        assert_eq!(again.id, first.id);
        assert_eq!(again.label, "主号", "已有账号的别名不应被覆盖");
        assert_eq!(e.accounts().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backfills_identity_for_old_account() {
        let dir = tmpdir("backfill");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let token = jwt_sub("legacy-1");

        // 旧版本导入：账号没有 platform_uid。
        let old = e
            .add_account(
                Vendor::WorkBuddy,
                "老账号",
                Credential::from_refresh_token(&token),
            )
            .unwrap();
        assert!(old.platform_uid.is_none());

        let (acc, updated, merged) = e
            .upsert_account(
                Vendor::WorkBuddy,
                "重导",
                Credential::from_refresh_token(&token),
            )
            .unwrap();
        assert!(updated, "应能从旧凭据补齐身份并认成同一账号");
        assert_eq!(merged, 0);
        assert_eq!(acc.id, old.id);
        assert_eq!(e.accounts().len(), 1);
        assert_eq!(e.accounts()[0].platform_uid.as_deref(), Some("legacy-1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_opaque_token_updates_instead_of_duplicating() {
        let dir = tmpdir("opaque-dedup");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        // Trae 这类不是 JWT 的令牌：只能靠整串完全相同来认。
        let (first, updated, _) = e
            .upsert_account(
                Vendor::TraeCn,
                "Trae 主号",
                Credential::from_access_token("opaque-token-1234567890"),
            )
            .unwrap();
        assert!(!updated);

        let (again, updated, _) = e
            .upsert_account(
                Vendor::TraeCn,
                "别名不同",
                Credential::from_access_token("opaque-token-1234567890"),
            )
            .unwrap();
        assert!(updated, "整串令牌相同应认成同一账号");
        assert_eq!(again.id, first.id);
        assert_eq!(e.accounts().len(), 1);

        // 不同令牌仍是不同账号。
        e.upsert_account(
            Vendor::TraeCn,
            "Trae 小号",
            Credential::from_access_token("opaque-token-0987654321"),
        )
        .unwrap();
        assert_eq!(e.accounts().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merges_duplicates_left_by_earlier_imports() {
        let dir = tmpdir("merge-dups");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let token = jwt_sub("user-7");
        // 旧版本重复导入留下的两条同账号记录（都没有身份字段）。
        e.add_account(
            Vendor::WorkBuddy,
            "重复 A",
            Credential::from_refresh_token(&token),
        )
        .unwrap();
        e.add_account(
            Vendor::WorkBuddy,
            "重复 B",
            Credential::from_refresh_token(&token),
        )
        .unwrap();
        assert_eq!(e.accounts().len(), 2);

        let (acc, updated, merged) = e
            .upsert_account(
                Vendor::WorkBuddy,
                "再导一次",
                Credential::from_refresh_token(&token),
            )
            .unwrap();
        assert!(updated);
        assert_eq!(merged, 1, "应合并掉多余的一条");
        assert_eq!(e.accounts().len(), 1);
        assert_eq!(e.accounts()[0].id, acc.id);
        assert_eq!(e.accounts()[0].platform_uid.as_deref(), Some("user-7"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_to_take_over_remote_executor_account() {
        let dir = tmpdir("remote-takeover");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let phone = DeviceInfo::new("Pixel", "android");
        let mut remote = Account::new(Vendor::WorkBuddy, "远端账号", "hint");
        remote.id = "remote-9".into();
        remote.platform_uid = Some("user-9".into());
        remote.executor = Some(phone.id.clone());
        remote.origin_device = Some(phone.id.clone());
        let mut payload = SyncPayload::new(&phone, 0);
        payload.accounts = vec![remote];
        e.sync_merge(&payload).unwrap();

        let err = e
            .upsert_account(
                Vendor::WorkBuddy,
                "本机导入",
                Credential::from_refresh_token(&jwt_sub("user-9")),
            )
            .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
        assert_eq!(e.accounts().len(), 1, "不能给远端执行的账号在本机再存一份凭据");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 直接改写落盘的 accounts.json，模拟「设备 id 变过」的历史数据。
    fn rewrite_accounts(dir: &Path, f: impl Fn(&mut Vec<Account>)) {
        let path = dir.join("accounts.json");
        let raw = std::fs::read_to_string(&path).unwrap();
        let mut accounts: Vec<Account> = serde_json::from_str(&raw).unwrap();
        f(&mut accounts);
        std::fs::write(&path, serde_json::to_string_pretty(&accounts).unwrap()).unwrap();
    }

    /// 今天 23:59，保证所有默认排期都已到点。
    fn late_today() -> chrono::NaiveDateTime {
        chrono::Local::now()
            .date_naive()
            .and_hms_opt(23, 59, 0)
            .unwrap()
    }

    #[test]
    fn orphan_executor_is_reclaimed_on_open() {
        let dir = tmpdir("exec-reclaim");
        {
            let mut e = Engine::open(Some(dir.clone())).unwrap();
            e.add_account(
                Vendor::WorkBuddy,
                "主号",
                Credential::from_refresh_token("tok-reclaim-123456"),
            )
            .unwrap();
        }
        // device.json 重新生成后，账号仍记着旧设备 id。
        rewrite_accounts(&dir, |list| {
            list[0].executor = Some("ghost-device-id".into());
        });
        let e = Engine::open(Some(dir.clone())).unwrap();
        assert_eq!(
            e.accounts()[0].executor.as_deref(),
            Some(e.local_device_id()),
            "执行端设备不存在时应收回本机，否则永远签不了到"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paired_device_executor_is_kept() {
        let dir = tmpdir("exec-paired");
        let phone_id = {
            let mut e = Engine::open(Some(dir.clone())).unwrap();
            e.add_account(
                Vendor::WorkBuddy,
                "主号",
                Credential::from_refresh_token("tok-paired-123456"),
            )
            .unwrap();
            e.pair_device("Pixel", "android", "hash-1".into())
                .unwrap()
                .id
        };
        rewrite_accounts(&dir, |list| {
            list[0].executor = Some(phone_id.clone());
        });
        let e = Engine::open(Some(dir.clone())).unwrap();
        assert_eq!(
            e.accounts()[0].executor.as_deref(),
            Some(phone_id.as_str()),
            "已配对设备的执行端不能被误收回"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn due_accounts_skip_remote_executor() {
        let dir = tmpdir("due-remote");
        let phone_id = {
            let mut e = Engine::open(Some(dir.clone())).unwrap();
            e.add_account(
                Vendor::WorkBuddy,
                "主号",
                Credential::from_refresh_token("tok-due-remote-1234"),
            )
            .unwrap();
            e.pair_device("Pixel", "android", "h".into()).unwrap().id
        };
        rewrite_accounts(&dir, |list| {
            list[0].executor = Some(phone_id.clone());
        });
        let e = Engine::open(Some(dir.clone())).unwrap();
        assert!(
            e.due_accounts(late_today()).is_empty(),
            "非本机执行的账号不该被调度，否则每轮都失败"
        );
        assert!(e.checkin_alerts(late_today()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retry_after_blocks_auto_retry_until_due() {
        let dir = tmpdir("retry-window");
        {
            let mut e = Engine::open(Some(dir.clone())).unwrap();
            e.add_account(
                Vendor::WorkBuddy,
                "主号",
                Credential::from_refresh_token("tok-retry-123456"),
            )
            .unwrap();
        }
        let now = late_today();
        let now_ts = local_epoch(now);
        rewrite_accounts(&dir, |list| {
            list[0].last_checkin_at = Some(now_ts - 86_400);
            list[0].retry_after = Some(now_ts + 600);
        });
        let e = Engine::open(Some(dir.clone())).unwrap();
        assert!(e.due_accounts(now).is_empty(), "退避窗口内不该自动重试");

        rewrite_accounts(&dir, |list| list[0].retry_after = Some(now_ts - 1));
        let e = Engine::open(Some(dir.clone())).unwrap();
        assert_eq!(e.due_accounts(now).len(), 1, "退避到期后应恢复调度");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retry_delay_is_shorter_for_plain_failures() {
        let at = 1_795_759_157;
        assert_eq!(retry_delay_secs(CheckinStatus::Failed, at), 30 * 60);
        let auth = retry_delay_secs(CheckinStatus::AuthFailed, at);
        assert!(
            (60..=24 * 3600).contains(&auth),
            "凭据失效应至少等到次日，实际 {auth}"
        );
        assert!(auth > 30 * 60);
    }

    #[test]
    fn failure_notice_is_daily_throttled() {
        let dir = tmpdir("notice-throttle");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        let acc = e
            .add_account(
                Vendor::WorkBuddy,
                "主号",
                Credential::from_refresh_token("tok-notice-123456"),
            )
            .unwrap();
        assert!(e.mark_failure_notice(&acc.id), "当天第一次失败应发通知");
        assert!(!e.mark_failure_notice(&acc.id), "同一天第二次不再发");
        // 模拟成昨天的标记：跨天后允许再次通知。
        rewrite_accounts(&dir, |list| list[0].last_failure_notice_at = Some(0));
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        assert!(e.mark_failure_notice(&acc.id), "跨天后可以再次通知");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkin_alerts_lists_today_unfinished_failures() {
        let dir = tmpdir("alerts");
        {
            let mut e = Engine::open(Some(dir.clone())).unwrap();
            e.add_account(
                Vendor::WorkBuddy,
                "主号",
                Credential::from_refresh_token("tok-alert-123456"),
            )
            .unwrap();
        }
        let now = late_today();
        let now_ts = local_epoch(now);
        rewrite_accounts(&dir, |list| {
            list[0].last_checkin_at = Some(now_ts - 86_400);
            list[0].last_error = Some("网络不通".into());
        });
        let e = Engine::open(Some(dir.clone())).unwrap();
        let alerts = e.checkin_alerts(now);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].label, "主号");
        assert!(alerts[0].message.contains("网络不通"));

        // 今天签成功后不再提示。
        rewrite_accounts(&dir, |list| list[0].last_checkin_at = Some(now_ts));
        let e = Engine::open(Some(dir.clone())).unwrap();
        assert!(e.checkin_alerts(now).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
