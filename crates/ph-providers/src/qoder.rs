//! Qoder CN（阿里）适配器。
//!
//! ## 现状（必须诚实说明）
//!
//! 1. 自 2026-11-19 10:00 起，Qoder 每日 100 Credits **仅对付费/试用账号开放**，
//!    体验版（免费）账号的入口被服务端直接隐藏，返回的是「无资格」而**不是**错误。
//!    因此本适配器把「无资格」映射为 [`ph_core::CheckinStatus::NotEligible`]，
//!    不重试、不报警、不计入失败数。
//!
//! 2. 领取走的是活动制：先取活动列表，再对 `kind == CREDITS` 的那条领取，
//!    并且请求必须携带设备风控头（`Cosy-MachineToken` / `Cosy-MachineCode` /
//!    `Cosy-MachineType`），缺头会返回空或 503。
//!
//! 3. **本版尚未取得可公开验证的领取端点**，因此：
//!    - 若 `Credential.domain` 里给了完整地址（形如 `https://host/path`），
//!      本适配器会按「POST 该地址」尝试领取；
//!    - 否则直接返回 `Unsupported`，**绝不猜测路径**。
//!
//! 为什么宁可不做也不猜：猜出来的 404 会被用户当成「软件坏了」，
//! 而实际是端点未知。明确说「待实测」比假装能用更有价值。

use crate::http;
use crate::{Capabilities, Provider};
use async_trait::async_trait;
use ph_core::{
    Account, CheckinOutcome, CheckinStatus, Credential, Error, Result, Vendor, VendorSnapshot,
};
use serde_json::{json, Value};

pub struct QoderCnProvider {
    client: reqwest::Client,
}

impl QoderCnProvider {
    pub fn new() -> Self {
        Self {
            client: http::build_client(None).expect("默认客户端一定可构建"),
        }
    }

    fn endpoint(cred: &Credential) -> Option<String> {
        cred.domain
            .as_deref()
            .map(str::trim)
            .filter(|s| s.starts_with("http") && s.contains('/'))
            .map(|s| s.to_string())
    }

    fn with_device_headers<'a>(
        rb: reqwest::RequestBuilder,
        cred: &'a Credential,
    ) -> reqwest::RequestBuilder {
        let mut rb = rb;
        if let Some(t) = cred.device_id.as_deref().filter(|s| !s.is_empty()) {
            // 设备指纹三件套：缺任一项，服务端会返回空活动或 503。
            rb = rb
                .header("Cosy-MachineToken", t)
                .header("Cosy-MachineCode", t)
                .header("Cosy-MachineType", "windows");
        }
        if let Some(c) = cred.cookie.as_deref().filter(|s| !s.is_empty()) {
            rb = rb.header("Cookie", c);
        }
        rb
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
            // 端点待实测，先声明为 false，界面才不会给出一个点了就报错的按钮。
            checkin: false,
            travel: false,
            balance: false,
            token_refresh: false,
            verified: false,
        }
    }

    async fn refresh(&self, _cred: &mut Credential) -> Result<()> {
        Err(Error::Unsupported(
            "Qoder CN 的令牌刷新未实现，请在过期后重新导入".into(),
        ))
    }

    async fn snapshot(&self, account: &Account, cred: &Credential) -> Result<VendorSnapshot> {
        let Some(url) = Self::endpoint(cred) else {
            return Err(Error::Unsupported(
                "Qoder CN 的积分查询端点尚未实测确认，本版不提供余额。详见 README 的「平台能力矩阵」。"
                    .into(),
            ));
        };
        let rb = self.client.get(&url);
        let resp = Self::with_device_headers(rb, cred).send().await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if let Some(e) = http::map_status(status, &body) {
            return Err(e);
        }
        let data = body.get("data").cloned().unwrap_or_else(|| body.clone());
        let mut snap = VendorSnapshot::empty(account, chrono::Local::now().timestamp());
        snap.balance = http::pick_f64(&data, http::BALANCE_KEYS);
        snap.note = Some("Qoder 余额来自自定义端点，字段按通用候选名解析".into());
        Ok(snap)
    }

    async fn checkin(&self, account: &Account, cred: &Credential) -> Result<CheckinOutcome> {
        let Some(url) = Self::endpoint(cred) else {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::QoderCn,
                status: CheckinStatus::NotEligible,
                credit: None,
                streak_days: None,
                message: "Qoder CN 端点未配置（v0.1.0 待实测），本次跳过，不计为失败".into(),
                at: chrono::Local::now().timestamp(),
            });
        };

        let rb = self.client.post(&url);
        let resp = Self::with_device_headers(rb, cred).json(&json!({})).send().await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        let now = chrono::Local::now().timestamp();
        let code = http::code_of(&body);
        let msg = http::msg_of(&body);
        let data = body.get("data").cloned().unwrap_or(Value::Null);

        if status == 401 || status == 403 {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::QoderCn,
                status: CheckinStatus::AuthFailed,
                credit: None,
                streak_days: None,
                message: if msg.is_empty() { "凭据被拒绝".into() } else { msg },
                at: now,
            });
        }
        // 「没有资格」不是错误：Qoder 免费号自 2026-11-19 起就是这种状态。
        if msg.contains("无资格")
            || msg.contains("不满足")
            || msg.contains("not eligible")
            || msg.contains("empty")
        {
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::QoderCn,
                status: CheckinStatus::NotEligible,
                credit: None,
                streak_days: None,
                message: if msg.is_empty() {
                    "当前账号不参与该活动".into()
                } else {
                    msg
                },
                at: now,
            });
        }
        if code == Some(0) && status < 400 {
            let credit = http::pick_i64(&data, &["credit", "amount", "credits"]);
            return Ok(CheckinOutcome {
                account_id: account.id.clone(),
                vendor: Vendor::QoderCn,
                status: CheckinStatus::Claimed,
                credit,
                streak_days: None,
                message: match credit {
                    Some(c) => format!("领取成功，+{c} Credits"),
                    None => "领取成功".into(),
                },
                at: now,
            });
        }
        Ok(CheckinOutcome {
            account_id: account.id.clone(),
            vendor: Vendor::QoderCn,
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
