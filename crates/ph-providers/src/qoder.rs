//! Qoder CN（阿里）适配器。
//!
//! ## 机制（2026-11-26 用真实账号实测确认）
//!
//! Qoder 的每日 100 Credits 走「活动」模型，而不是一个固定的 checkin 端点：
//!
//! 1. `GET /sash/api/v1/me/campaigns` 取活动列表，里面每条带
//!    `actionType / claimStatus / startAt / endAt / benefit.amount`；
//! 2. 只处理 `actionType == CLAIM_BENEFIT` 且当前时间落在 `[startAt, endAt)` 的活动；
//! 3. `POST /sash/api/v1/me/campaigns/{campaignId}/claim`（空对象）领取；
//! 4. 余额取 `GET /sash/api/v2/me/usage` 里各配额桶的 `remaining` 之和。
//!
//! 免费账号也能领：用户 2026-11-26 的实测账号就是新注册的免费号（D07 已修正）。
//! 因此**不按套餐猜资格**，一律以接口返回为准。
//!
//! 鉴权用 `Authorization: Bearer <access_token>`，令牌来自客户端登录态，
//! 本版不做刷新（客户端登录态有效期约 30 天），过期后重新导入即可。

use crate::http;
use crate::{Capabilities, Provider};
use async_trait::async_trait;
use chrono::TimeZone as _;
use ph_core::{
    Account, CheckinOutcome, CheckinStatus, Credential, Error, Result, Vendor, VendorSnapshot,
};
use serde_json::{json, Value};

const QODER_BASE: &str = "https://openapi.qoder.com.cn";
const CAMPAIGNS_PATH: &str = "/sash/api/v1/me/campaigns";
const USAGE_PATH: &str = "/sash/api/v2/me/usage";
/// 领取窗口是每天 10:00 刷新，到次日 10:00 结束；文案按这个口径写。
const WINDOW_HINT: &str = "每天 10:00 刷新，领取窗口到次日 10:00";

/// 单条活动的领取结果。
enum ClaimResult {
    /// 本次领到了。
    Claimed,
    /// 服务端说此前已经领过。
    Already,
    /// 服务端回了空响应，需要以最终活动状态判定。
    Unknown,
}

pub struct QoderCnProvider {
    client: reqwest::Client,
}

impl QoderCnProvider {
    pub fn new() -> Self {
        Self {
            client: http::build_client(None).expect("默认客户端一定可构建"),
        }
    }

    fn token(cred: &Credential) -> Result<&str> {
        cred.access_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::Auth("Qoder CN 需要 access_token，请先在本机导入登录态".into()))
    }

    /// 默认用官方域名；`domain` 给了完整地址时按自定义端点走（内网代理等场景）。
    fn base(cred: &Credential) -> String {
        cred.domain
            .as_deref()
            .map(str::trim)
            .filter(|s| s.starts_with("http"))
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_else(|| QODER_BASE.to_string())
    }

    fn signed(rb: reqwest::RequestBuilder, cred: &Credential) -> Result<reqwest::RequestBuilder> {
        let token = Self::token(cred)?;
        let mut rb = rb
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .header("User-Agent", "Qoder/claim")
            .header("Cosy-ClientType", "10");
        if let Some(cookie) = cred.cookie.as_deref().filter(|s| !s.is_empty()) {
            rb = rb.header("Cookie", cookie);
        }
        Ok(rb)
    }

    async fn fetch_campaigns(&self, cred: &Credential) -> Result<Value> {
        let url = format!("{}{}", Self::base(cred), CAMPAIGNS_PATH);
        let resp = Self::signed(self.client.get(&url), cred)?.send().await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        if let Some(code) = http::code_of(&body) {
            if code != 0 {
                let msg = http::msg_of(&body);
                return Err(Error::Upstream(if msg.is_empty() {
                    format!("Qoder 活动接口返回 code={code}")
                } else {
                    msg
                }));
            }
        }
        Ok(body)
    }

    async fn fetch_balance(&self, cred: &Credential) -> Result<Option<f64>> {
        let url = format!("{}{}", Self::base(cred), USAGE_PATH);
        let resp = Self::signed(self.client.get(&url), cred)?.send().await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        Ok(balance_from_usage(&body))
    }

    /// 领取一条活动。
    async fn claim(&self, cred: &Credential, campaign_id: &str) -> Result<ClaimResult> {
        let url = format!(
            "{}{}/{}/claim",
            Self::base(cred),
            CAMPAIGNS_PATH,
            campaign_id
        );
        let rb = Self::signed(self.client.post(&url), cred)?.json(&json!({}));
        let resp = rb.send().await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        let code = http::code_of(&body);
        let msg = http::msg_of(&body);
        let lowered = msg.to_lowercase();

        if status == 401 || status == 403 {
            return Err(Error::Auth(if msg.is_empty() {
                "Qoder 令牌被拒绝".into()
            } else {
                msg
            }));
        }
        if status == 429 || status >= 500 {
            return Err(Error::Upstream(format!(
                "Qoder 服务暂时不可用（HTTP {status}）"
            )));
        }
        if (400..500).contains(&status) {
            if code == Some(10001) || msg.contains('已') || lowered.contains("already") {
                return Ok(ClaimResult::Already);
            }
            return Err(Error::Upstream(format!(
                "领取 Qoder Credits 失败（HTTP {status}）：{}",
                if msg.is_empty() { "未知错误" } else { &msg }
            )));
        }
        if code == Some(0) {
            return Ok(ClaimResult::Claimed);
        }
        if let Some(data) = body.get("data") {
            if data.is_object() && data.as_object().map_or(false, |o| !o.is_empty()) {
                return Ok(ClaimResult::Claimed);
            }
        }
        if msg.contains('已') || lowered.contains("already") {
            return Ok(ClaimResult::Already);
        }
        // 服务端可能回一个空对象；最终以活动状态为准，不在这里判失败。
        Ok(ClaimResult::Unknown)
    }
}

impl Default for QoderCnProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for QoderCnProvider {
    fn vendor(&self) -> Vendor {
        Vendor::QoderCn
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
            "Qoder CN 的客户端登录态不支持接口续期；过期后重新「本机导入」即可".into(),
        ))
    }

    async fn snapshot(&self, account: &Account, cred: &Credential) -> Result<VendorSnapshot> {
        Self::token(cred)?;
        let now = chrono::Local::now().timestamp();
        let mut snap = VendorSnapshot::empty(account, now);
        let mut notes: Vec<String> = Vec::new();

        match self.fetch_campaigns(cred).await {
            Ok(payload) => {
                let active = active_campaigns(&payload, now);
                if active.is_empty() {
                    notes.push(format!("当前不在领取窗口（{WINDOW_HINT}）"));
                } else {
                    let claimed = active.iter().all(|c| claim_status(c) == "CLAIMED");
                    snap.checked_in_today = Some(claimed);
                    snap.daily_credit = Some(active.iter().map(|c| claim_amount(c)).sum());
                    if let Some(end) = active.iter().filter_map(|c| as_i64(c.get("endAt"))).max() {
                        notes.push(format!("本次窗口至 {}", fmt_local(end)));
                    }
                }
            }
            Err(Error::Auth(m)) => return Err(Error::Auth(m)),
            Err(e) => notes.push(format!("签到状态查询失败：{}", e.user_message())),
        }

        match self.fetch_balance(cred).await {
            Ok(Some(b)) => snap.balance = Some(b),
            Ok(None) => notes.push("余额接口没有返回可识别的配额字段".into()),
            Err(Error::Auth(m)) => return Err(Error::Auth(m)),
            Err(e) => notes.push(format!("余额查询失败：{}", e.user_message())),
        }

        if !notes.is_empty() {
            snap.note = Some(notes.join("；"));
        }
        Ok(snap)
    }

    async fn checkin(&self, account: &Account, cred: &Credential) -> Result<CheckinOutcome> {
        Self::token(cred)?;
        let now = chrono::Local::now().timestamp();
        let payload = self.fetch_campaigns(cred).await?;
        let active = active_campaigns(&payload, now);

        if active.is_empty() {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::QoderCn,
                status: CheckinStatus::NotEligible,
                credit: None,
                streak_days: None,
                message: format!("当前不在 Qoder 领取窗口（{WINDOW_HINT}）"),
                at: now,
            });
        }

        let claimable: Vec<&Value> = active
            .iter()
            .copied()
            .filter(|c| claim_status(c) == "CLAIMABLE")
            .collect();
        if claimable.is_empty() {
            if active.iter().all(|c| claim_status(c) == "CLAIMED") {
                return Ok(CheckinOutcome {
                    account_id: account.id.clone(),
                    vendor: Vendor::QoderCn,
                    status: CheckinStatus::AlreadyDone,
                    credit: None,
                    streak_days: None,
                    message: "今日已签到，无需重复操作".into(),
                    at: now,
                });
            }
            let states: Vec<String> = active.iter().map(|c| claim_status(c).to_string()).collect();
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::QoderCn,
                status: CheckinStatus::Failed,
                credit: None,
                streak_days: None,
                message: format!("活动状态异常（{}）", states.join("/")),
                at: now,
            });
        }

        let mut total = 0i64;
        let mut submitted = false;
        let mut already = 0usize;
        for campaign in &claimable {
            let Some(id) = campaign.get("campaignId").and_then(Value::as_str) else {
                return Err(Error::Upstream("Qoder 活动缺少 campaignId".into()));
            };
            match self.claim(cred, id).await? {
                ClaimResult::Claimed | ClaimResult::Unknown => {
                    submitted = true;
                    total += claim_amount(campaign);
                }
                ClaimResult::Already => already += 1,
            }
        }

        // 以服务端最终状态为准：领取请求成功不代表马上可查，但状态必须能收敛。
        let final_claimed = self
            .fetch_campaigns(cred)
            .await
            .ok()
            .map(|p| {
                let rows = active_campaigns(&p, chrono::Local::now().timestamp());
                !rows.is_empty() && rows.iter().all(|c| claim_status(c) == "CLAIMED")
            })
            .unwrap_or(false);
        let now = chrono::Local::now().timestamp();

        if submitted && final_claimed {
            let mut message = format!("领取成功，+{total} Credits");
            if already > 0 {
                message.push_str(&format!("（另有 {already} 条此前已领）"));
            }
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::QoderCn,
                status: CheckinStatus::Claimed,
                credit: Some(total),
                streak_days: None,
                message,
                at: now,
            });
        }
        if submitted {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::QoderCn,
                status: CheckinStatus::Failed,
                credit: None,
                streak_days: None,
                message: "领取请求已发送，但服务端仍显示未领取，请稍后重试".into(),
                at: now,
            });
        }
        if final_claimed {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::QoderCn,
                status: CheckinStatus::AlreadyDone,
                credit: None,
                streak_days: None,
                message: "今日已签到，无需重复操作".into(),
                at: now,
            });
        }
        Ok(CheckinOutcome {
            account_id: account.id.clone(),
            vendor: Vendor::QoderCn,
            status: CheckinStatus::Failed,
            credit: None,
            streak_days: None,
            message: "领取后活动状态未更新，请稍后重试".into(),
            at: now,
        })
    }
}

// ---------------------------------------------------------------- JSON 小工具

fn as_i64(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn campaign_rows(payload: &Value) -> &[Value] {
    let rows = payload
        .get("campaigns")
        .or_else(|| payload.get("data").and_then(|d| d.get("campaigns")));
    rows.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn action_type(c: &Value) -> &str {
    c.get("actionType").and_then(Value::as_str).unwrap_or("")
}

fn claim_status(c: &Value) -> &str {
    c.get("claimStatus").and_then(Value::as_str).unwrap_or("")
}

fn claim_amount(c: &Value) -> i64 {
    c.get("benefit")
        .and_then(|b| b.get("amount"))
        .and_then(|a| a.as_i64().or_else(|| a.as_f64().map(|f| f as i64)))
        .unwrap_or(100)
}

/// 当前窗口内、可领取 Credits 的活动。
fn active_campaigns<'a>(payload: &'a Value, now: i64) -> Vec<&'a Value> {
    campaign_rows(payload)
        .iter()
        .filter(|c| action_type(c) == "CLAIM_BENEFIT")
        .filter(|c| {
            let (Some(start), Some(end)) = (as_i64(c.get("startAt")), as_i64(c.get("endAt"))) else {
                return false;
            };
            start <= now && now < end
        })
        .collect()
}

/// 各配额桶 `remaining` 之和；拿不到任何一桶就返回 None（不填 0）。
fn balance_from_usage(payload: &Value) -> Option<f64> {
    let usage = payload.get("qoderUsage").unwrap_or(payload);
    let mut total = 0.0;
    let mut found = false;

    let mut add = |bucket: &Value| {
        let Some(obj) = bucket.as_object() else { return };
        let remaining = obj
            .get("remaining")
            .and_then(Value::as_f64)
            .or_else(|| match (obj.get("total"), obj.get("used")) {
                (Some(t), Some(u)) => Some(t.as_f64()? - u.as_f64()?),
                _ => None,
            });
        if let Some(r) = remaining {
            total += r;
            found = true;
        }
    };

    for key in ["userQuota", "addOnQuota", "orgResourcePackage"] {
        if let Some(bucket) = usage.get(key) {
            add(bucket);
        }
    }
    match usage.get("dedicatedResourcePackages") {
        Some(Value::Array(items)) => items.iter().for_each(&mut add),
        Some(Value::Object(map)) => map.values().for_each(&mut add),
        _ => {}
    }

    found.then(|| (total * 100.0).round() / 100.0)
}

fn fmt_local(epoch: i64) -> String {
    chrono::Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|d| d.format("%m-%d %H:%M").to_string())
        .unwrap_or_else(|| epoch.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign(id: &str, start: i64, end: i64, status: &str, amount: i64) -> Value {
        json!({
            "campaignId": id,
            "campaignKey": format!("act-{id}"),
            "actionType": "CLAIM_BENEFIT",
            "claimStatus": status,
            "startAt": start,
            "endAt": end,
            "benefit": { "kind": "CREDITS", "amount": amount }
        })
    }

    #[test]
    fn picks_only_active_claim_benefit_campaigns() {
        let now = 1_000_000;
        let payload = json!({
            "campaigns": [
                campaign("open", now - 60, now + 60, "CLAIMABLE", 100),
                campaign("future", now + 60, now + 3600, "CLAIMABLE", 100),
                campaign("expired", now - 3600, now - 60, "CLAIMABLE", 100),
                { "campaignId": "banner", "actionType": "SHOW_BANNER", "startAt": now - 1, "endAt": now + 1 }
            ]
        });
        let picked = active_campaigns(&payload, now);
        assert_eq!(picked.len(), 1, "只应命中窗口内的 CLAIM_BENEFIT");
        assert_eq!(picked[0]["campaignId"], "open");
        assert_eq!(claim_amount(picked[0]), 100);
    }

    #[test]
    fn balance_sums_quota_buckets_and_ignores_missing() {
        let payload = json!({
            "qoderUsage": {
                "userQuota": { "remaining": 900.5, "total": 1000, "used": 99.5 },
                "addOnQuota": { "remaining": 8.5 },
                "orgResourcePackage": null,
                "dedicatedResourcePackages": [ { "remaining": 1.0 }, { "total": 3, "used": 1 } ]
            }
        });
        assert_eq!(balance_from_usage(&payload), Some(912.0));
        assert_eq!(balance_from_usage(&json!({ "qoderUsage": {} })), None);
        assert_eq!(balance_from_usage(&json!({})), None);
    }

    #[test]
    fn missing_token_is_auth_error() {
        assert!(matches!(
            QoderCnProvider::token(&Credential::default()),
            Err(Error::Auth(_))
        ));
    }
}
