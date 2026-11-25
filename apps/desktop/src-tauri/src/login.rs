//! 登录导入的公共部分：登录网址、凭据组装、诊断日志。
//!
//! 为什么不读客户端登录态：WorkBuddy 5.6.2+ 把令牌换成 AES 信封，密钥由客户端
//! 运行时提供、不落盘。要在本地解出来只能扫进程内存——既复杂又容易被安全软件拦。
//!
//! 也不从网页登录态里抠令牌：实测网页版的登录凭证是 **HttpOnly 的 session cookie**，
//! `localStorage` 里只有埋点数据，压根没有 JWT；而 cookie 是随请求轮换的短命会话，
//! 拷出来用不了几次。所以改走 **官方插件授权**：换回来的是 refreshToken，能长期自续期。
//! 具体实现在 `browser_login`，用的是你平时那个浏览器，不弹新窗口。

use ph_core::{Credential, Vendor};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// 每个平台的登录入口。
///
/// 只放站点根路径：登录方式（手机号 / 扫码 / SSO）由平台自己的页面决定，
/// 我们写死 `/login` 反而会在改版后失效。
pub fn login_url(vendor: Vendor) -> &'static str {
    match vendor {
        Vendor::WorkBuddy => "https://www.workbuddy.cn",
        Vendor::TraeCn => "https://www.trae.cn",
        Vendor::QoderCn => "https://www.qoder.com",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedAuth {
    /// 协议版本：1 = 早期的网页 localStorage 抓取，2 = 官方插件授权。
    #[serde(default)]
    pub v: u32,
    pub token: String,
    /// 长效令牌。有它才能长期自续期，才是一个能一直用的账号。
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// 短期令牌的过期时间（epoch 秒），0/None 表示未知。
    #[serde(default)]
    pub expires_at: Option<i64>,
    /// 抓取时的浏览器 UA，请求时必须原样回放。
    #[serde(default)]
    pub ua: String,
    /// 抓取页面的 origin，用于推断鉴权域名。
    #[serde(default)]
    pub origin: String,
}

/// 各平台真正接受 API 请求的主机名。
///
/// 登录页可能跳转到任意 SSO 域名（例如统一登录），**不能**直接拿 origin 当接口域名——
/// 那样签到会打到登录站上去。只有 origin 恰好是这些主机时才写进凭据，否则交给
/// 适配器用默认域名。
fn api_hosts(vendor: Vendor) -> &'static [&'static str] {
    match vendor {
        Vendor::WorkBuddy => &["www.codebuddy.cn", "codebuddy.cn"],
        Vendor::TraeCn => &["www.trae.cn", "trae.cn"],
        Vendor::QoderCn => &["qoder.com", "www.qoder.com"],
    }
}

/// 把抓到的令牌落成凭据。
pub fn build_credential(vendor: Vendor, auth: &CapturedAuth) -> Credential {
    let mut cred = Credential::from_access_token(auth.token.trim());
    // 长效令牌才是能长期用的那把钥匙：有它就存，后面靠它自续期。
    if let Some(rt) = auth.refresh_token.as_deref().map(str::trim) {
        if !rt.is_empty() {
            cred.refresh_token = Some(rt.to_string());
        }
    }
    if let Some(exp) = auth.expires_at.filter(|v| *v > 0) {
        cred.expires_at = Some(exp);
    }
    if !auth.ua.trim().is_empty() {
        cred.user_agent = Some(auth.ua.trim().to_string());
    }
    // 授权换来的令牌没有绑定浏览器 UA，但网关认浏览器 UA；留空就用适配器默认值。
    if cred.user_agent.is_none() && vendor == Vendor::WorkBuddy {
        cred.user_agent = Some(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/126.0.0.0 Safari/537.36"
                .into(),
        );
    }
    let host = auth
        .origin
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    if api_hosts(vendor).iter().any(|h| *h == host) {
        cred.domain = Some(host);
    }
    cred
}

/// 登录流程的诊断日志。
///
/// 发布版是 `windows_subsystem = "windows"`，没有控制台，`eprintln!` 等于丢掉；
/// 所以同时落一份到 `logs/login.log`。用户登录完没自动导入时，看这一行就知道
/// 是「页面没进站点」还是「令牌存在别的键名下」。
pub(crate) fn logln(app: &AppHandle, msg: &str) {
    eprintln!("{msg}");
    let dir = app
        .state::<crate::SharedState>()
        .data_dir
        .join(ph_core::files::LOGS_DIR);
    if std::fs::create_dir_all(&dir).is_ok() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("login.log"))
        {
            let _ = writeln!(
                f,
                "[{}] {}",
                chrono::Local::now().format("%m-%d %H:%M:%S"),
                msg
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_ua_and_only_trusts_known_api_hosts() {
        // 登录站就是 API 站：域名落进凭据
        let a = CapturedAuth {
            v: 2,
            token: "t".into(),
            refresh_token: Some("r".into()),
            expires_at: Some(123),
            ua: "UA/1.0".into(),
            origin: "https://www.codebuddy.cn".into(),
        };
        let c = build_credential(Vendor::WorkBuddy, &a);
        assert_eq!(c.user_agent.as_deref(), Some("UA/1.0"));
        assert_eq!(c.domain.as_deref(), Some("www.codebuddy.cn"));

        // SSO 跳转域名不该被当成接口域名
        let b = CapturedAuth {
            origin: "https://sso.example.com".into(),
            ..a.clone()
        };
        let c2 = build_credential(Vendor::WorkBuddy, &b);
        assert!(c2.domain.is_none(), "陌生 origin 不能当接口域名");
        assert_eq!(c2.user_agent.as_deref(), Some("UA/1.0"));
    }

    #[test]
    fn every_vendor_has_a_login_url() {
        for v in Vendor::ALL {
            assert!(login_url(v).starts_with("https://"));
        }
    }
}
