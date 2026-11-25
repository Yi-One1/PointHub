//! 登录导入的公共部分：登录网址、凭据组装、诊断日志。
//!
//! 为什么不读客户端登录态：WorkBuddy 5.6.2+ 把令牌换成 AES 信封，密钥由客户端
//! 运行时提供、不落盘。要在本地解出来只能扫进程内存——既复杂又容易被安全软件拦。
//!
//! 所以走另一条：**用系统浏览器登录，从浏览器的 localStorage 里取令牌**。
//! 具体实现在 `browser_login`（CDP 驱动，不需要 F12、不需要手动复制）。
//!
//! 令牌来自浏览器，部分网关会校验「会话与 UA 绑定」，所以抓取时的 UA 要一起存下、
//! 请求时原样回放，否则会 401。

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
    /// 协议版本，方便以后改字段。
    #[serde(default)]
    pub v: u32,
    pub token: String,
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
    if !auth.ua.trim().is_empty() {
        cred.user_agent = Some(auth.ua.trim().to_string());
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
            v: 1,
            token: "t".into(),
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
