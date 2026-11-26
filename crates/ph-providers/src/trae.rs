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
const TRAE_WEB: &str = "https://www.trae.cn/authorization";
const STATUS_PATH: &str = "/trae/api/v2/ug/checkin_credits/status";
const CLAIM_PATH: &str = "/trae/api/v2/ug/checkin_credits/claim";
const BALANCE_PATH: &str = "/trae/api/v2/pay/ide_user_ent_usage";
const EXCHANGE_PATH: &str = "/trae/api/v3/oauth/ExchangeToken";
/// IDE 版客户端 id（SOLO 版是另一套，本版只接 IDE）。
const CLIENT_ID_IDE: &str = "ono9krqynydwx5";
const FALLBACK_CLIENT_VERSION: &str = "3.4.25";
const FALLBACK_TRON_VERSION: &str = "2.3.108162";

/// 浏览器授权会话：私钥只留在内存，授权成功后随凭据一起加密保存。
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub id: String,
    pub auth_url: String,
    pub login_trace: String,
    pub device_id: String,
    pub machine_id: String,
    verifier: String,
    private_pem: String,
    public_pem: String,
    client_id: String,
    client_version: String,
    created_at: i64,
}

impl AuthSession {
    pub fn is_expired(&self, now: i64) -> bool {
        now - self.created_at > 300
    }
}

pub struct TraeCnProvider {
    client: reqwest::Client,
}

impl TraeCnProvider {
    pub fn new() -> Self {
        Self {
            client: http::build_client(None).expect("默认客户端一定可构建"),
        }
    }

    /// 发起浏览器授权。`callback_url` 由桌面端监听（形如 `http://127.0.0.1:PORT/authorize`）。
    pub fn begin_auth(callback_url: &str) -> Result<AuthSession> {
        let (private_pem, public_pem) = generate_keypair()?;
        let verifier = crate::oauth::b64url(&crate::oauth::random_bytes(48)?);
        let challenge = crate::oauth::code_challenge(&verifier);
        let login_trace = crate::oauth::random_hex(32)?;
        let device_id = random_device_id()?;
        let machine_id = stable_machine_id();
        let (client_version, tron_version) = detect_client_version();

        let auth_url = format!(
            "{TRAE_WEB}?login_version=1&auth_from=trae&login_channel=native_ide&plugin_version={}\
&auth_type=local&client_id={}&redirect=0&login_trace_id={}&auth_callback_url={}\
&machine_id={}&device_id={}&x_device_id={}&x_machine_id={}&x_device_brand=PC&x_device_type=PC\
&x_os_version=10.0&x_app_version={}&x_app_type=stable&code_challenge={}&code_challenge_method=S256",
            crate::oauth::url_encode(&tron_version),
            CLIENT_ID_IDE,
            login_trace,
            crate::oauth::url_encode(callback_url),
            machine_id,
            device_id,
            device_id,
            machine_id,
            crate::oauth::url_encode(&client_version),
            crate::oauth::url_encode(&challenge)
        );

        Ok(AuthSession {
            id: crate::oauth::random_id()?,
            auth_url,
            login_trace,
            device_id,
            machine_id,
            verifier,
            private_pem,
            public_pem,
            client_id: CLIENT_ID_IDE.to_string(),
            client_version,
            created_at: chrono::Local::now().timestamp(),
        })
    }

    /// 用回调里的 AuthCode 换令牌。`user_info` 来自回调，用于补昵称 / 邮箱。
    pub async fn exchange(
        &self,
        session: &AuthSession,
        auth_code: &str,
        verifier: Option<&str>,
        user_info: &Value,
    ) -> Result<Credential> {
        let body = json!({
            "ClientID": session.client_id,
            "AuthCode": auth_code,
            "CodeVerifier": verifier.filter(|s| !s.is_empty()).unwrap_or(&session.verifier),
            "DeviceInfo": {
                "DeviceID": session.device_id,
                "MachineID": session.machine_id,
                "PlatformCode": "IDE_PC",
                "DeviceType": "PC",
                "DeviceName": hostname(),
                "DeviceModel": "",
                "ClientVersion": session.client_version,
                "DevicePublicKey": session.public_pem,
                "DeviceBrand": "",
                "DeviceCPU": "",
                "OSInfo": "Windows",
                "OSVersion": "10.0",
            },
            "IDEVersion": session.client_version,
        });
        let resp = self
            .client
            .post(format!("{TRAE_BASE}{EXCHANGE_PATH}"))
            .header("Content-Type", "application/json")
            .header("x-cloudide-token", "")
            .json(&body)
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if status == 401 || status == 403 {
            return Err(Error::Auth("Trae 授权码已失效，请重新发起授权".into()));
        }
        if status >= 400 {
            return Err(Error::Upstream(format!(
                "Trae 授权失败（HTTP {status}）：{}",
                http::msg_of(&body)
            )));
        }
        let tokens = parse_token_response(&body)
            .ok_or_else(|| Error::Upstream("Trae 授权响应里没有令牌".into()))?;
        let uid = tokens
            .user_id
            .or_else(|| pick_user_field(user_info, &["UserID", "userId", "uid"]))
            .ok_or_else(|| Error::Upstream("Trae 授权响应里没有 user_id".into()))?;

        Ok(Credential {
            access_token: Some(tokens.access_token),
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            platform_uid: Some(uid),
            device_id: Some(session.device_id.clone()),
            machine_id: Some(session.machine_id.clone()),
            device_key: Some(session.private_pem.clone()),
            ..Default::default()
        })
    }

    /// 用设备私钥签名，把 refresh_token 换成新令牌。
    async fn refresh_with_key(&self, cred: &mut Credential) -> Result<()> {
        let refresh_token = cred
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::Auth("Trae 凭据缺少 refresh token".into()))?
            .to_string();
        let private_pem = cred
            .device_key
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| Error::Auth("Trae 凭据缺少设备私钥，请重新授权".into()))?;
        let public_pem = public_pem_from_private(private_pem)?;
        let client_id = CLIENT_ID_IDE;

        let timestamp = chrono::Local::now().timestamp();
        let nonce = crate::oauth::random_hex(16)?;
        let canonical = format!(
            "POST\n{EXCHANGE_PATH}\n{client_id}\n{refresh_token}\n{timestamp}\n{nonce}"
        );
        let signature = sign_der_b64(private_pem, canonical.as_bytes())?;

        let body = json!({
            "ClientID": client_id,
            "ClientSecret": "",
            "RefreshToken": refresh_token,
            "DeviceInfo": {
                "DeviceID": cred.device_id.as_deref().unwrap_or(""),
                "MachineID": cred.machine_id.as_deref().unwrap_or(""),
                "PlatformCode": "IDE_PC",
                "DeviceType": "PC",
                "DeviceName": hostname(),
                "DeviceModel": "",
                "ClientVersion": detect_client_version().0,
                "DevicePublicKey": public_pem,
                "DeviceBrand": "",
                "DeviceCPU": "",
                "OSInfo": "Windows",
                "OSVersion": "10.0",
            },
            "DeviceProof": {
                "Signature": signature,
                "Timestamp": timestamp,
                "Nonce": nonce,
            },
            "IDEVersion": detect_client_version().0,
        });
        let resp = self
            .client
            .post(format!("{TRAE_BASE}{EXCHANGE_PATH}"))
            .header("Content-Type", "application/json")
            .header(
                "x-cloudide-token",
                cred.access_token.as_deref().unwrap_or_default(),
            )
            .json(&body)
            .send()
            .await?;
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
        if status == 401 || status == 403 {
            return Err(Error::Auth("Trae refresh token 已失效，请重新授权".into()));
        }
        if status == 404 || status == 405 || status == 501 {
            return Err(Error::Unsupported(
                "Trae 令牌续期接口不可用，请重新授权".into(),
            ));
        }
        if status >= 400 {
            return Err(Error::Upstream(format!(
                "Trae 令牌续期失败（HTTP {status}）：{}",
                http::msg_of(&body)
            )));
        }
        let tokens = parse_token_response(&body)
            .ok_or_else(|| Error::Upstream("Trae 续期响应里没有新令牌".into()))?;
        if let (Some(old), Some(new)) = (
            cred.platform_uid.as_deref().filter(|s| !s.is_empty()),
            tokens.user_id.as_deref(),
        ) {
            if old != new {
                return Err(Error::Auth("Trae 续期返回了不同的账号身份".into()));
            }
        }
        cred.access_token = Some(tokens.access_token);
        if let Some(rt) = tokens.refresh_token {
            cred.refresh_token = Some(rt);
        }
        if let Some(exp) = tokens.expires_at {
            cred.expires_at = Some(exp);
        }
        if let Some(uid) = tokens.user_id {
            cred.platform_uid = Some(uid);
        }
        Ok(())
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
            token_refresh: true,
            verified: true,
        }
    }

    async fn refresh(&self, cred: &mut Credential) -> Result<()> {
        self.refresh_with_key(cred).await
    }

    async fn identity(&self, cred: &Credential) -> Result<Option<String>> {
        if let Some(uid) = cred
            .platform_uid
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Ok(Some(uid.to_string()));
        }
        // 本机导入的 access token 是 JWT，用户 id 在 `data.id` 里（没有 `sub`）。
        if let Some(uid) = cred.access_token.as_deref().and_then(uid_from_jwt) {
            return Ok(Some(uid));
        }
        // 兜底：标准 `sub`。
        Ok(crate::identity::platform_uid(cred))
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

// ---------------------------------------------------------------- 授权辅助

struct TokenSet {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    user_id: Option<String>,
}

/// Trae 的令牌响应统一包在 `Result` 里（授权码交换与续期同构）。
fn parse_token_response(body: &Value) -> Option<TokenSet> {
    let result = body.get("Result").unwrap_or(body);
    let access_token = http::pick_str(result, &["Token", "token", "access_token"])?;
    let refresh_token = http::pick_str(result, &["RefreshToken", "refresh_token"]);
    let expires_at = match http::pick_i64(result, &["TokenExpireAt", "expires_at"]) {
        Some(v) if v > 0 => Some(crate::oauth::ms_to_secs(v)),
        _ => http::pick_i64(result, &["TokenExpireDuration"])
            .filter(|s| *s > 0)
            .map(|secs| chrono::Local::now().timestamp() + secs),
    };
    Some(TokenSet {
        access_token,
        refresh_token,
        expires_at,
        user_id: http::pick_str(result, &["UserID", "user_id", "userId"]),
    })
}

fn pick_user_field(user_info: &Value, keys: &[&str]) -> Option<String> {
    for root in [
        Some(user_info),
        user_info.get("Result"),
        user_info.get("data"),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(v) = http::pick_str(root, keys) {
            return Some(v);
        }
    }
    None
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string())
}

/// 与客户端同款格式的 16 位数字设备 id。
fn random_device_id() -> Result<String> {
    let bytes = crate::oauth::random_bytes(8)?;
    let mut n = u64::from_le_bytes(bytes.try_into().expect("8 字节"));
    n %= 1_000_000_000_000_000;
    Ok(format!("1{n:015}"))
}

/// 机器码：同一台机器上保持稳定（与客户端算法同构，但加了自己的种子）。
fn stable_machine_id() -> String {
    use sha2::{Digest, Sha256};
    let mut input = String::new();
    for key in ["COMPUTERNAME", "USERNAME", "APPDATA"] {
        input.push_str(&std::env::var(key).unwrap_or_default());
        input.push('|');
    }
    input.push_str("pointhub|trae");
    let digest = Sha256::digest(input.as_bytes());
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn install_roots() -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let base = std::path::PathBuf::from(local).join("Programs");
        for name in ["Trae CN", "Trae"] {
            roots.push(base.join(name));
        }
    }
    if let Some(pf) = std::env::var_os("PROGRAMFILES") {
        let base = std::path::PathBuf::from(pf);
        for name in ["Trae CN", "Trae"] {
            roots.push(base.join(name));
        }
    }
    roots
}

/// 读本机安装的客户端版本；读不到就用兜底值（授权页对版本有下限）。
fn detect_client_version() -> (String, String) {
    for root in install_roots() {
        let path = root.join("resources").join("app").join("product.json");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let app = v.get("appVersion").and_then(Value::as_str).unwrap_or("");
        let tron = v
            .get("tronBuildVersion")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !app.is_empty() {
            return (
                app.to_string(),
                if tron.is_empty() {
                    FALLBACK_TRON_VERSION.to_string()
                } else {
                    tron.to_string()
                },
            );
        }
    }
    (
        FALLBACK_CLIENT_VERSION.to_string(),
        FALLBACK_TRON_VERSION.to_string(),
    )
}

fn generate_keypair() -> Result<(String, String)> {
    use p256::ecdsa::SigningKey;
    use p256::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};

    let bytes = crate::oauth::random_bytes(32)?;
    let key = SigningKey::from_slice(&bytes)
        .map_err(|e| Error::Format(format!("生成 Trae 设备密钥失败：{e}")))?;
    let private = key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| Error::Format(format!("导出私钥失败：{e}")))?
        .to_string();
    let public = key
        .verifying_key()
        .to_public_key_pem(LineEnding::LF)
        .map_err(|e| Error::Format(format!("导出公钥失败：{e}")))?;
    Ok((private, public))
}

fn sign_der_b64(private_pem: &str, data: &[u8]) -> Result<String> {
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::SigningKey;
    use p256::pkcs8::DecodePrivateKey;

    let key = SigningKey::from_pkcs8_pem(private_pem)
        .map_err(|e| Error::Auth(format!("解析 Trae 设备私钥失败：{e}")))?;
    let signature: p256::ecdsa::Signature = key.sign(data);
    use base64::Engine as _;
    Ok(base64::engine::general_purpose::STANDARD.encode(signature.to_der().as_bytes()))
}

fn public_pem_from_private(private_pem: &str) -> Result<String> {
    use p256::ecdsa::SigningKey;
    use p256::pkcs8::{DecodePrivateKey, EncodePublicKey, LineEnding};

    let key = SigningKey::from_pkcs8_pem(private_pem)
        .map_err(|e| Error::Auth(format!("解析 Trae 设备私钥失败：{e}")))?;
    key.verifying_key()
        .to_public_key_pem(LineEnding::LF)
        .map_err(|e| Error::Format(format!("导出公钥失败：{e}")))
}

/// 从 Trae 的 access token（JWT）里取用户 id：`data.id` 优先，其次标准 `sub`。
fn uid_from_jwt(token: &str) -> Option<String> {
    use base64::Engine as _;

    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;

    let from_data = value.get("data").and_then(|data| match data {
        Value::Object(_) => http::pick_str(data, &["id", "userId", "user_id"]),
        Value::String(raw) => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|v| http::pick_str(&v, &["id", "userId", "user_id"])),
        _ => None,
    });
    from_data.or_else(|| http::pick_str(&value, &["sub", "userId", "user_id", "uid"]))
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

    #[test]
    fn auth_url_contains_pkce_device_and_callback() {
        let session = TraeCnProvider::begin_auth("http://127.0.0.1:4567/authorize").unwrap();
        assert!(session.auth_url.starts_with("https://www.trae.cn/authorization?"));
        assert!(session.auth_url.contains("code_challenge_method=S256"));
        assert!(session.auth_url.contains("client_id=ono9krqynydwx5"));
        assert!(session
            .auth_url
            .contains("auth_callback_url=http%3A%2F%2F127.0.0.1%3A4567%2Fauthorize"));
        assert_eq!(session.device_id.len(), 16);
        assert!(session.device_id.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(session.machine_id.len(), 32);
        assert_eq!(session.login_trace.len(), 64);
        assert!(!session.verifier.is_empty());
    }

    #[test]
    fn keypair_signs_and_verifies() {
        use p256::ecdsa::signature::Verifier;
        use p256::ecdsa::{Signature, VerifyingKey};
        use p256::pkcs8::DecodePublicKey;

        let (private, public) = generate_keypair().unwrap();
        assert!(private.contains("BEGIN PRIVATE KEY"));
        assert!(public.contains("BEGIN PUBLIC KEY"));

        let sig_b64 = sign_der_b64(&private, b"POST\n/trae/api/v3/oauth/ExchangeToken").unwrap();
        use base64::Engine as _;
        let der = base64::engine::general_purpose::STANDARD
            .decode(sig_b64)
            .unwrap();
        let signature = Signature::from_der(&der).unwrap();
        let key = VerifyingKey::from_public_key_pem(&public).unwrap();
        key.verify(b"POST\n/trae/api/v3/oauth/ExchangeToken", &signature)
            .expect("签名必须能被公钥验证");
        assert!(key
            .verify(b"POST\n/trae/api/v3/oauth/Other", &signature)
            .is_err());
    }

    #[test]
    fn uid_from_jwt_reads_data_id() {
        use base64::Engine as _;
        let payload = json!({
            "iat": 1,
            "exp": 2,
            "data": { "id": "123456789012345", "type": "user", "source": "trae" }
        });
        let token = format!(
            "h.{}.s",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&payload).unwrap())
        );
        assert_eq!(uid_from_jwt(&token).as_deref(), Some("123456789012345"));
        assert_eq!(uid_from_jwt("not-a-jwt"), None);

        let sub_token = format!(
            "h.{}.s",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(br#"{"sub":"sub-user"}"#)
        );
        assert_eq!(uid_from_jwt(&sub_token).as_deref(), Some("sub-user"));
    }

    #[test]
    fn token_response_parses_result_wrapper() {
        let body = json!({
            "Result": {
                "Token": "jwt-abc",
                "RefreshToken": "rt-xyz",
                "UserID": "user-9",
                "TokenExpireDuration": 3600
            }
        });
        let t = parse_token_response(&body).unwrap();
        assert_eq!(t.access_token, "jwt-abc");
        assert_eq!(t.refresh_token.as_deref(), Some("rt-xyz"));
        assert_eq!(t.user_id.as_deref(), Some("user-9"));
        assert!(t.expires_at.unwrap() > chrono::Local::now().timestamp());
        assert!(parse_token_response(&json!({})).is_none());
    }
}
