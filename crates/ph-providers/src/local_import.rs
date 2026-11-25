//! 本机导入：读取本机已登录客户端的登录态，转成凭据。
//!
//! 边界（`DECISIONS.md` D16 / `docs/adr/0003-local-client-import.md`）：
//! - **只读**：不修改、不备份客户端文件；
//! - 读不到就说清楚为什么，不回退去猜令牌；
//! - 仅 Windows（客户端不在手机上）。
//!
//! 目前只有 WorkBuddy 能可靠读取：它的客户端会把签到用的长效令牌缓存到
//! `%USERPROFILE%\.workbuddy\checkin-rt.json`，`rt` 字段是 Keycloak 的
//! Offline 刷新令牌（JWT，明文）。
//!
//! Trae CN / Qoder CN 的登录态在各自客户端的加密库里（需要先退出客户端再解密），
//! 本版不提供，扫描结果里会如实标注，界面据此提示改用手动令牌。

use crate::identity::jwt_payload;
use chrono::TimeZone as _;
use ph_core::{Credential, Error, Result, Vendor};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 一个平台的本机导入可用性，给界面展示用。
#[derive(Debug, Clone)]
pub struct ImportStatus {
    pub vendor: Vendor,
    pub available: bool,
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
                detail: "本机导入仅支持 Windows".into(),
            })
            .collect(),
    }
}

/// 读取某平台的本机登录态。调用方负责先展示扫描结果，再让用户确认导入。
pub fn import(vendor: Vendor) -> Result<LocalCredential> {
    let home = home_dir()
        .ok_or_else(|| Error::Unsupported("本机导入仅支持 Windows".into()))?;
    import_in(vendor, &home)
}

/// 指定用户目录的扫描版本（测试与自定义环境用）。
pub fn scan_in(home: &Path) -> Vec<ImportStatus> {
    Vendor::ALL
        .iter()
        .map(|v| match read_credential(*v, home) {
            Ok(found) => ImportStatus {
                vendor: *v,
                available: true,
                detail: found.detail,
            },
            Err(e) => ImportStatus {
                vendor: *v,
                available: false,
                detail: e.user_message(),
            },
        })
        .collect()
}

/// 指定用户目录的导入版本。
pub fn import_in(vendor: Vendor, home: &Path) -> Result<LocalCredential> {
    read_credential(vendor, home)
}

fn read_credential(vendor: Vendor, home: &Path) -> Result<LocalCredential> {
    match vendor {
        Vendor::WorkBuddy => workbuddy_credential(home),
        Vendor::TraeCn => Err(Error::Unsupported(
            "Trae CN 的登录态在客户端的加密数据库里，本版暂不支持本机导入；请用手动令牌（需填设备 ID）。"
                .into(),
        )),
        Vendor::QoderCn => Err(Error::Unsupported(
            "Qoder CN 的登录态是加密文件，且余额/签到端点尚未实测；本版暂不支持本机导入，请用手动令牌。"
                .into(),
        )),
    }
}

// ---------------------------------------------------------------- WorkBuddy

fn workbuddy_credential(home: &Path) -> Result<LocalCredential> {
    let path = home.join(".workbuddy").join("checkin-rt.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::NotFound(format!(
            "没找到 WorkBuddy 客户端的登录态（{}）。请先登录 WorkBuddy 客户端，或改用手动令牌。",
            path.display()
        )),
        std::io::ErrorKind::PermissionDenied => Error::Auth(
            "没有权限读取 WorkBuddy 客户端的登录态文件，请检查文件权限后重试。".into(),
        ),
        _ => Error::Io(format!("读取 WorkBuddy 登录态失败：{e}")),
    })?;

    let json: Value = serde_json::from_str(&raw).map_err(|_| {
        Error::Format("WorkBuddy 客户端登录态格式无法解析（客户端可能已升级），请改用手动令牌。".into())
    })?;
    let rt = json
        .get("rt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Format(
                "WorkBuddy 客户端登录态里没有长效令牌（rt 为空），请打开客户端重新登录后再试。".into(),
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

fn fmt_time(epoch: i64) -> String {
    chrono::Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| epoch.to_string())
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
    use base64::Engine as _;
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

    #[test]
    fn reads_workbuddy_refresh_token() {
        let home = temp_home("ok");
        let now = chrono::Local::now().timestamp();
        let token = jwt(serde_json::json!({ "exp": now + 3600, "nickname": "测试号" }));
        write_checkin(&home, &token);

        let found = import_in(Vendor::WorkBuddy, &home).unwrap();
        assert_eq!(found.credential.refresh_token.as_deref(), Some(token.as_str()));
        assert!(found.credential.access_token.is_none());
        assert!(found.detail.contains("测试号"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn rejects_expired_token() {
        let home = temp_home("expired");
        let now = chrono::Local::now().timestamp();
        write_checkin(&home, &jwt(serde_json::json!({ "exp": now - 60 })));

        let err = import_in(Vendor::WorkBuddy, &home).unwrap_err();
        assert!(err.user_message().contains("过期"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn missing_file_points_to_manual_token() {
        let home = temp_home("missing");
        let err = import_in(Vendor::WorkBuddy, &home).unwrap_err();
        assert!(err.user_message().contains("手动令牌"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn malformed_rt_is_rejected_not_guessed() {
        let home = temp_home("bad");
        write_checkin(&home, "not-a-jwt");
        let err = import_in(Vendor::WorkBuddy, &home).unwrap_err();
        assert!(err.user_message().contains("JWT"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn scan_marks_only_workbuddy_available() {
        let home = temp_home("scan");
        let now = chrono::Local::now().timestamp();
        write_checkin(&home, &jwt(serde_json::json!({ "exp": now + 3600 })));

        let statuses = scan_in(&home);
        let wb = statuses.iter().find(|s| s.vendor == Vendor::WorkBuddy).unwrap();
        assert!(wb.available, "找到客户端登录态时应可用：{}", wb.detail);
        for v in [Vendor::TraeCn, Vendor::QoderCn] {
            let s = statuses.iter().find(|s| s.vendor == v).unwrap();
            assert!(!s.available, "{} 本版应如实标为不可用", v.display());
        }
        let _ = std::fs::remove_dir_all(&home);
    }
}
