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
    Account, CheckinOutcome, CheckinStatus, Credential, Error, Result, Settings, TravelSnapshot,
    Vendor, VendorSnapshot,
};
use ph_providers::{provider_for, TravelAction};
use ph_store::Store;
use ph_vault::Vault;
use std::path::{Path, PathBuf};

/// 汇总视图，给悬浮球与总览页用。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Aggregate {
    pub total_balance: i64,
    pub account_count: usize,
    /// 今天还没签到的账号数。
    pub pending_count: usize,
    /// 有余额数据的账号数（用于说明「总数覆盖了几个账号」）。
    pub balance_known: usize,
    /// 最近一次采样时间。
    pub last_sampled_at: Option<i64>,
}

pub struct Engine {
    store: Store,
    vault: Vault,
    settings: Settings,
    accounts: Vec<Account>,
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
        let accounts = store.load_accounts().unwrap_or_else(|e| {
            store.log(&format!("账号读取失败：{e}"));
            Vec::new()
        });
        Ok(Self {
            store,
            vault,
            settings,
            accounts,
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
        self.vault.put(&account.id, cred);
        self.vault.flush()?;
        self.accounts.push(account.clone());
        self.store.save_accounts(&self.accounts)?;
        self.store
            .log(&format!("新增账号 {} / {}", vendor.display(), account.label));
        Ok(account)
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
            .filter(|a| a.enabled)
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
    fn estimate_gain(&self, account_id: &str, now_balance: Option<i64>) -> Option<i64> {
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
        if diff >= 0 {
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
        Ok(outcome)
    }

    /// 一键全部签到。逐个串行 + 随机间隔，避免同秒打上游触发风控。
    pub async fn checkin_all(&mut self) -> Vec<CheckinOutcome> {
        let ids: Vec<String> = self
            .accounts
            .iter()
            .filter(|a| a.enabled && a.checkin_enabled)
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
        result
    }

    // ------------------------------------------------------------ 汇总

    pub fn aggregate(&self) -> Aggregate {
        let latest = self.store.latest_by_account().unwrap_or_default();
        let mut agg = Aggregate {
            account_count: self.accounts.len(),
            ..Default::default()
        };
        let mut total = 0i64;
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
