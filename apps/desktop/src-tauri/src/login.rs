//! 浏览器登录导入。
//!
//! 为什么不做「读客户端登录态」：WorkBuddy 5.6.2+ 把令牌换成 AES 信封，
//! 密钥由客户端运行时提供、不落盘。要在本地解出来只能去扫进程内存——
//! 那既复杂又容易被安全软件拦，不值得。
//!
//! 所以走另一条：**打开官方登录页，让用户自己登录**。登录后页面把令牌放在
//! localStorage 里，我们注入一段只读脚本把它取出来，连同当前 User-Agent
//! 一起带走（部分网关会校验「会话与 UA 绑定」，不带原 UA 会 401）。
//!
//! 回传通道用地址哈希：脚本 `history.replaceState` 把凭据写进 `#ph-auth=`，
//! Rust 侧轮询 `webview.url()` 取。这样不需要远程 IPC 权限、不需要本地 HTTP
//! 服务、也不需要给远端页面任何 Tauri 能力——注入的脚本只能写自己的地址栏。

use base64::Engine;
use ph_core::{Credential, Vendor};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

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

pub fn window_label(vendor: Vendor) -> String {
    format!("login-{}", vendor.id())
}

pub fn vendor_of_label(label: &str) -> Option<Vendor> {
    label.strip_prefix("login-").and_then(Vendor::parse)
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

/// 注入到登录页的只读脚本。
///
/// 三条约束：
/// 1. **只读**：只读 localStorage / sessionStorage，不写、不发网络请求；
/// 2. 只认 JWT 形态（`eyJ...`.`...`.`...`），避免把 SDK 的临时串当令牌；
/// 3. 用 `history.replaceState` 而不是改 `location.hash`——后者会触发
///    `hashchange`，有些单页应用会因此重新路由。
const EXTRACT_JS: &str = r##"
(function () {
  var KEYS = ["accessToken", "access_token", "token", "jwt", "authToken", "Authorization", "auth"];
  var JWT = /^eyJ[\w-]+\.[\w-]+\.[\w-]+$/;

  function grab() {
    for (var i = 0; i < KEYS.length; i++) {
      var k = KEYS[i];
      try { var v = localStorage.getItem(k); if (v) return v; } catch (e) {}
      try { var s = sessionStorage.getItem(k); if (s) return s; } catch (e) {}
    }
    return "";
  }

  function pick(raw) {
    var t = String(raw).trim();
    if (t.charAt(0) === "{") {
      try {
        var o = JSON.parse(t);
        t = (o && o.auth && o.auth.accessToken) || o.accessToken || o.token || "";
      } catch (e) { return ""; }
    }
    t = String(t).replace(/^Bearer\s+/i, "");
    // 有的站点把令牌存成 JSON 字符串里的字段，再去一层引号
    t = t.replace(/^"|"$/g, "");
    return JWT.test(t) ? t : "";
  }

  function b64u(s) {
    var b = btoa(unescape(encodeURIComponent(s)));
    return b.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }

  function probe() {
    var token = pick(grab());
    if (!token) return;
    var payload = b64u(JSON.stringify({
      v: 1,
      token: token,
      ua: navigator.userAgent,
      origin: location.origin
    }));
    var want = "#ph-auth=" + payload;
    if (location.hash === want) return;
    try { history.replaceState(null, "", want); } catch (e) { location.hash = want.slice(1); }
  }

  probe();
  setInterval(probe, 1500);
  document.addEventListener("visibilitychange", probe);

  // 20 秒还没拿到令牌，就把「页面里实际有哪些存储键」报回来（只报键名，不报值）。
  // 令牌的键名各站点不一样，靠猜会一直卡住；有了这份名单，改 KEYS 一次就够。
  setTimeout(function () {
    if (pick(grab())) return;
    var names = [];
    try {
      for (var i = 0; i < localStorage.length && i < 80; i++) names.push(localStorage.key(i));
    } catch (e) {}
    try {
      for (var j = 0; j < sessionStorage.length && j < 80; j++) names.push("s:" + sessionStorage.key(j));
    } catch (e) {}
    var payload = b64u(JSON.stringify({ v: 1, origin: location.origin, keys: names }));
    try { history.replaceState(null, "", "#ph-diag=" + payload); } catch (e) {}
  }, 20000);
})();
"##;

/// 从地址哈希里取回凭据。返回 None 表示这次还没拿到。
pub fn parse_url(url: &tauri::Url) -> Option<CapturedAuth> {
    let frag = url.fragment()?;
    let b64 = frag.strip_prefix("ph-auth=")?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64.as_bytes())
        .ok()?;
    let auth: CapturedAuth = serde_json::from_slice(&raw).ok()?;
    if auth.token.trim().is_empty() {
        return None;
    }
    Some(auth)
}

/// 从地址哈希里取回「页面有哪些存储键」的自诊断结果。
///
/// 只在拿不到令牌时由注入脚本写入，用来快速定位「令牌存在别的键名下」这类问题。
pub fn parse_diag(url: &tauri::Url) -> Option<DiagReport> {
    let frag = url.fragment()?;
    let b64 = frag.strip_prefix("ph-diag=")?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64.as_bytes())
        .ok()?;
    serde_json::from_slice(&raw).ok()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagReport {
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub keys: Vec<String>,
}

/// 登录流程的诊断日志。
///
/// 发布版是 `windows_subsystem = "windows"`，没有控制台，`eprintln!` 等于丢掉；
/// 所以同时落一份到 `logs/login.log`。用户登录完没自动导入时，看这一行就知道
/// 是「页面没引入站点」还是「令牌存在别的键名下」。
fn logln(app: &AppHandle, msg: &str) {
    eprintln!("{msg}");
    let dir = app.state::<crate::SharedState>().data_dir.join(ph_core::files::LOGS_DIR);
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

/// 打开登录窗口，并在后台等待凭据回传。
pub fn start(app: &AppHandle, vendor: Vendor) -> Result<(), String> {
    if let Some(old) = app.get_webview_window(&window_label(vendor)) {
        let _ = old.close();
    }
    let url: tauri::Url = login_url(vendor)
        .parse()
        .map_err(|e| format!("登录地址无效：{e}"))?;
    let label = window_label(vendor);
    WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url.clone()))
        .title("登录后会自动导入 · 积分枢纽")
        .inner_size(560.0, 780.0)
        .min_inner_size(420.0, 560.0)
        .center()
        .initialization_script(EXTRACT_JS)
        .on_page_load(|w, payload| {
            // 导航失败时 WebView2 只会给一片空白，不留错误页，所以把加载事件
            // 记下来，出问题时能直接看出是「没导航」还是「导航了但渲染空」。
            logln(
                &w.app_handle().clone(),
                &format!("page_load event={:?} url={}", payload.event(), payload.url()),
            );
        })
        .build()
        .map_err(|e| format!("打开登录窗口失败：{e}"))?;
    logln(app, &format!("window created for {url}"));

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        // 最多等 8 分钟：够慢的扫码登录，也不至于永远挂着一个线程。
        let mut last: Option<String> = None;
        let mut diag_logged = false;
        for _ in 0..960 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let Some(w) = app2.get_webview_window(&label) else {
                return; // 用户自己关了窗口
            };
            match w.url() {
                Ok(u) => {
                    // 只在地址变化时记一行：排查空白页时能看出「停在 about:blank」
                    // 还是「跳到了站点」，又不至于把日志刷满。
                    let cur = u.to_string();
                    if last.as_deref() != Some(cur.as_str()) {
                        // 地址哈希里可能带整段 base64，日志只留前面一截。
                        let shown: String = cur.chars().take(110).collect();
                        logln(&app2, &format!("url -> {shown}"));
                        last = Some(cur);
                    }
                    if let Some(auth) = parse_url(&u) {
                        let _ = w.close();
                        let _ = crate::commands::import_from_browser(&app2, vendor, auth).await;
                        return;
                    }
                    if let Some(diag) = parse_diag(&u) {
                        if !diag_logged {
                            diag_logged = true;
                            logln(
                                &app2,
                                &format!("未发现令牌；origin={} 存储键={:?}", diag.origin, diag.keys),
                            );
                        }
                    }
                }
                Err(_) => return,
            }
        }
    });
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(auth: &CapturedAuth) -> String {
        let raw = serde_json::to_vec(auth).unwrap();
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
    }

    fn url_with(payload: &str) -> tauri::Url {
        format!("https://www.workbuddy.cn/#ph-auth={payload}")
            .parse()
            .unwrap()
    }

    #[test]
    fn parses_captured_payload_from_fragment() {
        let auth = CapturedAuth {
            v: 1,
            token: "eyJhbGciOi.payload.sig".into(),
            ua: "Mozilla/5.0 test".into(),
            origin: "https://www.codebuddy.cn".into(),
        };
        let got = parse_url(&url_with(&encode(&auth))).expect("应当解析出凭据");
        assert_eq!(got.token, auth.token);
        assert_eq!(got.ua, auth.ua);
    }

    #[test]
    fn ignores_urls_without_payload() {
        let u: tauri::Url = "https://www.workbuddy.cn/".parse().unwrap();
        assert!(parse_url(&u).is_none());
        let u2: tauri::Url = "https://www.workbuddy.cn/#ph-auth=not-base64!!".parse().unwrap();
        assert!(parse_url(&u2).is_none());
    }

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
            assert_eq!(vendor_of_label(&window_label(v)), Some(v));
        }
    }
}
