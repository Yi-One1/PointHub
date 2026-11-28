//! WorkBuddy 5.6+ 共享登录态（`$wbEncrypted` 信封）。
//!
//! 新版客户端把 access/refresh token 与昵称放进
//! `%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info`，
//! 敏感字段是 `{"$wbEncrypted":1,"envelope":"<base64>"}`：
//! envelope 解出 `{suite,keyId,nonce,authTag,ciphertext}`，AES-256-GCM。
//!
//! 密钥不在文件里，而在客户端 native binding（`electron_browser_workbuddy_storage`）。
//! 本模块只在用户明确允许时，短暂以 `ELECTRON_RUN_AS_NODE=1` 启动一次客户端 exe
//! 取密钥；不写客户端文件、不联网、不打开窗口。

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use ph_core::{Error, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 给界面识别的“需要用户授权取密钥”标记。
pub const NEEDS_KEY: &str = "已找到 WorkBuddy 5.x 登录态，但需要读取客户端官方密钥才能导入。";

const AUTH_DIR_REL: &str = "CodeBuddyExtension/Data/Public/auth";
const AUTH_FILES: [&str; 2] = ["workbuddy-desktop.info", "workbuddy-desktop-ai.info"];
const KEY_TTL: Duration = Duration::from_secs(60);
const KEY_TIMEOUT: Duration = Duration::from_secs(10);

/// 读取 at-rest 密钥的 Node 片段。只调用客户端自己的 binding，不碰网络与文件。
const KEY_SCRIPT: &str = r#"
const crypto = require("crypto");
try {
  const raw = process._linkedBinding("electron_browser_workbuddy_storage").loggerGet();
  const info = JSON.parse(raw);
  const secret = info && info.version === 1 ? info.atRestSecretKey : "";
  if (typeof secret !== "string" || secret.length === 0) process.exit(2);
  const bytes = Buffer.from(secret, "base64");
  if (bytes.length !== 32) process.exit(2);
  const digest = crypto.createHash("sha256").update(secret, "utf8").digest();
  process.stdout.write(digest.toString("base64"));
  digest.fill(0);
  bytes.fill(0);
} catch (_) {
  process.exit(2);
}
"#;

/// 官方 AES-256-GCM 密钥。
#[derive(Clone)]
pub struct WbKey {
    bytes: [u8; 32],
    /// keyId = sha256(key) 前 16 位 hex；与 envelope 里的 keyId 不一致说明客户端换钥。
    id: String,
}

impl WbKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        let id = format!("{:x}", Sha256::digest(bytes))[..16].to_owned();
        Self { bytes, id }
    }

    /// 官方客户端的 AAD 帧（协议事实，必须逐字节一致）。
    fn aad(&self) -> Vec<u8> {
        let mut out = b"WB-AAD\0\x01".to_vec();
        for label in ["WBEV1", "sym-v1"] {
            out.extend_from_slice(&(label.len() as u32).to_be_bytes());
            out.extend_from_slice(label.as_bytes());
        }
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&(self.id.len() as u32).to_be_bytes());
        out.extend_from_slice(self.id.as_bytes());
        out.extend_from_slice(&[2, 0, 0]);
        out
    }

    /// 解一个 `$wbEncrypted` 信封。
    pub fn open(&self, wrapper: &Value) -> Result<String> {
        let obj = wrapper
            .as_object()
            .ok_or_else(|| Error::Format("WorkBuddy 加密字段形态不对".into()))?;
        if obj.len() != 2 || obj.get("$wbEncrypted") != Some(&Value::from(1)) {
            return Err(Error::Format("WorkBuddy 加密字段版本不受支持".into()));
        }
        let encoded = obj
            .get("envelope")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Format("WorkBuddy 加密字段缺 envelope".into()))?;
        let envelope: Value = serde_json::from_slice(&decode_b64_strict(encoded)?)
            .map_err(|_| Error::Format("WorkBuddy 加密信封解析失败".into()))?;
        let suite = envelope.get("suite").and_then(Value::as_u64).unwrap_or(0);
        if suite != 1 {
            return Err(Error::Format(format!(
                "WorkBuddy 加密 suite 不受支持：{suite}"
            )));
        }
        let key_id = envelope
            .get("keyId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if key_id != self.id {
            return Err(Error::NeedsKey(
                "客户端密钥已变化，请重新授权读取一次。".into(),
            ));
        }
        let nonce = decode_b64_strict(
            envelope
                .get("nonce")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Format("WorkBuddy 加密信封缺 nonce".into()))?,
        )?;
        let tag = decode_b64_strict(
            envelope
                .get("authTag")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Format("WorkBuddy 加密信封缺 authTag".into()))?,
        )?;
        if nonce.len() != 12 || tag.len() != 16 {
            return Err(Error::Format("WorkBuddy 加密 nonce/tag 长度不对".into()));
        }
        let mut ciphertext = decode_b64_strict(
            envelope
                .get("ciphertext")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Format("WorkBuddy 加密信封缺 ciphertext".into()))?,
        )?;
        ciphertext.extend_from_slice(&tag);
        let cipher = Aes256Gcm::new_from_slice(&self.bytes)
            .map_err(|_| Error::Format("WorkBuddy 密钥长度不对".into()))?;
        let plain = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &self.aad(),
                },
            )
            .map_err(|_| Error::NeedsKey("WorkBuddy 加密字段校验失败，请重新读取密钥。".into()))?;
        String::from_utf8(plain).map_err(|_| Error::Format("WorkBuddy 加密字段解出非 UTF-8".into()))
    }
}

fn decode_b64_strict(encoded: &str) -> Result<Vec<u8>> {
    if encoded.len() > 16 * 1024 * 1024 {
        return Err(Error::Format("WorkBuddy 加密字段超长".into()));
    }
    let bytes = B64
        .decode(encoded)
        .map_err(|_| Error::Format("WorkBuddy 加密字段不是合法 base64".into()))?;
    if B64.encode(&bytes) != encoded {
        return Err(Error::Format(
            "WorkBuddy 加密字段 base64 不是规范形式".into(),
        ));
    }
    Ok(bytes)
}

/// 新版共享登录态里的可读字段。
#[derive(Debug, Clone, Default)]
pub struct WbAuth {
    pub refresh_token: Option<String>,
    pub access_token: Option<String>,
    pub domain: Option<String>,
    pub uid: Option<String>,
    pub nickname: Option<String>,
    pub expires_at: Option<i64>,
}

/// 找共享 auth 文件；没有 `CodeBuddyExtension` 目录时返回 None。
pub fn auth_file_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let dir = PathBuf::from(local).join(AUTH_DIR_REL);
    for name in AUTH_FILES {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn read_auth_file(path: &Path) -> Result<Value> {
    let raw = std::fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::NotFound(format!(
            "没找到 WorkBuddy 5.x 登录态（{}）。请先登录客户端。",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => {
            Error::Auth("没有权限读取 WorkBuddy 客户端登录态，请检查文件权限后重试。".into())
        }
        _ => Error::Io(format!("读取 WorkBuddy 登录态失败：{e}")),
    })?;
    serde_json::from_str(&raw)
        .map_err(|_| Error::Format("WorkBuddy 5.x 登录态格式无法解析。".into()))
}

fn read_string(
    slot: Option<&Value>,
    key: Option<&WbKey>,
    allow_missing_key: bool,
) -> Result<Option<String>> {
    let Some(value) = slot else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::String(s) => {
            let s = s.trim();
            Ok((!s.is_empty()).then(|| s.to_string()))
        }
        Value::Object(obj) if obj.get("$wbEncrypted").is_some() => {
            let Some(key) = key else {
                if allow_missing_key {
                    return Ok(None);
                }
                return Err(Error::NeedsKey(NEEDS_KEY.into()));
            };
            let plain = key.open(value)?;
            let plain = plain.trim();
            Ok((!plain.is_empty()).then(|| plain.to_string()))
        }
        _ => Err(Error::Format("WorkBuddy 登录态字段形态不认识。".into())),
    }
}

/// 解析 auth JSON。`key` 为 None 时，遇到加密的 token 会返回 [`Error::NeedsKey`]。
pub fn parse_auth(value: &Value, key: Option<&WbKey>) -> Result<WbAuth> {
    let auth = value
        .get("auth")
        .ok_or_else(|| Error::Format("WorkBuddy 登录态里没有 auth 段。".into()))?;
    let account = value.get("account");
    let refresh_token = read_string(auth.get("refreshToken"), key, false)?;
    let access_token = read_string(auth.get("accessToken"), key, false)?;
    if refresh_token.is_none() && access_token.is_none() {
        return Err(Error::Format(
            "WorkBuddy 登录态里没有可用的 refresh/access token。".into(),
        ));
    }
    let nickname = account
        .map(|a| read_string(a.get("nickname"), key, true))
        .transpose()?
        .flatten();
    let uid = account
        .and_then(|a| a.get("uid"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let domain = auth
        .get("domain")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    // 客户端写的是毫秒时间戳；兼容秒级。
    let expires_at = auth.get("expiresAt").and_then(Value::as_i64).map(|ts| {
        if ts > 10_000_000_000 {
            ts / 1000
        } else {
            ts
        }
    });
    Ok(WbAuth {
        refresh_token,
        access_token,
        domain,
        uid,
        nickname,
        expires_at,
    })
}

pub fn read_auth(path: &Path, key: Option<&WbKey>) -> Result<WbAuth> {
    let raw = read_auth_file(path)?;
    parse_auth(&raw, key)
}

static KEY_CACHE: Mutex<Option<(WbKey, Instant)>> = Mutex::new(None);

pub fn invalidate_key() {
    if let Ok(mut cache) = KEY_CACHE.lock() {
        *cache = None;
    }
}

/// 取官方密钥；60 秒内复用，避免频繁起辅助进程。
pub fn load_key() -> Result<WbKey> {
    if let Ok(cache) = KEY_CACHE.lock() {
        if let Some((key, at)) = &*cache {
            if at.elapsed() < KEY_TTL {
                return Ok(key.clone());
            }
        }
    }
    let exe = find_client_exe().ok_or_else(|| {
        Error::NotFound("没有找到 WorkBuddy 客户端 exe，无法读取官方加密密钥。".into())
    })?;
    let key = load_key_for_exe(&exe)?;
    if let Ok(mut cache) = KEY_CACHE.lock() {
        *cache = Some((key.clone(), Instant::now()));
    }
    Ok(key)
}

/// 只启动客户端 exe 的 Node 模式取密钥，不打开 GUI。
pub fn load_key_for_exe(exe: &Path) -> Result<WbKey> {
    let mut command = std::process::Command::new(exe);
    command
        .env("ELECTRON_RUN_AS_NODE", "1")
        .env_remove("NODE_OPTIONS")
        .args(["-e", KEY_SCRIPT])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| Error::Io(format!("启动 WorkBuddy 密钥辅助进程失败：{e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Io("密钥辅助进程没有可读输出。".into()))?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let mut stdout = stdout;
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let output = match rx.recv_timeout(KEY_TIMEOUT) {
        Ok(buf) => buf,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Io(
                "读取 WorkBuddy 官方密钥超时，请稍后重试。".into(),
            ));
        }
    };
    let status = child
        .wait()
        .map_err(|e| Error::Io(format!("等待密钥辅助进程失败：{e}")))?;
    if !status.success() {
        return Err(Error::NeedsKey(
            "当前 WorkBuddy 客户端没有提供官方密钥接口，可能版本已变更。".into(),
        ));
    }
    if output.len() != 44 {
        return Err(Error::Format(format!(
            "WorkBuddy 官方密钥输出长度不对（{} 字节）。",
            output.len()
        )));
    }
    let encoded = String::from_utf8(output)
        .map_err(|_| Error::Format("WorkBuddy 官方密钥不是 UTF-8。".into()))?;
    let decoded = decode_b64_strict(encoded.trim())?;
    let bytes = <[u8; 32]>::try_from(decoded.as_slice())
        .map_err(|_| Error::Format("WorkBuddy 官方密钥长度不是 32 字节。".into()))?;
    Ok(WbKey::new(bytes))
}

/// 在常见安装位置找 `WorkBuddy.exe`；只做浅层扫描，不遍历整盘。
pub fn find_client_exe() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("WORKBUDDY_EXE") {
        let p = PathBuf::from(explicit);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    for key in ["LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)"] {
        if let Some(base) = std::env::var_os(key) {
            let base = PathBuf::from(base);
            roots.push(base.join("Programs"));
            roots.push(base.join("WorkBuddy"));
            roots.push(base.clone());
        }
    }
    roots.push(PathBuf::from(r"D:\APP"));
    roots.push(PathBuf::from(r"C:\APP"));
    for root in roots {
        let direct = root.join("WorkBuddy.exe");
        if direct.is_file() {
            return Some(direct);
        }
        let nested = root.join("WorkBuddy").join("WorkBuddy.exe");
        if nested.is_file() {
            return Some(nested);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> WbKey {
        WbKey::new([7u8; 32])
    }

    fn seal(key: &WbKey, nonce: [u8; 12], plain: &str) -> Value {
        let cipher = Aes256Gcm::new_from_slice(&key.bytes).unwrap();
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plain.as_bytes(),
                    aad: &key.aad(),
                },
            )
            .unwrap();
        let split = sealed.len() - 16;
        let envelope = serde_json::json!({
            "suite": 1,
            "keyId": key.id,
            "nonce": B64.encode(nonce),
            "authTag": B64.encode(&sealed[split..]),
            "ciphertext": B64.encode(&sealed[..split]),
        });
        serde_json::json!({
            "$wbEncrypted": 1,
            "envelope": B64.encode(serde_json::to_vec(&envelope).unwrap()),
        })
    }

    #[test]
    fn opens_encrypted_auth_with_key() {
        let k = key();
        let auth = serde_json::json!({
            "auth": {
                "refreshToken": seal(&k, [1u8; 12], "refresh-1"),
                "accessToken": seal(&k, [2u8; 12], "access-1"),
                "domain": "www.codebuddy.cn",
                "expiresAt": 1_893_456_000_000i64,
            },
            "account": {
                "uid": "uid-1",
                "nickname": seal(&k, [3u8; 12], "毅One"),
            }
        });
        let parsed = parse_auth(&auth, Some(&k)).unwrap();
        assert_eq!(parsed.refresh_token.as_deref(), Some("refresh-1"));
        assert_eq!(parsed.access_token.as_deref(), Some("access-1"));
        assert_eq!(parsed.uid.as_deref(), Some("uid-1"));
        assert_eq!(parsed.nickname.as_deref(), Some("毅One"));
        assert_eq!(parsed.domain.as_deref(), Some("www.codebuddy.cn"));
        assert_eq!(parsed.expires_at, Some(1_893_456_000));
    }

    #[test]
    fn encrypted_auth_without_key_reports_needs_key() {
        let k = key();
        let auth = serde_json::json!({
            "auth": { "refreshToken": seal(&k, [4u8; 12], "r") },
            "account": { "uid": "u" }
        });
        let err = parse_auth(&auth, None).unwrap_err();
        assert!(matches!(err, Error::NeedsKey(_)), "{err:?}");
    }

    #[test]
    fn plaintext_auth_is_read_without_key() {
        let auth = serde_json::json!({
            "auth": {
                "refreshToken": "plain-refresh",
                "accessToken": "plain-access",
                "domain": "www.codebuddy.cn"
            },
            "account": { "uid": "uid-9", "nickname": "测试号" }
        });
        let parsed = parse_auth(&auth, None).unwrap();
        assert_eq!(parsed.refresh_token.as_deref(), Some("plain-refresh"));
        assert_eq!(parsed.access_token.as_deref(), Some("plain-access"));
        assert_eq!(parsed.nickname.as_deref(), Some("测试号"));
    }

    #[test]
    fn wrong_key_is_rejected() {
        let k = key();
        let auth = serde_json::json!({
            "auth": { "refreshToken": seal(&k, [5u8; 12], "r") }
        });
        let other = WbKey::new([8u8; 32]);
        let err = parse_auth(&auth, Some(&other)).unwrap_err();
        assert!(matches!(err, Error::NeedsKey(_)), "{err:?}");
    }
}
