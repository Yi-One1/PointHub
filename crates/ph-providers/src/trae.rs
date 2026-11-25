//! Trae CN（字节国内版）适配器。
//!
//! 关键约束（来自实测反馈）：
//! - 必须带 `Cloud-IDE-JWT`，且**必须**带真机 `x-device-id`，
//!   设备 id 不一致会返回业务码 9074；
//! - 必须带 `X-User-Region: CN`，否则会被路由到国际站；
//! - 令牌刷新走 v3 `ExchangeToken`（需要设备私钥签名），
//!   本版**不做自动刷新**：过期后提示用户重新导入。
//!
//! 状态：请求/响应形状已按公开资料实现，但**尚未用真实账号实机验证**，
//! 因此 `capabilities().verified = false`，界面会显示「待验证」标记。

use crate::http;
use crate::{Capabilities, Provider};
use async_trait::async_trait;
use ph_core::{
    Account, CheckinOutcome, CheckinStatus, Credential, Error, Result, Vendor, VendorSnapshot,
};
use serde_json::{json, Value};

const TRAE_BASE: &str = "https://api.trae.cn";
const STATUS_PATH: &str = "/trae/api/v2/ug/checkin_credits/status";
const CLAIM_PATH: &str = "/trae/api/v2/ug/checkin_credits/claim";

pub struct TraeCnProvider {
    client: reqwest::Client,
}

impl TraeCnProvider {
    pub fn new() -> Self {
        Self {
            client: http::build_client(None).expect("默认客户端一定可构建"),
        }
    }

    fn headers<'a>(
        rb: reqwest::RequestBuilder,
        cred: &'a Credential,
    ) -> reqwest::RequestBuilder {
        let mut rb = rb.header("X-User-Region", "CN");
        if let Some(jwt) = cred.access_token.as_deref().filter(|s| !s.is_empty()) {
            rb = rb.header("Cloud-IDE-JWT", jwt);
        }
        if let Some(dev) = cred.device_id.as_deref().filter(|s| !s.is_empty()) {
            rb = rb.header("x-device-id", dev);
        }
        rb
    }

    fn check_prereq(cred: &Credential) -> Result<()> {
        if cred.access_token.as_deref().unwrap_or("").is_empty() {
            return Err(Error::Auth("Trae CN 需要 Cloud-IDE-JWT 令牌".into()));
        }
        if cred.device_id.as_deref().unwrap_or("").is_empty() {
            return Err(Error::Auth(
                "Trae CN 需要原始设备 id（x-device-id），缺失会被判 9074。请重新导入时一并提供。"
                    .into(),
            ));
        }
        Ok(())
    }
}

impl Default for TraeCnProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for TraeCnProvider {
    fn vendor(&self) -> Vendor {
        Vendor::TraeCn
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            checkin: true,
            travel: false,
            balance: true,
            token_refresh: false,
            verified: false,
        }
    }

    async fn refresh(&self, _cred: &mut Credential) -> Result<()> {
        Err(Error::Unsupported(
            "Trae CN 的令牌刷新需要对设备私钥签名（v3 ExchangeToken），本版未实现；请重新导入令牌".into(),
        ))
    }

    async fn snapshot(&self, account: &Account, cred: &Credential) -> Result<VendorSnapshot> {
        Self::check_prereq(cred)?;
        let url = format!("{TRAE_BASE}{STATUS_PATH}");
        let rb = self.client.get(&url);
        let resp = Self::headers(rb, cred).send().await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        let code = http::code_of(&body);
        if code == Some(9074) {
            return Err(Error::Auth(
                "Trae 判定设备指纹不一致（9074），请用该账号原设备导出的 device id".into(),
            ));
        }
        let data = body.get("data").cloned().unwrap_or_else(|| body.clone());

        let mut snap = VendorSnapshot::empty(account, chrono::Local::now().timestamp());
        snap.balance = http::pick_f64(&data, http::BALANCE_KEYS);
        snap.checked_in_today = http::pick_bool(
            &data,
            &["today_checked_in", "checked_in", "has_checked_in", "todayCheckedIn"],
        );
        snap.daily_credit = http::pick_i64(&data, &["daily_credit", "credit", "reward_credit"]);
        snap.streak_days = http::pick_i64(&data, &["streak_days", "continuous_days"]);
        if snap.balance.is_none() && snap.daily_credit.is_none() {
            snap.note = Some("Trae 未返回可识别的积分字段，可能是接口改版".into());
        }
        Ok(snap)
    }

    async fn checkin(&self, account: &Account, cred: &Credential) -> Result<CheckinOutcome> {
        Self::check_prereq(cred)?;
        let url = format!("{TRAE_BASE}{CLAIM_PATH}");
        let rb = self.client.post(&url);
        let resp = Self::headers(rb, cred).json(&json!({})).send().await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        let now = chrono::Local::now().timestamp();
        let code = http::code_of(&body);
        let msg = http::msg_of(&body);
        let data = body.get("data").cloned().unwrap_or(Value::Null);
        let credit = http::pick_i64(&data, &["credit", "reward_credit", "daily_credit"])
            .or_else(|| http::pick_i64(&body, &["credit", "reward_credit"]));

        if status == 401 || status == 403 || code == Some(9074) {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::TraeCn,
                status: CheckinStatus::AuthFailed,
                credit: None,
                streak_days: None,
                message: if msg.is_empty() {
                    "令牌或设备 id 被拒绝".into()
                } else {
                    msg
                },
                at: now,
            });
        }
        if msg.contains("已签") || msg.contains("already") {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::TraeCn,
                status: CheckinStatus::AlreadyDone,
                credit: None,
                streak_days: None,
                message: if msg.is_empty() { "今天已签到".into() } else { msg },
                at: now,
            });
        }
        if code == Some(0) && status < 400 {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::TraeCn,
                status: CheckinStatus::Claimed,
                credit,
                streak_days: None,
                message: match credit {
                    Some(c) => format!("领取成功，+{c} 积分"),
                    None => "领取成功".into(),
                },
                at: now,
            });
        }
        Ok(CheckinOutcome {
            account_id: account.id.clone(),
            vendor: Vendor::TraeCn,
            status: CheckinStatus::Failed,
            credit: None,
            streak_days: None,
            message: format!(
                "未成功（HTTP {status}, code {}）：{}",
                code.map(|c| c.to_string()).unwrap_or_else(|| "-".into()),
                if msg.is_empty() { "平台未给出原因" } else { &msg }
            ),
            at: now,
        })
    }
}
