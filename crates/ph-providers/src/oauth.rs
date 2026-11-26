//! 浏览器授权流程共用的随机数 / PKCE / URL 编码小工具。
//!
//! Qoder 与 Trae 都用「PKCE + 系统浏览器」完成授权，区别只在：
//! - Qoder 是设备码轮询（无本地回调）；
//! - Trae 是本地 127.0.0.1 回调（由桌面端的 `browser_login` 负责监听）。

use ph_core::{Credential, Error, Result};
use rand::RngCore;

/// 一次轮询的结果。
#[derive(Debug, Clone)]
pub enum AuthPoll {
    /// 用户还没在浏览器里确认。
    Pending,
    /// 拿到了凭据。
    Ready(Credential),
}

/// 加密安全的随机字节。
pub fn random_bytes(len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    rand::rngs::OsRng
        .try_fill_bytes(&mut buf)
        .map_err(|_| Error::Format("系统随机数不可用".into()))?;
    Ok(buf)
}

/// 小写十六进制随机串。
pub fn random_hex(len: usize) -> Result<String> {
    Ok(random_bytes(len)?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// 形如 UUID 的稳定风格 id（去掉横线，便于直接放进 URL 参数）。
pub fn random_id() -> Result<String> {
    random_hex(16)
}

/// Base64URL 无填充编码（PKCE 规范要求）。
pub fn b64url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// PKCE S256：`base64url(sha256(verifier))`。
pub fn code_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    b64url(&Sha256::digest(verifier.as_bytes()))
}

/// RFC 3986 百分号编码（只保留 `A-Z a-z 0-9 - . _ ~`）。
pub fn url_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// 解析回调地址里的查询参数（只做 URL 解码，不处理 `+`）。
pub fn parse_query(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (url_decode(k), url_decode(v))
        })
        .collect()
}

fn url_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 毫秒时间戳折算成秒。
pub fn ms_to_secs(ms: i64) -> i64 {
    if ms > 1_000_000_000_000 {
        ms / 1000
    } else {
        ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_s256_base64url() {
        // RFC 7636 附录 B 的示例：verifier -> challenge
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let expected = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(code_challenge(verifier), expected);
        assert!(!code_challenge(verifier).contains('='));
    }

    #[test]
    fn query_roundtrip_handles_encoded_json() {
        let raw = "a=1&b=%7B%22x%22%3A%22y%22%7D&c=a%20b";
        let q = parse_query(raw);
        assert_eq!(q[0], ("a".into(), "1".into()));
        assert_eq!(q[1].1, r#"{"x":"y"}"#);
        assert_eq!(q[2].1, "a b");
    }

    #[test]
    fn url_encode_keeps_unreserved_only() {
        assert_eq!(url_encode("a b&c"), "a%20b%26c");
        assert_eq!(url_encode("http://x/y?a=1"), "http%3A%2F%2Fx%2Fy%3Fa%3D1");
    }

    #[test]
    fn random_helpers_have_expected_shape() {
        let h = random_hex(8).unwrap();
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(random_id().unwrap().len(), 32);
    }
}
