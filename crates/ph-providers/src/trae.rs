//! Trae CN（字节国内版）适配器。
//!
//! ## 接口事实（2026-11-26 用真实账号实测确认）
//!
//! - 鉴权头是 `Authorization: Cloud-IDE-JWT <token>`（**不是**自定义头）；
//! - 必须带真机 `X-Device-Id`，设备 id 不一致会返回业务码 9074；
//! - 必须带 `X-User-Region: CN`，否则可能被路由到国际站；
//! - 签到状态 / 领取都是 `POST`：
//!   `/trae/api/v2/ug/checkin_credits/status`、`/trae/api/v2/ug/checkin_credits/claim`；
//! - 余额取 `POST /trae/api/v2/pay/ide_user_ent_usage` 的 `total_balance`。
//!
//! 令牌来自本机客户端登录态（`iCubeAuthInfo://icube.cloudide`），有效期约一周；
//! 续期需要对设备私钥签名（v3 ExchangeToken），本版不做，过期后重新导入即可。

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
const BALANCE_PATH: &str = "/trae/api/v2/pay/ide_user_ent_usage";

pub struct TraeCnProvider {
    client: reqwest::Client,
}

impl TraeCnProvider {
    pub fn new() -> Self {
        Self {
            client: http::build_client(None).expect("默认客户端一定可构建"),
        }
    }

    fn base(cred: &Credential) -> String {
        cred.domain
            .as_deref()
            .map(str::trim)
            .filter(|s| s.starts_with("http"))
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_else(|| TRAE_BASE.to_string())
    }

    fn headers(rb: reqwest::RequestBuilder, cred: &Credential) -> reqwest::RequestBuilder {
        let mut rb = rb
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-User-Region", "CN");
        if let Some(jwt) = cred.access_token.as_deref().filter(|s| !s.is_empty()) {
            rb = rb.header("Authorization", format!("Cloud-IDE-JWT {jwt}"));
        }
        if let Some(dev) = cred.device_id.as_deref().filter(|s| !s.is_empty()) {
            rb = rb.header("X-Device-Id", dev);
        }
        if let Some(cookie) = cred.cookie.as_deref().filter(|s| !s.is_empty()) {
            rb = rb.header("Cookie", cookie);
        }
        rb
    }

    fn check_prereq(cred: &Credential) -> Result<()> {
        if cred.access_token.as_deref().unwrap_or("").trim().is_empty() {
            return Err(Error::Auth(
                "Trae CN 需要 Cloud-IDE-JWT 令牌，请先在本机导入登录态".into(),
            ));
        }
        if cred.device_id.as_deref().unwrap_or("").trim().is_empty() {
            return Err(Error::Auth(
                "Trae CN 需要原始设备 id（X-Device-Id），缺失会被判 9074。请重新导入。".into(),
            ));
        }
        Ok(())
    }

    async fn post_json(&self, cred: &Credential, path: &str) -> Result<(u16, Value)> {
        let url = format!("{}{}", Self::base(cred), path);
        let resp = Self::headers(self.client.post(&url), cred)
            .json(&json!({}))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        Ok((status, body))
    }

    fn surface(status: u16, body: &Value) -> Option<Error> {
        if http::code_of(body) == Some(9074) {
            return Some(Error::Auth(
                "Trae 判定设备指纹不一致（9074），请用该账号原设备导出的 device id".into(),
            ));
        }
        http::map_status(status, body)
    }

    /// 余额是失败可忽略的附加信息：拿不到就留空，不影响签到。
    async fn balance(&self, cred: &Credential) -> Result<Option<f64>> {
        let (status, body) = self.post_json(cred, BALANCE_PATH).await?;
        if let Some(e) = Self::surface(status, &body) {
            return Err(e);
        }
        let data = body.get("data").unwrap_or(&body);
        Ok(http::pick_f64(data, &["total_balance", "total_balance_credits", "balance"])
            .or_else(|| http::pick_f64(&body, &["total_balance", "balance"])))
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
            verified: true,
        }
    }

    async fn refresh(&self, _cred: &mut Credential) -> Result<()> {
        Err(Error::Unsupported(
            "Trae CN 的令牌续期需要设备私钥签名（v3 ExchangeToken），本版未实现；过期后重新「本机导入」".into(),
        ))
    }

    async fn snapshot(&self, account: &Account, cred: &Credential) -> Result<VendorSnapshot> {
        Self::check_prereq(cred)?;
        let (status, body) = self.post_json(cred, STATUS_PATH).await?;
        if let Some(e) = Self::surface(status, &body) {
            return Err(e);
        }
        let code = http::code_of(&body);
        if let Some(code) = code.filter(|c| *c != 0) {
            return Err(Error::Upstream(format!(
                "Trae 签到状态查询失败（code={code}）：{}",
                http::msg_of(&body)
            )));
        }

        let data = body.get("data").unwrap_or(&body);
        let mut snap = VendorSnapshot::empty(account, chrono::Local::now().timestamp());
        snap.checked_in_today = http::pick_bool(data, &["checked_in", "today_checked_in"])
            .or_else(|| http::pick_bool(&body, &["checked_in", "today_checked_in"]));
        snap.daily_credit = http::pick_i64(
            data,
            &["credits", "daily_credit", "reward_credit", "extra_credits"],
        )
        .or_else(|| http::pick_i64(&body, &["credits", "daily_credit", "reward_credit"]));
        snap.streak_days = http::pick_i64(data, &["streak_days", "continuous_days"]);

        let mut notes: Vec<String> = Vec::new();
        if let Some(false) = body.get("enable").and_then(Value::as_bool) {
            notes.push("签到活动当前未开启".into());
        }
        match self.balance(cred).await {
            Ok(Some(b)) => snap.balance = Some(b),
            Ok(None) => notes.push("余额接口没有返回可识别的字段".into()),
            Err(Error::Auth(m)) => return Err(Error::Auth(m)),
            Err(e) => notes.push(format!("余额查询失败：{}", e.user_message())),
        }
        if !notes.is_empty() {
            snap.note = Some(notes.join("；"));
        }
        Ok(snap)
    }

    async fn checkin(&self, account: &Account, cred: &Credential) -> Result<CheckinOutcome> {
        Self::check_prereq(cred)?;
        let now = chrono::Local::now().timestamp();

        // 先查状态：Trae 的领取接口对「今天已领」不会报错，直接再点一次
        // 仍会返回成功，不预检就会天天显示「领取成功」。
        let (pre_status, pre_body) = self.post_json(cred, STATUS_PATH).await?;
        if let Some(e) = Self::surface(pre_status, &pre_body) {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::TraeCn,
                status: CheckinStatus::AuthFailed,
                credit: None,
                streak_days: None,
                message: e.user_message(),
                at: now,
            });
        }
        if let Some(false) = pre_body.get("enable").and_then(Value::as_bool) {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::TraeCn,
                status: CheckinStatus::NotEligible,
                credit: None,
                streak_days: None,
                message: "Trae 签到活动当前未开启".into(),
                at: now,
            });
        }
        if let Some(code) = http::code_of(&pre_body).filter(|c| *c != 0) {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::TraeCn,
                status: CheckinStatus::Failed,
                credit: None,
                streak_days: None,
                message: format!(
                    "签到状态查询失败（code={code}）：{}",
                    http::msg_of(&pre_body)
                ),
                at: now,
            });
        }
        let pre_data = pre_body.get("data").unwrap_or(&pre_body);
        let already = http::pick_bool(pre_data, &["checked_in", "today_checked_in"])
            .or_else(|| http::pick_bool(&pre_body, &["checked_in", "today_checked_in"]))
            .unwrap_or(false);
        if already {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::TraeCn,
                status: CheckinStatus::AlreadyDone,
                credit: None,
                streak_days: None,
                message: "今天已签到，无需重复操作".into(),
                at: now,
            });
        }

        let (status, body) = self.post_json(cred, CLAIM_PATH).await?;
        if let Some(e) = Self::surface(status, &body) {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::TraeCn,
                status: CheckinStatus::AuthFailed,
                credit: None,
                streak_days: None,
                message: e.user_message(),
                at: now,
            });
        }

        let code = http::code_of(&body);
        let msg = http::msg_of(&body);
        let lowered = msg.to_lowercase();
        let data = body.get("data").unwrap_or(&body);
        let credits = http::pick_i64(data, &["credits", "daily_credit", "reward_credit"])
            .or_else(|| http::pick_i64(&body, &["credits", "daily_credit", "reward_credit"]));
        let extra = http::pick_i64(data, &["extra_credits"])
            .or_else(|| http::pick_i64(&body, &["extra_credits"]))
            .unwrap_or(0);

        if msg.contains('已') || lowered.contains("already") {
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

        if code.map_or(true, |c| c == 0) && status < 400 {
            let total = credits.unwrap_or(0) + extra;
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::TraeCn,
                status: CheckinStatus::Claimed,
                credit: (total > 0).then_some(total),
                streak_days: None,
                message: if total > 0 {
                    format!("领取成功，+{total} 积分")
                } else {
                    "领取成功".into()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prereq_requires_token_and_device() {
        assert!(matches!(
            TraeCnProvider::check_prereq(&Credential::from_access_token("jwt")),
            Err(Error::Auth(_))
        ));
        let mut cred = Credential::from_access_token("jwt");
        cred.device_id = Some("dev-1".into());
        assert!(TraeCnProvider::check_prereq(&cred).is_ok());
    }

    #[test]
    fn device_mismatch_maps_to_auth() {
        let err = TraeCnProvider::surface(200, &json!({ "code": 9074, "message": "device" }))
            .expect("9074 必须判为凭据问题");
        assert!(matches!(err, Error::Auth(_)));
        assert!(TraeCnProvider::surface(200, &json!({ "code": 0 })).is_none());
    }
}
