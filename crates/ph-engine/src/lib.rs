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
    Account, ActivityEvent, CheckinOutcome, CheckinStatus, Credential, DeviceInfo, Error,
    MergeReport, Result, Settings, SyncPayload, Tombstone, TravelSnapshot, Vendor, VendorSnapshot,
    SYNC_PROTOCOL,
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
fn device_kind() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else {
        "pc"
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
        let settings = store.load_settings().unwrap_or_else(|e| {
            store.log(&format!("设置读取失败，使用默认值：{e}"));
            Settings::default()
        });
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
        let mut dirty = false;
        for a in &mut accounts {
            if a.executor.is_none() {
                a.executor = Some(device.id.clone());
                dirty = true;
            }
            if a.origin_device.is_none() {
                a.origin_device = Some(device.id.clone());
                dirty = true;
            }
        }
        if dirty {
            store.save_accounts(&accounts)?;
        }
        Ok(Self {
            store,
            vault,
            settings,
            accounts,
            device,
        })
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

    pub fn update_settings(&mut self, s: Settings) -> Result<()> {
        self.store.save_settings(&s)?;
        self.settings = s;
        Ok(())
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
            let matches: Vec<Account> = self
                .accounts
                .iter()
                .filter(|a| a.vendor == vendor && a.platform_uid.as_deref() == Some(uid))
                .cloned()
                .collect();
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

    /// 带验证的导入：先用临时账号刷新 + 采样，全部通过才落库。
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
        provider.refresh(&mut cred).await?;
        let snap = provider.snapshot(&probe, &cred).await?;
        let (account, updated, merged) = self.upsert_account(vendor, label, cred)?;
        Ok((account, snap, updated, merged))
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
        // 先尝试刷新；刷新失败但还有 access_token 时继续（可能是网络抖动）。
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
                return Ok(outcome);
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
            } else {
                acc.last_error = Some(outcome.message.clone());
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
        agg
    }

    pub fn latest_snapshots(&self) -> std::collections::BTreeMap<String, VendorSnapshot> {
        self.store.latest_by_account().unwrap_or_default()
    }

    /// 近 `days` 天的每日余额合计（含今天，按日期升序）。
    ///
    /// 规则：取每天最后一次采样；只统计**当前仍纳管**的账号——
    /// 删掉的账号不该继续占着历史合计，否则曲线会莫名掉一截。
    pub fn daily_series(&self, days: i64) -> Vec<DayPoint> {
        use chrono::Datelike;
        use std::collections::BTreeMap;

        let today = chrono::Local::now().date_naive();
        let from = today - chrono::Duration::days(days.max(1) - 1);
        // date -> account_id -> 当天最后余额
        let mut per_day: BTreeMap<chrono::NaiveDate, BTreeMap<String, f64>> = BTreeMap::new();

        if let Ok(snaps) = self.store.load_snapshots(20000) {
            for s in snaps {
                let Some(dt) = ph_core::schedule::local_from_epoch(s.captured_at) else {
                    continue;
                };
                let d = dt.date();
                if d < from || d > today {
                    continue;
                }
                if let Some(b) = s.balance {
                    // 顺序遍历，后写覆盖前写，天然得到「当天最后一次」。
                    per_day.entry(d).or_default().insert(s.account_id.clone(), b);
                }
            }
        }

        let mut out = Vec::with_capacity(days.max(1) as usize);
        let mut prev: Option<(f64, usize)> = None;
        for i in (0..days.max(1)).rev() {
            let d = today - chrono::Duration::days(i);
            let mut total = 0f64;
            let mut known = 0usize;
            if let Some(m) = per_day.get(&d) {
                for a in &self.accounts {
                    if let Some(b) = m.get(&a.id) {
                        total += *b;
                        known += 1;
                    }
                }
            }
            // 只有两天都有数据才算得出「新增」，否则宁可空着。
            let gain = match prev {
                Some((pt, pk)) if known > 0 && pk > 0 => Some(total - pt),
                _ => None,
            };
            if known > 0 {
                prev = Some((total, known));
            }
            out.push(DayPoint {
                date: format!("{:02}-{:02}", d.month(), d.day()),
                total,
                known,
                gain,
            });
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
        self.accounts
            .iter()
            .filter(|a| a.enabled && a.checkin_enabled)
            .filter(|a| a.schedule.due_today(now, &a.id))
            .filter(|a| {
                // 今天已经签过的就不再补
                a.last_checkin_at
                    .and_then(ph_core::schedule::local_from_epoch)
                    .map(|d| d.date() < today)
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
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
    async fn verified_import_rolls_back_when_verification_fails() {
        let dir = tmpdir("verify-fail");
        let mut e = Engine::open(Some(dir.clone())).unwrap();
        // Qoder 的刷新未实现：验证必然失败，账号不能落库。
        let err = e
            .upsert_account_verified(
                Vendor::QoderCn,
                "q",
                Credential::from_access_token("whatever"),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
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
}
