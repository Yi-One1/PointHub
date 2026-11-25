//! 共享 HTTP 客户端与响应解析小工具。
//!
//! 所有平台共用同一套超时/重试/错误映射，避免每个适配器各写一份。

use ph_core::{Error, Result};
use serde_json::Value;
use std::time::Duration;

/// 默认 UA。部分网关会校验 UA 与会话是否一致，
/// 那种场景必须用凭据里保存的原始 UA（见 `client_for`）。
pub const DEFAULT_UA: &str = "PointHub/0.1.0 (+https://github.com/Yi-One1/PointHub)";

/// 构造客户端。`ua` 为空时用默认值。
pub fn build_client(ua: Option<&str>) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(ua.unwrap_or(DEFAULT_UA))
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(10))
        .gzip(true)
        .build()
        .map_err(|e| Error::Network(format!("HTTP 客户端初始化失败：{e}")))
}

/// 把 `code` 字段统一取成 i64（可能是数字、字符串、或缺失）。
pub fn code_of(v: &Value) -> Option<i64> {
    match v.get("code") {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

pub fn msg_of(v: &Value) -> String {
    for key in ["msg", "message", "error", "errmsg"] {
        if let Some(s) = v.get(key).and_then(Value::as_str) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

/// 在若干候选字段名里找第一个数值。
///
/// 真实接口里同一个含义会换名字（`total_credits` 与 `total_credit` 都出现过），
/// 硬编码一个名字就会得到「余额永远为空」。
pub fn pick_i64(v: &Value, keys: &[&str]) -> Option<i64> {
    for k in keys {
        if let Some(n) = v.get(*k).and_then(Value::as_i64) {
            return Some(n);
        }
        if let Some(f) = v.get(*k).and_then(Value::as_f64) {
            return Some(f as i64);
        }
        if let Some(s) = v.get(*k).and_then(Value::as_str) {
            if let Ok(n) = s.trim().parse::<i64>() {
                return Some(n);
            }
        }
    }
    None
}

pub fn pick_bool(v: &Value, keys: &[&str]) -> Option<bool> {
    for k in keys {
        if let Some(b) = v.get(*k).and_then(Value::as_bool) {
            return Some(b);
        }
    }
    None
}

pub fn pick_str(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(Value::as_str) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// 余额字段的候选名（按优先级）。集中一处，避免各适配器漂移。
pub const BALANCE_KEYS: &[&str] = &[
    "total_credits",
    "total_credit",
    "balance",
    "credits",
    "credit_balance",
    "remaining_credits",
    "available_credits",
];

/// 把 HTTP 状态映射成领域错误。
pub fn map_status(status: u16, body: &Value) -> Option<Error> {
    let msg = msg_of(body);
    match status {
        200..=299 => None,
        401 | 403 => Some(Error::Auth(if msg.is_empty() {
            "令牌被拒绝（401/403）".into()
        } else {
            msg
        })),
        404 => Some(Error::Upstream(format!(
            "接口地址不存在（404）：{}",
            if msg.is_empty() { "可能是平台已改版" } else { &msg }
        ))),
        429 => Some(Error::Upstream("请求过于频繁（429），请稍后再试".into())),
        s if s >= 500 => Some(Error::Upstream(format!(
            "平台服务端异常（{s}）{}",
            if msg.is_empty() { String::new() } else { format!("：{msg}") }
        ))),
        s => {
            if msg.is_empty() {
                Some(Error::Upstream(format!("未预期的状态码 {s}")))
            } else {
                Some(Error::Upstream(msg))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn code_accepts_number_and_string() {
        assert_eq!(code_of(&json!({"code": 0})), Some(0));
        assert_eq!(code_of(&json!({"code": "10001"})), Some(10001));
        assert_eq!(code_of(&json!({})), None);
    }

    #[test]
    fn balance_falls_back_across_names() {
        assert_eq!(pick_i64(&json!({"total_credits": 1340}), BALANCE_KEYS), Some(1340));
        assert_eq!(pick_i64(&json!({"balance": 42}), BALANCE_KEYS), Some(42));
        assert_eq!(pick_i64(&json!({"balance": "42"}), BALANCE_KEYS), Some(42));
        assert_eq!(pick_i64(&json!({}), BALANCE_KEYS), None);
    }

    #[test]
    fn status_mapping() {
        assert!(map_status(200, &json!({})).is_none());
        assert!(matches!(map_status(401, &json!({})), Some(Error::Auth(_))));
        assert!(matches!(map_status(503, &json!({})), Some(Error::Upstream(_))));
    }
}
