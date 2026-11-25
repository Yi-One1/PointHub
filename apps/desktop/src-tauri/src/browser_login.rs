//! 用系统浏览器登录，并从浏览器里取回令牌。
//!
//! 为什么不用内嵌窗口：WebView2 在部分机器上渲染异常（整片空白），而且它的
//! 会话和用户平时用的浏览器是两套，等于每次都要重新登录。
//!
//! 做法：用**独立的浏览器配置目录**启动 Edge/Chrome，并打开调试端口；
//! 我们在本机通过 CDP（Chrome DevTools Protocol）在这台浏览器里执行一段
//! 只读脚本，把 `localStorage` 里的令牌取回来。全程：
//!
//! - 不碰用户平时的浏览器配置（独立 user-data-dir，互不影响）
//! - 不开 DevTools、不需要 F12、不需要复制粘贴
//! - 令牌只在本机进程之间流动，不出网
//!
//! 安全边界：调试端口只绑定 `127.0.0.1`，且只在这台机器上；脚本只读不写。

use ph_core::Vendor;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use crate::login::CapturedAuth;

/// 在页面里跑的只读脚本：先按常见键名找 JWT，找不到就把**键名**报回来。
///
/// 返回一个 JSON 字符串（`returnByValue` 直接给字符串，省去解嵌套）。
const PROBE_JS: &str = r#"
(function () {
  var KEYS = ["accessToken","access_token","token","jwt","authToken","Authorization","auth"];
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
    t = String(t).replace(/^Bearer\s+/i, "").replace(/^"|"$/g, "");
    return JWT.test(t) ? t : "";
  }
  var tk = pick(grab());
  if (tk) return JSON.stringify({ token: tk, ua: navigator.userAgent, origin: location.origin });
  var names = [];
  try { for (var i = 0; i < localStorage.length && i < 80; i++) names.push(localStorage.key(i)); } catch (e) {}
  return JSON.stringify({ keys: names, origin: location.origin });
})()
"#;

// ------------------------------------------------------------------ 浏览器发现

/// 找本机的 Chromium 系浏览器（Edge 优先：Windows 一定有）。
fn find_browser() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for var in ["PROGRAMFILES(X86)", "PROGRAMFILES", "LOCALAPPDATA"] {
        if let Ok(base) = std::env::var(var) {
            let base = PathBuf::from(base);
            candidates.push(base.join("Microsoft/Edge/Application/msedge.exe"));
            candidates.push(base.join("Google/Chrome/Application/chrome.exe"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// 让系统分配一个空闲端口（bind 到 0 再读回端口号）。
fn free_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

// ------------------------------------------------------------------ 极小 HTTP

/// 找字节串第一次出现的位置。
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// 对 127.0.0.1 发一个 GET，返回正文。只用于读 CDP 的 `/json/list`。
///
/// 不能直接 `read_to_end`：对端不一定按 `Connection: close` 关连接，
/// 读超时一到就会返回 Err，把**已经读到的数据一起丢掉**（实测导致目标列表永远是空）。
/// 所以按 `Content-Length` 判断收够了没有。
fn http_get(port: u16, path: &str) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    s.set_write_timeout(Some(Duration::from_secs(3))).ok()?;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes()).ok()?;

    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 8192];
    let mut need: Option<usize> = None;
    loop {
        match s.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => break, // 超时：用已读到的部分
        }
        if need.is_none() {
            if let Some(pos) = find(&buf, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                need = Some(pos + 4 + len);
            }
        }
        if let Some(w) = need {
            if buf.len() >= w {
                break;
            }
        }
    }

    let pos = find(&buf, b"\r\n\r\n")?;
    let rest = &buf[pos + 4..];
    let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
    let len = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(rest.len());
    let body = &rest[..len.min(rest.len())];
    Some(String::from_utf8_lossy(body).to_string())
}

/// 从 `/json/list` 里挑一个要操作的页面目标。
fn pick_target(list: &str, host_hint: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(list.trim()).ok()?;
    let arr = v.as_array()?;
    let mut fallback: Option<(String, String)> = None;
    for t in arr {
        if t.get("type").and_then(|x| x.as_str()) != Some("page") {
            continue;
        }
        let url = t.get("url").and_then(|x| x.as_str()).unwrap_or("");
        let ws = t.get("webSocketDebuggerUrl").and_then(|x| x.as_str())?;
        if url.starts_with("http") && url.contains(host_hint) {
            return Some((url.to_string(), ws.to_string()));
        }
        // about:blank / 其它页留作兜底：登录可能跳到了别的域名
        if fallback.is_none() && url.starts_with("http") {
            fallback = Some((url.to_string(), ws.to_string()));
        }
    }
    fallback
}

/// 从 ws:// 地址里取端口（CDP 都在本机）。
fn ws_port(ws_url: &str) -> Option<u16> {
    let rest = ws_url.strip_prefix("ws://")?;
    let host_port = rest.split('/').next()?;
    host_port.rsplit(':').next()?.parse().ok()
}

/// 连上去执行一次探测，返回脚本给出的 JSON 字符串。
///
/// 自己建 TCP 连接而不用 `tungstenite::connect`：后者拿不到底层流，
/// 读操作没有超时，浏览器不回消息就会把这个线程挂死（实测过一次，
/// 表现是登录流程毫无日志地停住）。
fn evaluate(ws_url: &str) -> Option<String> {
    use tungstenite::client::IntoClientRequest;

    let port = ws_port(ws_url)?;
    let stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(4)))
        .ok()?;

    let req = ws_url.into_client_request().ok()?;
    let (mut ws, _) = match tungstenite::client(req, stream) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[cdp] handshake failed: {e}");
            return None;
        }
    };

    let req = json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": { "expression": PROBE_JS, "returnByValue": true, "awaitPromise": false }
    });
    if let Err(e) = ws.send(tungstenite::Message::Text(req.to_string())) {
        eprintln!("[cdp] send failed: {e}");
        return None;
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let msg = match ws.read() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[cdp] read: {e}");
                return None;
            }
        };
        let text = match msg {
            tungstenite::Message::Text(t) => t,
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("id").and_then(|x| x.as_i64()) != Some(1) {
            continue; // 事件推送，跳过
        }
        if let Some(err) = v.get("error") {
            eprintln!("[cdp] evaluate error: {err}");
            return None;
        }
        let val = v.get("result")?.get("result")?.get("value")?.as_str()?;
        return Some(val.to_string());
    }
    None
}

/// 每个平台页面上可能出现的域名（用于挑目标标签页）。
fn host_hint(vendor: Vendor) -> &'static str {
    match vendor {
        Vendor::WorkBuddy => "codebuddy.cn",
        Vendor::TraeCn => "trae.cn",
        Vendor::QoderCn => "qoder.com",
    }
}

// ------------------------------------------------------------------ 主流程

/// 启动系统浏览器并等待令牌。返回前会先把浏览器关掉。
///
/// 全过程跑在后台线程：用户登录可能要几十秒，不能卡住界面。
pub fn start(app: AppHandle, vendor: Vendor) -> Result<(), String> {
    let exe = find_browser().ok_or_else(|| {
        "没有找到 Edge 或 Chrome。请先安装其中之一，或改用「手动令牌」导入。".to_string()
    })?;
    let port = free_port().ok_or_else(|| "拿不到空闲端口".to_string())?;

    let data_dir = app.state::<crate::SharedState>().data_dir.clone();
    let profile = data_dir.join("browser-profile");
    std::fs::create_dir_all(&profile).map_err(|e| format!("建浏览器配置目录失败：{e}"))?;

    let url = crate::login::login_url(vendor);
    crate::login::logln(
        &app,
        &format!("启动系统浏览器：{} port={port} url={url}", exe.display()),
    );

    let child = Command::new(&exe)
        .arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // 关掉「恢复上次页面」的提示条，免得挡住登录
        .arg("--disable-features=msEdgeRestoreTabs")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动浏览器失败：{e}"))?;

    let app2 = app.clone();
    std::thread::spawn(move || {
        let outcome = wait_for_token(&app2, vendor, port);
        // 无论成功与否都收掉这个专用浏览器实例：它是我们开的，用户不该多一个窗口。
        shutdown(Some(child));
        match outcome {
            Ok(Some(auth)) => {
                let app3 = app2.clone();
                tauri::async_runtime::block_on(async move {
                    let _ = crate::commands::import_from_browser(&app3, vendor, auth).await;
                });
            }
            Ok(None) => crate::login::logln(&app2, "等待超时，未拿到令牌"),
            Err(e) => crate::login::logln(&app2, &format!("登录失败：{e}")),
        }
    });

    Ok(())
}

/// 轮询浏览器里的页面，直到拿到令牌或超时（8 分钟）。
fn wait_for_token(
    app: &AppHandle,
    vendor: Vendor,
    port: u16,
) -> Result<Option<CapturedAuth>, String> {
    let hint = host_hint(vendor);
    let deadline = Instant::now() + Duration::from_secs(480);
    let mut last_note = String::new();
    let mut diag_logged = false;

    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1500));

        let Some(list) = http_get(port, "/json/list") else {
            continue; // 浏览器还在启动
        };
        let Some((page_url, ws)) = pick_target(&list, hint) else {
            continue;
        };

        let Some(raw) = evaluate(&ws) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };

        if let Some(token) = v.get("token").and_then(|x| x.as_str()) {
            if !token.trim().is_empty() {
                let auth = CapturedAuth {
                    v: 1,
                    token: token.to_string(),
                    ua: v.get("ua").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    origin: v
                        .get("origin")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                };
                crate::login::logln(app, &format!("已取到令牌，来自 {page_url}"));
                return Ok(Some(auth));
            }
        }

        // 没令牌：把「页面上有哪些存储键」记一次，便于判断是不是键名不对
        if !diag_logged {
            if let Some(keys) = v.get("keys").and_then(|x| x.as_array()) {
                if !keys.is_empty() {
                    diag_logged = true;
                    let names: Vec<&str> = keys.iter().filter_map(|k| k.as_str()).collect();
                    crate::login::logln(
                        app,
                        &format!(
                            "页面 {} 暂未发现令牌；存储键={:?}",
                            v.get("origin").and_then(|x| x.as_str()).unwrap_or("-"),
                            names
                        ),
                    );
                }
            }
        }

        let note = format!("等待登录中：{page_url}");
        if note != last_note {
            last_note = note.clone();
            crate::login::logln(app, &note);
        }
    }
    Ok(None)
}

/// 收掉我们开的浏览器实例。
///
/// 只杀我们 spawn 出来的那个进程树（`/T`），不动用户自己开着的浏览器。
fn shutdown(child: Option<Child>) {
    if let Some(mut c) = child {
        // /T 连子进程一起收：Edge 是多进程的，只杀根进程会留下渲染进程
        let _ = Command::new("taskkill")
            .args(["/PID", &c.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = c.wait();
    }
}
