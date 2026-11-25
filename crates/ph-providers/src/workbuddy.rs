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

use crate::http;
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
/// 插件授权网关：客户端插件登录走的就是这套，换出来的是**长效**令牌。
const AUTH_BASE: &str = "https://copilot.tencent.com";
/// 网关会校验 UA，非浏览器 UA 直接 401。
const BROWSER_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/126.0.0.0 Safari/537.36";
/// 签到默认域名，仅当凭据里没有 domain 时兜底。
const DEFAULT_SIGNIN_DOMAIN: &str = "www.codebuddy.cn";
/// 派猫猫旅行域名（与签到不同，无 /v2 前缀）。
const TRAVEL_BASE: &str = "https://www.workbuddy.cn";
/// 积分汇总：**账户可用积分**的正主（注意这个路径没有 /v2）。
const SUMMARY_PATH: &str = "/billing/meter/get-user-resource-summary";

pub struct WorkBuddyProvider {
    client: reqwest::Client,
}

// ---------------------------------------------------------------- 授权登录

/// 发起授权的返回：`auth_url` 给用户点，`state` 用来轮询结果。
#[derive(Debug, Clone)]
pub struct AuthStart {
    pub state: String,
    pub auth_url: String,
}

/// 授权换回来的凭据。`refresh_token` 才是长期能用的那把钥匙。
#[derive(Debug, Clone)]
pub struct AuthCredential {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// access_token 过期时间（epoch 秒）。
    pub expires_at: Option<i64>,
    pub domain: Option<String>,
}

/// 一次轮询的结果。
#[derive(Debug, Clone)]
pub enum AuthPoll {
    /// 用户还没在浏览器里点授权。
    Pending,
    /// 拿到了凭据。
    Ready(AuthCredential),
    /// 会话已作废（超时或被用过），只能重新发起。
    Expired,
}

/// 积分展示：整数不带小数点，有小数就保留两位（如 `204.62`）。
pub fn fmt_credits(v: f64) -> String {
    if (v - v.round()).abs() < 0.005 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
    }
}

/// 账户积分总量（汇总口径）。
#[derive(Debug, Clone, Copy)]
pub struct CreditTotals {
    pub granted: f64,
    pub used: f64,
    pub remaining: f64,
}

/// `expiresAt` 常见是毫秒，`expiresIn` 是秒；统一折算成 epoch 秒。
fn expiry_secs(payloads: &[&Value]) -> Option<i64> {
    for p in payloads {
        if let Some(n) = http::pick_i64(p, &["expiresAt", "expires_at"]) {
            if n > 0 {
                return Some(if n > 1_000_000_000_000 { n / 1000 } else { n });
            }
        }
        if let Some(secs) = http::pick_i64(p, &["expiresIn", "expires_in"]) {
            if secs > 0 {
                return Some(chrono::Local::now().timestamp() + secs);
            }
        }
    }
    None
}

/// 浏览器授权拿到的令牌与 UA 绑定：抓取时用的 UA 必须原样回放，
/// 否则网关会把请求当成「换了个客户端」而拒绝。
fn with_ua(rb: reqwest::RequestBuilder, cred: &Credential) -> reqwest::RequestBuilder {
    match cred.user_agent.as_deref() {
        Some(ua) if !ua.trim().is_empty() => rb.header(reqwest::header::USER_AGENT, ua.trim()),
        _ => rb,
    }
}

impl WorkBuddyProvider {
    pub fn new() -> Self {
        Self {
            client: http::build_client(None).expect("默认客户端一定可构建"),
        }
    }

    /// 发起插件授权：拿到要打开的授权页地址，以及用于轮询的 state。
    ///
    /// 这是客户端插件登录用的同一套流程。相比「读网页登录态」，好处是换回来的是
    /// **长效令牌**（能自续期），而不是随请求轮换的网页会话。
    pub async fn begin_auth(&self) -> Result<AuthStart> {
        let resp = self
            .client
            .post(format!("{AUTH_BASE}/v2/plugin/auth/state?platform=workbuddy"))
            .header("accept", "application/json")
            .header("X-Domain", "copilot.tencent.com")
            .header(reqwest::header::USER_AGENT, BROWSER_UA)
            .json(&json!({}))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        let data = body.get("data").cloned().unwrap_or(Value::Null);
        let state = http::pick_str(&data, &["state"])
            .or_else(|| http::pick_str(&body, &["state"]))
            .ok_or_else(|| Error::Upstream("授权接口没有返回 state，平台可能改版".into()))?;
        let auth_url = http::pick_str(&data, &["authUrl", "auth_url", "url"])
            .or_else(|| http::pick_str(&body, &["authUrl"]))
            .ok_or_else(|| Error::Upstream("授权接口没有返回授权地址，平台可能改版".into()))?;
        Ok(AuthStart { state, auth_url })
    }

    /// 轮询一次授权结果。用户还没点授权时返回 `Pending`，不是错误。
    pub async fn poll_auth(&self, state: &str) -> Result<AuthPoll> {
        let resp = self
            .client
            .get(format!("{AUTH_BASE}/v2/plugin/auth/token"))
            .query(&[("state", state)])
            .header("accept", "application/json")
            .header(reqwest::header::USER_AGENT, BROWSER_UA)
            // 轮询时还没有身份，显式告诉网关「别按已登录校验」
            .header("X-No-Authorization", "true")
            .header("X-No-User-Id", "true")
            .header("X-No-Enterprise-Id", "true")
            .header("X-No-Department-Info", "true")
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status == 404 {
            return Ok(AuthPoll::Expired);
        }
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));

        // 202 与 code=11217（"login ing..."）都表示还在等用户点授权。
        if status == 202 || http::code_of(&body) == Some(11217) {
            return Ok(AuthPoll::Pending);
        }
        if status >= 400 {
            if let Some(e) = http::map_status(status, &body) {
                return Err(e);
            }
        }

        let payloads = [&body["data"]["data"], &body["data"], &body];
        let pick = |keys: &[&str]| -> Option<String> {
            payloads.iter().find_map(|d| http::pick_str(d, keys))
        };
        let Some(access_token) = pick(&["accessToken", "access_token", "token"]) else {
            return Ok(AuthPoll::Pending);
        };
        Ok(AuthPoll::Ready(AuthCredential {
            access_token,
            refresh_token: pick(&["refreshToken", "refresh_token"]),
            expires_at: expiry_secs(&payloads),
            domain: pick(&["domain"]),
        }))
    }

    /// 账户可用积分。
    ///
    /// 必须走汇总接口：签到活动返回的 `total_credits` 是**本季活动累计得分**
    /// （实测连签 36 天 = 3600，而账户实际可用是另一个数），拿它当余额会显示成
    /// 一个越来越大的假数字。
    async fn credit_totals(&self, cred: &mut Credential) -> Result<CreditTotals> {
        self.ensure_token(cred).await?;
        let base = Self::signin_base(cred);
        let token = cred.access_token.clone().unwrap_or_default();
        let resp = with_ua(
            self.client.post(format!("{base}{SUMMARY_PATH}")).bearer_auth(token),
            cred,
        )
        .json(&json!({}))
        .send()
        .await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        let data = body.get("data").cloned().unwrap_or(Value::Null);
        // TotalCredits 就是「剩余可用」，与官网口径一致。
        let remaining = http::pick_f64(&data, &["TotalCredits", "total_credits"])
            .ok_or_else(|| Error::Upstream("积分汇总接口没有返回 TotalCredits".into()))?;
        let mut granted = 0f64;
        let mut used = 0f64;
        if let Some(groups) = data.get("CreditGroups").and_then(Value::as_array) {
            for g in groups {
                granted += http::pick_f64(g, &["CycleTotalCapacity"]).unwrap_or(0.0);
                used += http::pick_f64(g, &["CycleUsedCapacity"]).unwrap_or(0.0);
            }
        }
        Ok(CreditTotals {
            granted,
            used,
            remaining,
        })
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
        let resp = with_ua(self.client.post(&url).bearer_auth(token), cred)
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
        snap.checked_in_today = http::pick_bool(&data, &["today_checked_in", "checked_in_today", "todayCheckedIn"]);
        snap.streak_days = http::pick_i64(&data, &["streak_days", "streakDays", "continuous_days"]);
        snap.daily_credit = http::pick_i64(&data, &["daily_credit", "dailyCredit", "today_credit"]);

        // 余额单独取一次：签到活动的 total_credits 是活动累计得分，不能当余额。
        match self.credit_totals(&mut cred).await {
            Ok(t) => snap.balance = Some(t.remaining),
            Err(e) => {
                snap.note = Some(format!("读取积分余额失败：{}", e.user_message()));
            }
        }
        Ok(snap)
    }

    async fn checkin(&self, account: &Account, cred: &Credential) -> Result<CheckinOutcome> {
        let mut cred = cred.clone();
        self.ensure_token(&mut cred).await?;
        let base = Self::signin_base(&cred);
        let url = format!("{base}/v2/billing/meter/daily-checkin");
        let token = cred.access_token.clone().unwrap_or_default();
        let resp = with_ua(self.client.post(&url).bearer_auth(token), &cred)
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

        let mut snap = self.travel_status(&cred).await?;

        if action == TravelAction::Status {
            return Ok(snap);
        }

        // 已到达 -> 先领，不管动作是什么（不领会过期吗？不会，但领了才可能再派）。
        if snap.state == TravelState::Arrived {
            match self.travel_claim(&cred).await {
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
            match self.travel_depart(&cred, loc).await {
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
    async fn travel_status(&self, cred: &Credential) -> Result<TravelSnapshot> {
        let token = cred.access_token.clone().unwrap_or_default();
        let url = format!("{TRAVEL_BASE}/activity/growth/buddy/travel/status");
        let resp = with_ua(self.client.get(&url).bearer_auth(token), cred).send().await?;
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

    async fn travel_claim(&self, cred: &Credential) -> Result<Option<i64>> {
        let token = cred.access_token.clone().unwrap_or_default();
        let url = format!("{TRAVEL_BASE}/activity/growth/buddy/travel/claim");
        let resp = with_ua(self.client.post(&url).bearer_auth(token), cred)
            .json(&json!({}))
            .send()
            .await?;
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
    async fn travel_depart(
        &self,
        cred: &Credential,
        location_id: u32,
    ) -> Result<Option<TravelState>> {
        let token = cred.access_token.clone().unwrap_or_default();
        let url = format!("{TRAVEL_BASE}/activity/growth/buddy/travel/depart");
        let resp = with_ua(self.client.post(&url).bearer_auth(token), cred)
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
