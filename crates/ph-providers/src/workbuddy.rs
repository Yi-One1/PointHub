//! WorkBuddy（腾讯 CodeBuddy）适配器。
//!
//! 三个容易踩死的点，都在这里显式处理：
//! 1. **签到域名不是写死的**：以凭据里的 `domain` 为准（登录态里带），
//!    写死 `copilot.tencent.com` 会 404。
//! 2. **派猫猫旅行是另一个域名，且路径不带 `/v2`**。
//! 3. **余额字段是复数 `total_credits`**，只认单数会永远拿到空。
//!
//! 「已签到」是幂等结果，不是错误——必须映射成 `AlreadyDone`，
//! 否则每天第二次点击都会弹一个吓人的红字。

use crate::http::{self, BALANCE_KEYS};
use crate::{Capabilities, Provider, TravelAction};
use async_trait::async_trait;
use ph_core::{
    Account, CheckinOutcome, CheckinStatus, Credential, Error, Result, TravelSnapshot, TravelState,
    Vendor, VendorSnapshot,
};
use rand::Rng;
use serde_json::{json, Value};

/// 长效令牌刷新网关（与签到域名无关，固定地址）。
const REFRESH_URL: &str = "https://copilot.tencent.com/v2/plugin/auth/token/refresh";
/// 签到默认域名，仅当凭据里没有 domain 时兜底。
const DEFAULT_SIGNIN_DOMAIN: &str = "www.codebuddy.cn";
/// 派猫猫旅行域名（与签到不同，无 /v2 前缀）。
const TRAVEL_BASE: &str = "https://www.workbuddy.cn";

pub struct WorkBuddyProvider {
    client: reqwest::Client,
}

impl WorkBuddyProvider {
    pub fn new() -> Self {
        Self {
            client: http::build_client(None).expect("默认客户端一定可构建"),
        }
    }

    fn signin_base(cred: &Credential) -> String {
        let host = cred
            .domain
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_SIGNIN_DOMAIN);
        let host = host.trim_end_matches('/');
        if host.starts_with("http") {
            host.to_string()
        } else {
            format!("https://{host}")
        }
    }

    /// 保证 `cred.access_token` 可用：没有或快过期就用长效令牌换一个。
    async fn ensure_token(&self, cred: &mut Credential) -> Result<()> {
        let now = chrono::Local::now().timestamp();
        let need = match (&cred.access_token, cred.expires_at) {
            (None, _) => true,
            (Some(_), Some(exp)) => exp - 60 <= now,
            (Some(_), None) => false,
        };
        if need {
            self.refresh(cred).await?;
        }
        if cred.access_token.is_none() {
            return Err(Error::Auth(
                "该账号没有可用的接口令牌，也没有长效令牌。请重新导入。".into(),
            ));
        }
        Ok(())
    }

    async fn get_status(&self, cred: &mut Credential) -> Result<Value> {
        self.ensure_token(cred).await?;
        let base = Self::signin_base(cred);
        let url = format!("{base}/v2/billing/meter/checkin-activity-status");
        let token = cred.access_token.clone().unwrap_or_default();
        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&json!({}))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        Ok(body)
    }
}

impl Default for WorkBuddyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for WorkBuddyProvider {
    fn vendor(&self) -> Vendor {
        Vendor::WorkBuddy
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            checkin: true,
            travel: true,
            balance: true,
            token_refresh: true,
            verified: true,
        }
    }

    async fn refresh(&self, cred: &mut Credential) -> Result<()> {
        let Some(rt) = cred.refresh_token.clone().filter(|s| !s.trim().is_empty()) else {
            return Ok(());
        };
        let resp = self
            .client
            .post(REFRESH_URL)
            .header("Content-Type", "application/json")
            .header("X-Refresh-Token", rt)
            .header("X-Auth-Refresh-Source", "plugin")
            .header("X-Domain", "copilot.tencent.com")
            .json(&json!({}))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(match e {
                Error::Upstream(m) => Error::Auth(format!("长效令牌刷新失败：{m}")),
                other => other,
            });
        }
        let code = http::code_of(&body).unwrap_or(0);
        if code != 0 {
            return Err(Error::Auth(format!(
                "长效令牌已失效（code {code} {}），请重新导入该账号",
                http::msg_of(&body)
            )));
        }
        let data = body.get("data").cloned().unwrap_or(Value::Null);
        let at = http::pick_str(&data, &["accessToken", "access_token", "token"]);
        let rt_new = http::pick_str(&data, &["refreshToken", "refresh_token"]);
        if at.is_none() {
            return Err(Error::Upstream(
                "刷新成功但响应里没有 accessToken，平台可能改版".into(),
            ));
        }
        cred.access_token = at;
        if let Some(n) = rt_new {
            // 每次刷新都轮换，必须回写，否则下次就用不了。
            cred.refresh_token = Some(n);
        }
        // 接口令牌通常 30 分钟量级；拿不到过期时间就按 20 分钟保守估计。
        let ttl = http::pick_i64(&data, &["expiresIn", "expires_in"]).unwrap_or(1200);
        cred.expires_at = Some(chrono::Local::now().timestamp() + ttl);
        Ok(())
    }

    async fn snapshot(&self, account: &Account, cred: &Credential) -> Result<VendorSnapshot> {
        let mut cred = cred.clone();
        let body = self.get_status(&mut cred).await?;
        let data = body.get("data").cloned().unwrap_or_else(|| body.clone());

        let mut snap = VendorSnapshot::empty(account, chrono::Local::now().timestamp());
        snap.balance = http::pick_i64(&data, BALANCE_KEYS).or_else(|| http::pick_i64(&body, BALANCE_KEYS));
        snap.checked_in_today = http::pick_bool(&data, &["today_checked_in", "checked_in_today", "todayCheckedIn"]);
        snap.streak_days = http::pick_i64(&data, &["streak_days", "streakDays", "continuous_days"]);
        snap.daily_credit = http::pick_i64(&data, &["daily_credit", "dailyCredit", "today_credit"]);

        if snap.balance.is_none() {
            snap.note = Some("平台未返回余额字段（可能是改版），可稍后重试".into());
        }
        Ok(snap)
    }

    async fn checkin(&self, account: &Account, cred: &Credential) -> Result<CheckinOutcome> {
        let mut cred = cred.clone();
        self.ensure_token(&mut cred).await?;
        let base = Self::signin_base(&cred);
        let url = format!("{base}/v2/billing/meter/daily-checkin");
        let token = cred.access_token.clone().unwrap_or_default();
        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&json!({}))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));

        let now = chrono::Local::now().timestamp();
        let code = http::code_of(&body);
        let msg = http::msg_of(&body);
        let data = body.get("data").cloned().unwrap_or(Value::Null);
        let credit = http::pick_i64(&data, &["credit", "daily_credit"])
            .or_else(|| http::pick_i64(&body, &["credit", "daily_credit"]));
        let streak = http::pick_i64(&data, &["streak_days"]).or_else(|| http::pick_i64(&body, &["streak_days"]));

        // 401/403 一定是凭据问题，先判。
        if status == 401 || status == 403 {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::WorkBuddy,
                status: CheckinStatus::AuthFailed,
                credit: None,
                streak_days: None,
                message: if msg.is_empty() { "令牌被拒绝".into() } else { msg },
                at: now,
            });
        }

        // 10001 = 今天已签到（幂等），不是错误。
        if code == Some(10001) || msg.contains("已签到") {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::WorkBuddy,
                status: CheckinStatus::AlreadyDone,
                credit: None,
                streak_days: streak,
                message: if msg.is_empty() { "今天已签到".into() } else { msg },
                at: now,
            });
        }

        if code == Some(0) && status < 400 {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::WorkBuddy,
                status: CheckinStatus::Claimed,
                credit,
                streak_days: streak,
                message: match credit {
                    Some(c) => format!("领取成功，+{c} 积分"),
                    None => "领取成功".into(),
                },
                at: now,
            });
        }

        // 其余情况：把平台原话带出去，不猜。
        Ok(CheckinOutcome {
            account_id: account.id.clone(),
            vendor: Vendor::WorkBuddy,
            status: CheckinStatus::Failed,
            credit: None,
            streak_days: streak,
            message: format!(
                "未成功（HTTP {status}, code {}）：{}",
                code.map(|c| c.to_string()).unwrap_or_else(|| "-".into()),
                if msg.is_empty() { "平台未给出原因" } else { &msg }
            ),
            at: now,
        })
    }

    async fn travel(
        &self,
        _account: &Account,
        cred: &Credential,
        action: TravelAction,
        location_id: Option<u32>,
    ) -> Result<TravelSnapshot> {
        let mut cred = cred.clone();
        self.ensure_token(&mut cred).await?;
        let token = cred.access_token.clone().unwrap_or_default();

        let mut snap = self.travel_status(&token).await?;

        if action == TravelAction::Status {
            return Ok(snap);
        }

        // 已到达 -> 先领，不管动作是什么（不领会过期吗？不会，但领了才可能再派）。
        if snap.state == TravelState::Arrived {
            match self.travel_claim(&token).await {
                Ok(gain) => {
                    snap.note = Some(match gain {
                        Some(g) => format!("已补领旅行奖励 +{g} 分"),
                        None => "已领取旅行奖励".into(),
                    });
                    snap.state = TravelState::Idle;
                }
                Err(e) => {
                    snap.note = Some(format!("领取失败：{}", e.user_message()));
                    return Ok(snap);
                }
            }
        }

        if action == TravelAction::ClaimOnly {
            return Ok(snap);
        }

        // 空闲且未达上限 -> 派出。
        if snap.state == TravelState::Idle && !snap.daily_limit_reached {
            let loc = location_id.unwrap_or_else(|| rand::thread_rng().gen_range(1..=4));
            match self.travel_depart(&token, loc).await {
                Ok(Some(state)) => {
                    snap.state = state;
                    let extra = snap.note.clone().unwrap_or_default();
                    snap.note = Some(if extra.is_empty() {
                        format!("已派出（地点 {loc}）")
                    } else {
                        format!("{extra}；已派出（地点 {loc}）")
                    });
                }
                Ok(None) => {
                    let extra = snap.note.clone().unwrap_or_default();
                    snap.note = Some(if extra.is_empty() {
                        "已派出".into()
                    } else {
                        format!("{extra}；已派出")
                    });
                }
                Err(e) => {
                    let extra = snap.note.clone().unwrap_or_default();
                    snap.note = Some(format!("{extra}；派出失败：{}", e.user_message()));
                }
            }
        } else if snap.state == TravelState::Idle && snap.daily_limit_reached {
            snap.note = Some(format!(
                "{}；今日派遣已达上限",
                snap.note.clone().unwrap_or_default()
            ));
        }

        Ok(snap)
    }
}

impl WorkBuddyProvider {
    async fn travel_status(&self, token: &str) -> Result<TravelSnapshot> {
        let url = format!("{TRAVEL_BASE}/activity/growth/buddy/travel/status");
        let resp = self.client.get(&url).bearer_auth(token).send().await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        let data = body.get("data").cloned().unwrap_or(Value::Null);
        let raw_state = http::pick_str(&data, &["state"]).unwrap_or_else(|| "unknown".into());
        let location = match data.get("location") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Object(o)) => http::pick_str(&Value::Object(o.clone()), &["name", "title"]),
            _ => http::pick_str(&data, &["location_name"]),
        };
        Ok(TravelSnapshot {
            state: TravelState::parse(&raw_state),
            location,
            arrive_at: http::pick_i64(&data, &["arrive_at", "arriveAt"]),
            reward_credit: http::pick_i64(&data, &["reward_credit", "rewardCredit"]),
            daily_limit_reached: http::pick_bool(&data, &["daily_limit_reached"]).unwrap_or(false),
            note: if raw_state == "unknown" {
                Some(format!("未识别的旅行状态：{raw_state}"))
            } else {
                None
            },
        })
    }

    async fn travel_claim(&self, token: &str) -> Result<Option<i64>> {
        let url = format!("{TRAVEL_BASE}/activity/growth/buddy/travel/claim");
        let resp = self.client.post(&url).bearer_auth(token).json(&json!({})).send().await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        let code = http::code_of(&body).unwrap_or(0);
        if code != 0 {
            return Err(Error::Upstream(format!(
                "领取被拒（code {code}）：{}",
                http::msg_of(&body)
            )));
        }
        let data = body.get("data").cloned().unwrap_or(Value::Null);
        Ok(http::pick_i64(&data, &["credit", "reward_credit"])
            .or_else(|| http::pick_i64(&body, &["credit", "reward_credit"])))
    }

    /// 返回派出后的状态（若平台没给就返回 None）。
    async fn travel_depart(&self, token: &str, location_id: u32) -> Result<Option<TravelState>> {
        let url = format!("{TRAVEL_BASE}/activity/growth/buddy/travel/depart");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&json!({ "location_id": location_id }))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        let code = http::code_of(&body).unwrap_or(0);
        if code != 0 {
            let msg = http::msg_of(&body);
            if msg.contains("no active buddy") || msg.contains("未激活") {
                return Err(Error::NotEligible(
                    "该账号尚未在客户端激活派猫猫旅行".into(),
                ));
            }
            return Err(Error::Upstream(format!("派出被拒（code {code}）：{msg}")));
        }
        let data = body.get("data").cloned().unwrap_or(Value::Null);
        Ok(http::pick_str(&data, &["state"]).map(|s| TravelState::parse(&s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signin_base_prefers_credential_domain() {
        let mut c = Credential::default();
        assert_eq!(
            WorkBuddyProvider::signin_base(&c),
            "https://www.codebuddy.cn"
        );
        c.domain = Some("www.codebuddy.cn".into());
        assert_eq!(
            WorkBuddyProvider::signin_base(&c),
            "https://www.codebuddy.cn"
        );
        c.domain = Some("http://127.0.0.1:8080".into());
        assert_eq!(WorkBuddyProvider::signin_base(&c), "http://127.0.0.1:8080");
    }

    #[test]
    fn capabilities_match_reality() {
        let p = WorkBuddyProvider::new();
        let c = p.capabilities();
        assert!(c.checkin && c.travel && c.balance && c.token_refresh && c.verified);
    }
}
