//! 账号身份：从凭据里的 JWT 推断「平台侧用户 id」。
//!
//! 用途只有一个：导入时判断是不是同一个账号，避免同一账号出现两条记录。
//! 只解析 JWT 的 `sub`（标准 subject），**不验签**——签名由平台在真正调用接口时校验，
//! 这里只是本地去重，不承担鉴权职责。
//!
//! 拿不到 JWT（例如 Trae 的短期令牌是普通字符串）时返回 `None`，
//! 调用方不得退化成「按令牌片段猜」，否则不同账号可能被误合并（`DECISIONS.md` D15）。

use base64::Engine as _;
use ph_core::Credential;
use serde_json::Value;

/// 从凭据推断平台用户 id。
///
/// 优先用浏览器授权时平台直接下发的 `platform_uid`（Qoder 的 token 是不透明字符串，
/// 只能靠它识别），其次解析 JWT 的 `sub`。
pub fn platform_uid(cred: &Credential) -> Option<String> {
    cred.platform_uid
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| cred.refresh_token.as_deref().and_then(subject))
        .or_else(|| cred.access_token.as_deref().and_then(subject))
}

fn subject(token: &str) -> Option<String> {
    let payload = jwt_payload(token)?;
    payload
        .get("sub")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 取出 JWT 的 payload。只解析、不验签。
pub(crate) fn jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    if parts.next().is_none() {
        return None;
    }
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(payload: Value) -> String {
        let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.sig",
            enc(br#"{"alg":"RS256","typ":"JWT"}"#),
            enc(serde_json::to_string(&payload).unwrap().as_bytes())
        )
    }

    #[test]
    fn reads_sub_from_refresh_token() {
        let mut c = Credential::default();
        c.refresh_token = Some(jwt(serde_json::json!({ "sub": "user-1" })));
        assert_eq!(platform_uid(&c).as_deref(), Some("user-1"));
    }

    #[test]
    fn falls_back_to_access_token() {
        let mut c = Credential::default();
        c.access_token = Some(jwt(serde_json::json!({ "sub": "user-2" })));
        assert_eq!(platform_uid(&c).as_deref(), Some("user-2"));
    }

    #[test]
    fn opaque_token_has_no_identity() {
        assert!(platform_uid(&Credential::from_access_token("dt-abc")).is_none());
        assert!(platform_uid(&Credential::default()).is_none());
    }
}
