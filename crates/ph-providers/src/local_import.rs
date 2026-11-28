//! 本机导入：读取本机已登录客户端的登录态，转成凭据。
//!
//! 边界（`DECISIONS.md` D16 / `docs/adr/0003-local-client-import.md`）：
//! - **只读**：不修改、不备份客户端文件；
//! - 读不到就说清楚为什么，不回退去猜令牌；
//! - 仅 Windows（客户端不在手机上）。
//!
//! 三个平台的登录态位置：
//!
//! | 平台 | 位置 | 形态 |
//! |---|---|---|
//! | WorkBuddy | `~/.workbuddy/checkin-rt.json` | 明文 JSON 里的 `rt`（长效令牌） |
//! | Trae CN | `%APPDATA%\Trae CN\User\globalStorage\storage.json` | `iCubeAuthInfo://icube.cloudide`（AES-128-CBC 或明文 JSON） |
//! | Qoder CN | `%APPDATA%\com.qodercn.app*\auth.v1.dat` | `v10` + AES-256-GCM，密钥在 `Local State`（DPAPI 保护） |
//!
//! Trae / Qoder 取到的是**短期 access token**（有效期数天到一个月），过期后
//! 客户端会自己续期，用户重新点一次「本机导入」即可，不需要手抄令牌。

use crate::client_crypto;
use crate::identity::jwt_payload;
use crate::wb_auth;
use base64::Engine as _;
use chrono::TimeZone as _;
use ph_core::{Credential, Error, Result, Vendor};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 一个平台的本机导入可用性，给界面展示用。
#[derive(Debug, Clone)]
pub struct ImportStatus {
    pub vendor: Vendor,
    pub available: bool,
    /// 登录态已找到，但读取它需要用户允许一次额外的本地操作（WorkBuddy 官方密钥）。
    pub needs_key: bool,
    pub detail: String,
}

/// 从客户端登录态里读出来的凭据。
#[derive(Debug, Clone)]
pub struct LocalCredential {
    pub credential: Credential,
    /// 给人看的来源说明，不含令牌。
    pub detail: String,
}

/// 扫描各平台的本机登录态（只读，不联网）。
pub fn scan() -> Vec<ImportStatus> {
    match home_dir() {
        Some(home) => scan_in(&home),
        None => Vendor::ALL
            .iter()
            .map(|v| ImportStatus {
                vendor: *v,
                available: false,
                needs_key: false,
                detail: "本机导入仅支持 Windows".into(),
            })
            .collect(),
    }
}

/// 读取某平台的本机登录态。
///
/// `allow_key_fetch` 只在用户点过「允许一次」后为 true；扫描阶段永远不启动客户端。
pub fn import(vendor: Vendor, allow_key_fetch: bool) -> Result<LocalCredential> {
    let home = home_dir().ok_or_else(|| Error::Unsupported("本机导入仅支持 Windows".into()))?;
    import_in(vendor, &home, allow_key_fetch)
}

/// 指定用户目录的扫描版本（测试与自定义环境用）。
pub fn scan_in(home: &Path) -> Vec<ImportStatus> {
    Vendor::ALL
        .iter()
        .map(|v| match read_credential(*v, home, false) {
            Ok(found) => ImportStatus {
                vendor: *v,
                available: true,
                needs_key: false,
                detail: found.detail,
            },
            Err(e) => ImportStatus {
                vendor: *v,
                available: false,
                needs_key: matches!(e, Error::NeedsKey(_)),
                detail: e.user_message(),
            },
        })
        .collect()
}

/// 指定用户目录的导入版本。
pub fn import_in(vendor: Vendor, home: &Path, allow_key_fetch: bool) -> Result<LocalCredential> {
    read_credential(vendor, home, allow_key_fetch)
}

fn read_credential(vendor: Vendor, home: &Path, allow_key_fetch: bool) -> Result<LocalCredential> {
    match vendor {
        Vendor::WorkBuddy => workbuddy_credential(home, allow_key_fetch),
        Vendor::TraeCn => trae_credential(home),
        Vendor::QoderCn => qoder_credential(home),
    }
}

// ---------------------------------------------------------------- WorkBuddy

fn workbuddy_credential(home: &Path, allow_key_fetch: bool) -> Result<LocalCredential> {
    let legacy = home.join(".workbuddy").join("checkin-rt.json");
    // 旧版文件存在时优先沿用，避免同一台机器上有两份登录态时行为漂移。
    if legacy.is_file() {
        return workbuddy_legacy_credential(&legacy);
    }
    if let Some(path) = wb_auth::auth_file_path() {
        return workbuddy_v5_credential(&path, allow_key_fetch);
    }
    workbuddy_legacy_credential(&legacy)
}

/// WorkBuddy 5.6+ 共享登录态：优先读 `$wbEncrypted`，必要时才向用户申请取密钥。
fn workbuddy_v5_credential(path: &Path, allow_key_fetch: bool) -> Result<LocalCredential> {
    let key = if allow_key_fetch {
        Some(wb_auth::load_key()?)
    } else {
        None
    };
    let auth = wb_auth::read_auth(path, key.as_ref())?;
    let credential = Credential {
        refresh_token: auth.refresh_token,
        access_token: auth.access_token,
        domain: auth.domain,
        platform_uid: auth.uid,
        expires_at: auth.expires_at,
        ..Default::default()
    };
    let detail = match auth.nickname {
        Some(name) => format!("已找到 WorkBuddy 5.x 登录态（{name}）"),
        None => "已找到 WorkBuddy 5.x 登录态".to_string(),
    };
    Ok(LocalCredential { credential, detail })
}

/// 旧版 `~/.workbuddy/checkin-rt.json`（明文 rt）。
fn workbuddy_legacy_credential(path: &Path) -> Result<LocalCredential> {
    let raw = std::fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::NotFound(format!(
            "没找到 WorkBuddy 客户端的登录态（{}）。请先登录 WorkBuddy 客户端，或改用手动令牌。",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => {
            Error::Auth("没有权限读取 WorkBuddy 客户端的登录态文件，请检查文件权限后重试。".into())
        }
        _ => Error::Io(format!("读取 WorkBuddy 登录态失败：{e}")),
    })?;

    let json: Value = serde_json::from_str(&raw).map_err(|_| {
        Error::Format(
            "WorkBuddy 客户端登录态格式无法解析（客户端可能已升级），请改用手动令牌。".into(),
        )
    })?;
    let rt = json
        .get("rt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Format(
                "WorkBuddy 客户端登录态里没有长效令牌（rt 为空），请打开客户端重新登录后再试。"
                    .into(),
            )
        })?;

    let payload = jwt_payload(rt).ok_or_else(|| {
        Error::Format("WorkBuddy 客户端令牌格式不认识（不是 JWT），请改用手动令牌。".into())
    })?;
    let exp = payload.get("exp").and_then(Value::as_i64);
    if let Some(exp) = exp {
        let now = chrono::Local::now().timestamp();
        if exp <= now {
            return Err(Error::Auth(format!(
                "WorkBuddy 客户端令牌已于 {} 过期，请打开客户端重新登录后再导入。",
                fmt_time(exp)
            )));
        }
    }

    let nickname = payload
        .get("nickname")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let detail = if nickname.is_empty() {
        "已找到 WorkBuddy 客户端登录态".to_string()
    } else {
        format!("已找到 WorkBuddy 客户端登录态（{nickname}）")
    };

    Ok(LocalCredential {
        credential: Credential {
            refresh_token: Some(rt.to_string()),
            ..Default::default()
        },
        detail,
    })
}

// ---------------------------------------------------------------- Trae CN

const TRAE_AUTH_KEY: &str = "iCubeAuthInfo://icube.cloudide";
const TRAE_DC_PREFIX: &str = "iCubeAuthInfo://icube-dc:";
const TRAE_APPS: &[&str] = &["Trae CN", "TRAE SOLO CN", "Trae", "TRAE SOLO"];
/// Trae 登录态密文头 `tc 05 10 00 00`（6 字节）。
const TRAE_HEADER: [u8; 6] = [0x74, 0x63, 0x05, 0x10, 0x00, 0x00];

/// 两段固定盐异或后的派生盐（来自客户端登录态格式，与账号无关）。
const TRAE_SALT_A: [u8; 64] = [
    82, 9, 106, 213, 48, 54, 165, 56, 191, 64, 163, 158, 129, 243, 215, 251, 124, 227, 57, 130,
    155, 47, 255, 135, 52, 142, 67, 68, 196, 222, 233, 203, 84, 123, 148, 50, 166, 194, 35, 61,
    238, 76, 149, 11, 66, 250, 195, 78, 8, 46, 161, 102, 40, 217, 36, 178, 118, 91, 162, 73, 109,
    139, 209, 37,
];
const TRAE_SALT_B: [u8; 64] = [
    31, 221, 168, 51, 136, 7, 199, 49, 177, 18, 16, 89, 39, 128, 236, 95, 96, 81, 127, 169, 25,
    181, 74, 13, 45, 229, 122, 159, 147, 201, 156, 239, 160, 224, 59, 77, 174, 42, 245, 176, 200,
    235, 187, 60, 131, 83, 153, 97, 23, 43, 4, 126, 186, 119, 214, 38, 225, 105, 20, 99, 85, 33,
    12, 125,
];

fn trae_credential(home: &Path) -> Result<LocalCredential> {
    let roaming = roaming_dir(home);
    let mut last_error: Option<Error> = None;

    for app in TRAE_APPS {
        let path = roaming
            .join(app)
            .join("User")
            .join("globalStorage")
            .join("storage.json");
        if !path.exists() {
            continue;
        }
        match read_trae_storage(&path) {
            Ok(found) => return Ok(found),
            Err(e) => last_error = Some(e),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        Error::NotFound(
            "没找到 Trae CN 客户端的登录态。请先打开并登录 Trae CN，或改用手动令牌（需填设备 ID）。"
                .into(),
        )
    }))
}

fn read_trae_storage(path: &Path) -> Result<LocalCredential> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| Error::Io(format!("读取 Trae 登录态失败：{e}")))?;
    let storage: Value = serde_json::from_str(&raw)
        .map_err(|_| Error::Format("Trae 的 storage.json 无法解析（客户端可能已升级）".into()))?;
    let Some(enc) = storage.get(TRAE_AUTH_KEY).and_then(Value::as_str) else {
        return Err(Error::Format(
            "Trae 客户端当前未登录（storage.json 里没有登录态）。请打开客户端登录后再试。".into(),
        ));
    };

    let auth: Value = serde_json::from_str(&decode_trae_auth(enc)?)
        .map_err(|_| Error::Format("Trae 登录态解密后不是合法 JSON".into()))?;
    let token = auth
        .get("token")
        .or_else(|| auth.get("accessToken"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Auth("Trae 登录态里没有 token，请打开客户端重新登录".into()))?;

    let device_id = storage
        .as_object()
        .and_then(|obj| {
            obj.keys()
                .find(|k| k.starts_with(TRAE_DC_PREFIX))
                .map(|k| k[TRAE_DC_PREFIX.len()..].to_string())
        })
        .or_else(|| {
            storage
                .get("telemetry.devDeviceId")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();

    let expires_at = parse_time(auth.get("expiredAt"));
    if let Some(exp) = expires_at {
        if exp <= chrono::Local::now().timestamp() {
            return Err(Error::Auth(format!(
                "Trae 登录令牌已于 {} 过期，请打开 Trae CN 客户端重新登录后再导入。",
                fmt_time(exp)
            )));
        }
    }

    let mut credential = Credential::from_access_token(token);
    if !device_id.trim().is_empty() {
        credential.device_id = Some(device_id);
    }
    credential.expires_at = expires_at;
    // 客户端登录态里带 userId；用它和浏览器授权导入的账号对齐，避免同一账号两条记录。
    credential.platform_uid = pick_uid(&auth, &["userId", "user_id", "uid"]);

    Ok(LocalCredential {
        detail: match expires_at {
            Some(exp) => format!(
                "已找到 Trae CN 客户端登录态（令牌有效期至 {}）",
                fmt_time(exp)
            ),
            None => "已找到 Trae CN 客户端登录态".into(),
        },
        credential,
    })
}

/// Trae 的登录态可能是明文 JSON，也可能是 `tc 05 10 00 00` 开头的 AES-128-CBC 密文。
fn decode_trae_auth(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        return Ok(trimmed.to_string());
    }

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|_| Error::Format("Trae 登录态不是有效的 Base64".into()))?;
    if bytes.len() <= 102 {
        return Err(Error::Format("Trae 登录态密文过短".into()));
    }
    if bytes[..6] != TRAE_HEADER {
        return Err(Error::Format("不支持的 Trae 登录态加密格式".into()));
    }

    let random = &bytes[6..38];
    let cipher = &bytes[38..];

    let mut salt = [0u8; 64];
    for i in 0..64 {
        salt[i] = TRAE_SALT_A[i] ^ TRAE_SALT_B[i];
    }
    let mut seed = Vec::with_capacity(128);
    seed.extend_from_slice(&client_crypto::sha512(random));
    seed.extend_from_slice(&salt);
    let derived = client_crypto::sha512(&seed);

    let plain = client_crypto::aes128_cbc_decrypt_pkcs7(&derived[..16], &derived[16..32], cipher)?;
    if plain.len() < 64 {
        return Err(Error::Format("Trae 登录态明文过短".into()));
    }
    let (expected, body) = plain.split_at(64);
    if expected != client_crypto::sha512(body) {
        return Err(Error::Auth(
            "Trae 登录态完整性校验失败（客户端可能正在写入或格式已变化），请稍后重试".into(),
        ));
    }
    String::from_utf8(body.to_vec()).map_err(|_| Error::Format("Trae 登录态不是 UTF-8 文本".into()))
}

// ---------------------------------------------------------------- Qoder CN

const QODER_AUTH_FILE: &str = "auth.v1.dat";
const QODER_LOCAL_STATE: &str = "Local State";

fn qoder_credential(home: &Path) -> Result<LocalCredential> {
    let root = qoder_root(home).ok_or_else(|| {
        Error::NotFound(
            "没找到 Qoder CN 客户端的登录态。请先打开并登录 Qoder CN，或改用手动令牌。".into(),
        )
    })?;

    let state_raw = std::fs::read(root.join(QODER_LOCAL_STATE))
        .map_err(|e| Error::Io(format!("读取 Qoder 的 Local State 失败：{e}")))?;
    let state: Value = serde_json::from_slice(&state_raw)
        .map_err(|_| Error::Format("Qoder 的 Local State 无法解析".into()))?;
    let encrypted_key = state
        .pointer("/os_crypt/encrypted_key")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Format("Qoder 的 Local State 缺少 os_crypt.encrypted_key".into()))?;
    let key_blob = base64::engine::general_purpose::STANDARD
        .decode(encrypted_key)
        .map_err(|_| Error::Format("Qoder 的登录态密钥不是有效 Base64".into()))?;
    let key_blob = key_blob
        .strip_prefix(b"DPAPI".as_slice())
        .ok_or_else(|| Error::Format("Qoder 的登录态密钥格式已变化".into()))?;
    let key = client_crypto::dpapi_unprotect(key_blob).map_err(|_| {
        Error::Auth("解不开 Qoder 登录态（需要同一 Windows 用户），请重新登录客户端".into())
    })?;

    let raw = std::fs::read(root.join(QODER_AUTH_FILE))
        .map_err(|e| Error::Io(format!("读取 Qoder 的登录态失败：{e}")))?;
    let session: Value = serde_json::from_slice(&decrypt_qoder_v10(&key, &raw)?)
        .map_err(|_| Error::Format("Qoder 登录态解密后不是合法 JSON".into()))?;
    let token = session
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Auth("Qoder 登录态里没有 token，请打开客户端重新登录".into()))?;

    let expires_at = parse_time(session.get("expiresAt"));
    if let Some(exp) = expires_at {
        if exp <= chrono::Local::now().timestamp() + 60 {
            return Err(Error::Auth(format!(
                "Qoder 登录态已于 {} 过期，请打开 Qoder CN 客户端重新登录后再导入。",
                fmt_time(exp)
            )));
        }
    }

    let mut credential = Credential::from_access_token(token);
    credential.expires_at = expires_at;
    // `auth.v1.dat` 里若带用户 id 就直接用；没有的话导入验证时由适配器走 userinfo 补。
    credential.platform_uid = pick_uid(&session, &["user_id", "userId", "uid"]);

    Ok(LocalCredential {
        detail: match expires_at {
            Some(exp) => format!(
                "已找到 Qoder CN 客户端登录态（令牌有效期至 {}）",
                fmt_time(exp)
            ),
            None => "已找到 Qoder CN 客户端登录态".into(),
        },
        credential,
    })
}

fn qoder_root(home: &Path) -> Option<PathBuf> {
    let roaming = roaming_dir(home);
    let primary = roaming.join("com.qodercn.app.stable");
    if primary.join(QODER_AUTH_FILE).exists() {
        return Some(primary);
    }
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let entries = std::fs::read_dir(&roaming).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("com.qodercn.app") {
            continue;
        }
        let path = entry.path();
        if !path.join(QODER_AUTH_FILE).exists() {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().map_or(true, |(t, _)| mtime > *t) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, p)| p)
}

/// `auth.v1.dat` = `v10` + 12 字节 nonce + (密文 + 16 字节 GCM 标签)。
fn decrypt_qoder_v10(key: &[u8], raw: &[u8]) -> Result<Vec<u8>> {
    if raw.len() < 3 + 12 + 16 + 2 {
        return Err(Error::Format("Qoder 登录态文件过短".into()));
    }
    if &raw[..3] != b"v10" {
        return Err(Error::Format("不支持的 Qoder 登录态格式".into()));
    }
    client_crypto::aes256_gcm_decrypt(key, &raw[3..15], &raw[15..])
}

// ---------------------------------------------------------------- 路径与时间

fn roaming_dir(home: &Path) -> PathBuf {
    if let (Some(appdata), Some(profile)) =
        (std::env::var_os("APPDATA"), std::env::var_os("USERPROFILE"))
    {
        if PathBuf::from(profile) == home {
            return PathBuf::from(appdata);
        }
    }
    home.join("AppData").join("Roaming")
}

/// 解析 ISO 时间或 epoch 秒；解析不了就返回 None，不猜。
fn parse_time(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    if let Some(n) = v.as_i64() {
        // 毫秒时间戳也兼容一下（> 1e12 视为毫秒）。
        return Some(if n > 1_000_000_000_000 { n / 1000 } else { n });
    }
    let s = v.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp());
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return chrono::Local
                .from_local_datetime(&ndt)
                .single()
                .map(|d| d.timestamp());
        }
    }
    None
}

fn fmt_time(epoch: i64) -> String {
    chrono::Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| epoch.to_string())
}

/// 从客户端 JSON 里取平台用户 id（数字也接受）。
fn pick_uid(root: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(v) = root.get(*key) else { continue };
        let text = match v {
            Value::String(s) => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            _ => continue,
        };
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home(tag: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ph-localimport-{tag}-{n}"));
        std::fs::create_dir_all(dir.join(".workbuddy")).unwrap();
        dir
    }

    fn jwt(payload: Value) -> String {
        let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.sig",
            enc(br#"{"alg":"RS256","typ":"JWT"}"#),
            enc(serde_json::to_string(&payload).unwrap().as_bytes())
        )
    }

    fn write_checkin(home: &Path, rt: &str) {
        std::fs::write(
            home.join(".workbuddy").join("checkin-rt.json"),
            serde_json::json!({ "v": 1, "rt": rt, "source": "refresh" }).to_string(),
        )
        .unwrap();
    }

    // ---------------------------------------------------------- Trae 夹具

    fn write_trae_storage(home: &Path, auth_value: &str, device_key: &str) {
        let dir = roaming_dir(home)
            .join("Trae CN")
            .join("User")
            .join("globalStorage");
        std::fs::create_dir_all(&dir).unwrap();
        let storage = serde_json::json!({
            TRAE_AUTH_KEY: auth_value,
            device_key: "opaque",
            "telemetry.devDeviceId": "fallback-device"
        });
        std::fs::write(dir.join("storage.json"), storage.to_string()).unwrap();
    }

    fn encrypt_trae_auth(plain: &str) -> String {
        let mut salt = [0u8; 64];
        for i in 0..64 {
            salt[i] = TRAE_SALT_A[i] ^ TRAE_SALT_B[i];
        }
        let mut random = [0u8; 32];
        for (i, b) in random.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(3);
        }
        let mut seed = Vec::new();
        seed.extend_from_slice(&client_crypto::sha512(&random));
        seed.extend_from_slice(&salt);
        let derived = client_crypto::sha512(&seed);

        let mut body = Vec::new();
        body.extend_from_slice(&client_crypto::sha512(plain.as_bytes()));
        body.extend_from_slice(plain.as_bytes());
        let ct = client_crypto::aes128_cbc_encrypt_pkcs7(&derived[..16], &derived[16..32], &body);

        let mut out = Vec::new();
        out.extend_from_slice(&TRAE_HEADER);
        out.extend_from_slice(&random);
        out.extend_from_slice(&ct);
        base64::engine::general_purpose::STANDARD.encode(out)
    }

    // ---------------------------------------------------------- Qoder 夹具

    #[cfg(windows)]
    fn write_qoder_store(home: &Path, key: &[u8], session: &Value) {
        let dir = roaming_dir(home).join("com.qodercn.app.stable");
        std::fs::create_dir_all(&dir).unwrap();

        let mut sealed_key = b"DPAPI".to_vec();
        sealed_key.extend(client_crypto::dpapi_protect(key).unwrap());
        let state = serde_json::json!({
            "os_crypt": {
                "encrypted_key": base64::engine::general_purpose::STANDARD.encode(sealed_key)
            }
        });
        std::fs::write(dir.join(QODER_LOCAL_STATE), state.to_string()).unwrap();

        let nonce = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let plain = serde_json::to_vec(session).unwrap();
        let sealed = client_crypto::aes256_gcm_encrypt(key, &nonce, &plain);
        let mut raw = b"v10".to_vec();
        raw.extend_from_slice(&nonce);
        raw.extend_from_slice(&sealed);
        std::fs::write(dir.join(QODER_AUTH_FILE), raw).unwrap();
    }

    // ---------------------------------------------------------- 测试

    #[test]
    fn reads_workbuddy_refresh_token() {
        let home = temp_home("ok");
        let now = chrono::Local::now().timestamp();
        let token = jwt(serde_json::json!({ "exp": now + 3600, "nickname": "测试号" }));
        write_checkin(&home, &token);

        let found = import_in(Vendor::WorkBuddy, &home, false).unwrap();
        assert_eq!(
            found.credential.refresh_token.as_deref(),
            Some(token.as_str())
        );
        assert!(found.credential.access_token.is_none());
        assert!(found.detail.contains("测试号"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn rejects_expired_token() {
        let home = temp_home("expired");
        let now = chrono::Local::now().timestamp();
        write_checkin(&home, &jwt(serde_json::json!({ "exp": now - 60 })));

        let err = import_in(Vendor::WorkBuddy, &home, false).unwrap_err();
        assert!(err.user_message().contains("过期"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn missing_file_points_to_manual_token() {
        let home = temp_home("missing");
        // 直接测旧版路径的缺失提示；不要受本机真实 WorkBuddy 5.x 登录态影响。
        let legacy = home.join(".workbuddy").join("checkin-rt.json");
        let err = workbuddy_legacy_credential(&legacy).unwrap_err();
        assert!(err.user_message().contains("手动令牌"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn malformed_rt_is_rejected_not_guessed() {
        let home = temp_home("bad");
        write_checkin(&home, "not-a-jwt");
        let err = import_in(Vendor::WorkBuddy, &home, false).unwrap_err();
        assert!(err.user_message().contains("JWT"));
        let _ = std::fs::remove_dir_all(&home);
    }

    fn write_v5_auth(dir: &Path, value: &Value) -> PathBuf {
        let path = dir.join("workbuddy-desktop.info");
        std::fs::write(&path, value.to_string()).unwrap();
        path
    }

    #[test]
    fn reads_workbuddy_v5_plaintext_auth() {
        let home = temp_home("wb5-plain");
        let path = write_v5_auth(
            &home,
            &serde_json::json!({
                "auth": {
                    "refreshToken": "v5-refresh",
                    "accessToken": "v5-access",
                    "domain": "www.codebuddy.cn",
                    "expiresAt": 1_893_456_000_000i64
                },
                "account": { "uid": "uid-v5", "nickname": "测试号" }
            }),
        );

        let found = workbuddy_v5_credential(&path, false).unwrap();
        assert_eq!(
            found.credential.refresh_token.as_deref(),
            Some("v5-refresh")
        );
        assert_eq!(found.credential.access_token.as_deref(), Some("v5-access"));
        assert_eq!(found.credential.platform_uid.as_deref(), Some("uid-v5"));
        assert_eq!(found.credential.domain.as_deref(), Some("www.codebuddy.cn"));
        assert_eq!(found.credential.expires_at, Some(1_893_456_000));
        assert!(found.detail.contains("测试号"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn workbuddy_v5_encrypted_tokens_need_user_permission() {
        let home = temp_home("wb5-enc");
        let path = write_v5_auth(
            &home,
            &serde_json::json!({
                "auth": {
                    "refreshToken": { "$wbEncrypted": 1, "envelope": "e30=" }
                },
                "account": { "uid": "uid-v5" }
            }),
        );

        let err = workbuddy_v5_credential(&path, false).unwrap_err();
        assert!(matches!(err, Error::NeedsKey(_)), "{err:?}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn scan_marks_only_importable_vendors_available() {
        let home = temp_home("scan");
        let now = chrono::Local::now().timestamp();
        write_checkin(&home, &jwt(serde_json::json!({ "exp": now + 3600 })));

        let statuses = scan_in(&home);
        let wb = statuses
            .iter()
            .find(|s| s.vendor == Vendor::WorkBuddy)
            .unwrap();
        assert!(wb.available, "找到客户端登录态时应可用：{}", wb.detail);
        for v in [Vendor::TraeCn, Vendor::QoderCn] {
            let s = statuses.iter().find(|s| s.vendor == v).unwrap();
            assert!(!s.available, "{} 没登录态时应标为不可用", v.display());
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn reads_trae_plaintext_auth_and_device_id() {
        let home = temp_home("trae-plain");
        let auth = serde_json::json!({
            "token": "trae-token",
            "userId": "trae-user-9",
            "expiredAt": "2099-01-01T00:00:00+08:00"
        })
        .to_string();
        write_trae_storage(&home, &auth, "iCubeAuthInfo://icube-dc:device-123");

        let found = import_in(Vendor::TraeCn, &home, false).unwrap();
        assert_eq!(found.credential.access_token.as_deref(), Some("trae-token"));
        assert_eq!(found.credential.device_id.as_deref(), Some("device-123"));
        assert_eq!(
            found.credential.platform_uid.as_deref(),
            Some("trae-user-9")
        );
        assert!(found.credential.expires_at.is_some());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn reads_trae_encrypted_auth() {
        let home = temp_home("trae-enc");
        let auth = serde_json::json!({
            "token": "trae-encrypted-token",
            "expiredAt": "2099-01-01T00:00:00+08:00"
        })
        .to_string();
        let sealed = encrypt_trae_auth(&auth);
        write_trae_storage(&home, &sealed, "iCubeAuthInfo://icube-dc:dev-xyz");

        let found = import_in(Vendor::TraeCn, &home, false).unwrap();
        assert_eq!(
            found.credential.access_token.as_deref(),
            Some("trae-encrypted-token")
        );
        assert_eq!(found.credential.device_id.as_deref(), Some("dev-xyz"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn trae_expired_token_is_rejected() {
        let home = temp_home("trae-expired");
        let auth = serde_json::json!({
            "token": "old",
            "expiredAt": "2000-01-01T00:00:00+08:00"
        })
        .to_string();
        write_trae_storage(&home, &auth, "iCubeAuthInfo://icube-dc:dev");

        let err = import_in(Vendor::TraeCn, &home, false).unwrap_err();
        assert!(
            err.user_message().contains("过期"),
            "{}",
            err.user_message()
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(windows)]
    #[test]
    fn reads_qoder_dpapi_gcm_store() {
        let home = temp_home("qoder-ok");
        let key = [42u8; 32];
        let session = serde_json::json!({
            "token": "qoder-token",
            "user_id": "qoder-user-7",
            "expiresAt": "2099-01-01T00:00:00+08:00",
            "refreshTokenExpiresAt": "2099-02-01T00:00:00+08:00"
        });
        write_qoder_store(&home, &key, &session);

        let found = import_in(Vendor::QoderCn, &home, false).unwrap();
        assert_eq!(
            found.credential.access_token.as_deref(),
            Some("qoder-token")
        );
        assert_eq!(
            found.credential.platform_uid.as_deref(),
            Some("qoder-user-7")
        );
        assert!(found.credential.expires_at.is_some());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(windows)]
    #[test]
    fn qoder_expired_token_is_rejected() {
        let home = temp_home("qoder-expired");
        let key = [7u8; 32];
        let session = serde_json::json!({
            "token": "qoder-old",
            "expiresAt": "2000-01-01T00:00:00+08:00"
        });
        write_qoder_store(&home, &key, &session);

        let err = import_in(Vendor::QoderCn, &home, false).unwrap_err();
        assert!(
            err.user_message().contains("过期"),
            "{}",
            err.user_message()
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn parse_time_accepts_iso_and_epoch() {
        assert!(parse_time(Some(&serde_json::json!(1_700_000_000))).is_some());
        assert!(parse_time(Some(&serde_json::json!(1_700_000_000_000i64))).is_some());
        assert!(parse_time(Some(&serde_json::json!("2026-12-04T17:36:00+08:00"))).is_some());
        assert!(parse_time(Some(&serde_json::json!("2026-12-04T17:36:00"))).is_some());
        assert!(parse_time(Some(&serde_json::json!("not-a-time"))).is_none());
    }
}
