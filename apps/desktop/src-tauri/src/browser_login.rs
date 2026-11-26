//! 用**你自己的浏览器**完成登录授权，再回到软件里用。
//!
//! 三个平台都是「系统默认浏览器 + 本地只读凭据」，不内嵌 WebView，也不读浏览器存储：
//!
//! | 平台 | 流程 |
//! |---|---|
//! | WorkBuddy | 官方插件授权：`auth/state` 换链接 → 轮询 `auth/token` |
//! | Qoder CN | 设备码 + PKCE：浏览器里确认 → 轮询 `deviceToken/poll` |
//! | Trae CN | PKCE + 本地回调：127.0.0.1 收回调 → 用 AuthCode 换令牌（设备私钥签名） |
//!
//! 为什么不用「内嵌窗口」，也不去读浏览器里存的令牌：
//!
//! - 内嵌 WebView2 在部分机器上整片空白；
//! - 网页版把登录态放在 **HttpOnly 的 session cookie** 里，`localStorage` 里根本没有
//!   令牌（实测键名全是埋点数据）。而 cookie 是随请求轮换的网页会话，拷出来放进
//!   软件里用不了几次就失效；
//! - 官方授权给的是 **refreshToken**，可以长期自续期，而且登录一次浏览器就记住了。
//!
//! 所以这里只做三件事：要一个授权链接 → 交给你自己的浏览器 → 等结果。

use ph_core::Vendor;
use ph_providers::qoder::QoderCnProvider;
use ph_providers::trae::TraeCnProvider;
use ph_providers::workbuddy::{AuthPoll, WorkBuddyProvider};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::login::CapturedAuth;

/// 授权链接有效期有限，等太久也没意义。
const DEADLINE: Duration = Duration::from_secs(600);
/// Trae 授权页要求 5 分钟内完成。
const TRAE_DEADLINE: Duration = Duration::from_secs(300);
/// 轮询间隔：太密没必要，太疏用户会觉得「点完没反应」。
const POLL_EVERY: Duration = Duration::from_secs(2);

/// 用系统默认浏览器打开一个链接。
///
/// 不走 `cmd /C start`：URL 里的 `&`、`?` 会被 cmd 当成命令分隔符，得自己加引号，
/// 而且引号里再嵌引号很容易翻车。这几个协议处理器都不经过 shell，参数是原样的。
fn open_in_browser(url: &str) -> Result<(), String> {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
        ("rundll32.exe", vec!["url.dll,FileProtocolHandler", url])
    } else if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else {
        ("xdg-open", vec![url])
    };
    let status = Command::new(cmd)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("没能调起浏览器（{cmd}）：{e}"))?;
    if !status.success() {
        return Err("没能调起浏览器，可以手动复制授权链接打开".into());
    }
    Ok(())
}

/// 发起一次授权登录。立刻返回，真正的等待在后台任务里。
pub fn start(app: AppHandle, vendor: Vendor) -> Result<(), String> {
    match vendor {
        Vendor::WorkBuddy => start_workbuddy(app),
        Vendor::QoderCn => start_qoder(app),
        Vendor::TraeCn => start_trae(app),
    }
}

fn announce(app: &AppHandle, url: &str) {
    let _ = tauri::Emitter::emit(app, "ph://auth-url", url);
}

// ---------------------------------------------------------------- WorkBuddy

fn start_workbuddy(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn(async move {
        let vendor = Vendor::WorkBuddy;
        let provider = WorkBuddyProvider::new();
        let start = match provider.begin_auth().await {
            Ok(v) => v,
            Err(e) => {
                crate::login::logln(&app, &format!("发起授权失败：{}", e.user_message()));
                return;
            }
        };

        crate::login::logln(&app, &format!("授权链接已就绪：{}", start.auth_url));
        if let Err(e) = open_in_browser(&start.auth_url) {
            crate::login::logln(&app, &e);
            announce(&app, &start.auth_url);
            return;
        }
        announce(&app, &start.auth_url);
        crate::login::logln(&app, "已在你平时的浏览器里打开授权页，等待确认");

        let deadline = Instant::now() + DEADLINE;
        let mut last_note = String::new();
        loop {
            if Instant::now() > deadline {
                crate::login::logln(&app, "等待授权超时（10 分钟），未拿到令牌");
                return;
            }
            tokio::time::sleep(POLL_EVERY).await;

            match provider.poll_auth(&start.state).await {
                Ok(AuthPoll::Pending) => {
                    if last_note.is_empty() {
                        last_note = "等待你在浏览器里确认授权".into();
                        crate::login::logln(&app, &last_note);
                    }
                }
                Ok(AuthPoll::Expired) => {
                    crate::login::logln(&app, "授权会话已失效，请重新发起");
                    let _ = tauri::Emitter::emit(
                        &app,
                        "ph://login",
                        "授权链接已失效，请重新点一次登录",
                    );
                    return;
                }
                Ok(AuthPoll::Ready(cred)) => {
                    crate::login::logln(&app, "浏览器已确认授权，拿到令牌，开始导入");
                    let auth = CapturedAuth {
                        v: 2,
                        token: cred.access_token,
                        refresh_token: cred.refresh_token,
                        expires_at: cred.expires_at,
                        ua: String::new(),
                        origin: cred
                            .domain
                            .map(|d| format!("https://{d}"))
                            .unwrap_or_default(),
                    };
                    let _ = crate::commands::import_from_browser(&app, vendor, auth).await;
                    return;
                }
                Err(e) => {
                    crate::login::logln(
                        &app,
                        &format!("轮询授权结果出错（会继续重试）：{}", e.user_message()),
                    );
                }
            }
        }
    });

    Ok(())
}

// ---------------------------------------------------------------- Qoder CN

fn start_qoder(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn(async move {
        let provider = QoderCnProvider::new();
        let session = match QoderCnProvider::begin_auth(None) {
            Ok(s) => s,
            Err(e) => {
                crate::login::logln(&app, &format!("发起 Qoder 授权失败：{}", e.user_message()));
                return;
            }
        };

        crate::login::logln(&app, &format!("Qoder 授权链接已就绪：{}", session.auth_url));
        if let Err(e) = open_in_browser(&session.auth_url) {
            crate::login::logln(&app, &e);
        }
        announce(&app, &session.auth_url);
        crate::login::logln(&app, "已在你平时的浏览器里打开 Qoder 授权页，等待确认");

        let deadline = Instant::now() + DEADLINE;
        let mut last_note = String::new();
        loop {
            if Instant::now() > deadline {
                crate::login::logln(&app, "等待 Qoder 授权超时（10 分钟）");
                let _ = tauri::Emitter::emit(&app, "ph://login", "Qoder 授权超时，请重新发起");
                return;
            }
            tokio::time::sleep(POLL_EVERY).await;

            match provider.poll_auth(&session).await {
                Ok(ph_providers::oauth::AuthPoll::Pending) => {
                    if last_note.is_empty() {
                        last_note = "等待你在 Qoder 授权页里确认".into();
                        crate::login::logln(&app, &last_note);
                    }
                }
                Ok(ph_providers::oauth::AuthPoll::Ready(cred)) => {
                    crate::login::logln(&app, "Qoder 授权完成，拿到令牌，开始导入");
                    let _ =
                        crate::commands::import_credential(&app, Vendor::QoderCn, cred).await;
                    return;
                }
                Err(e) => {
                    let msg = e.user_message();
                    if msg.contains("超过 5 分钟") {
                        crate::login::logln(&app, &msg);
                        let _ = tauri::Emitter::emit(
                            &app,
                            "ph://login",
                            "Qoder 授权已超时，请重新发起",
                        );
                        return;
                    }
                    crate::login::logln(
                        &app,
                        &format!("轮询 Qoder 授权结果出错（会继续重试）：{msg}"),
                    );
                }
            }
        }
    });

    Ok(())
}

// ---------------------------------------------------------------- Trae CN

fn start_trae(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(l) => l,
            Err(e) => {
                crate::login::logln(&app, &format!("无法监听本地回调端口：{e}"));
                return;
            }
        };
        let port = match listener.local_addr() {
            Ok(addr) => addr.port(),
            Err(e) => {
                crate::login::logln(&app, &format!("读取回调端口失败：{e}"));
                return;
            }
        };
        let callback = format!("http://127.0.0.1:{port}/authorize");
        let session = match TraeCnProvider::begin_auth(&callback) {
            Ok(s) => s,
            Err(e) => {
                crate::login::logln(&app, &format!("发起 Trae 授权失败：{}", e.user_message()));
                return;
            }
        };

        crate::login::logln(&app, &format!("Trae 授权链接已就绪：{}", session.auth_url));
        if let Err(e) = open_in_browser(&session.auth_url) {
            crate::login::logln(&app, &e);
        }
        announce(&app, &session.auth_url);
        crate::login::logln(&app, "已在你平时的浏览器里打开 Trae 授权页，等待确认");

        let callback = match wait_trae_callback(listener, TRAE_DEADLINE).await {
            Ok(v) => v,
            Err(e) => {
                crate::login::logln(&app, &e);
                let _ = tauri::Emitter::emit(&app, "ph://login", "Trae 授权超时，请重新发起");
                return;
            }
        };

        if let Some(err) = callback.error {
            crate::login::logln(&app, &format!("Trae 授权被拒绝：{err}"));
            let _ = tauri::Emitter::emit(&app, "ph://login", format!("Trae 授权失败：{err}"));
            return;
        }
        let Some(auth_code) = callback.auth_code else {
            crate::login::logln(&app, "Trae 回调里没有 AuthCode");
            let _ = tauri::Emitter::emit(&app, "ph://login", "Trae 授权失败：回调缺少 AuthCode");
            return;
        };

        let provider = TraeCnProvider::new();
        match provider
            .exchange(
                &session,
                &auth_code,
                callback.verifier.as_deref(),
                &callback.user_info,
            )
            .await
        {
            Ok(cred) => {
                crate::login::logln(&app, "Trae 授权完成，拿到令牌，开始导入");
                let _ = crate::commands::import_credential(&app, Vendor::TraeCn, cred).await;
            }
            Err(e) => {
                crate::login::logln(&app, &format!("Trae 换取令牌失败：{}", e.user_message()));
                let _ = tauri::Emitter::emit(
                    &app,
                    "ph://login",
                    format!("Trae 授权失败：{}", e.user_message()),
                );
            }
        }
    });

    Ok(())
}

struct TraeCallback {
    auth_code: Option<String>,
    verifier: Option<String>,
    user_info: serde_json::Value,
    error: Option<String>,
}

/// 等 Trae 的本地回调（最多 300 秒），返回 AuthCode 等参数。
async fn wait_trae_callback(
    listener: TcpListener,
    limit: Duration,
) -> Result<TraeCallback, String> {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        let accept = tokio::time::timeout_at(deadline, listener.accept()).await;
        let (mut stream, _) = match accept {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => return Err(format!("接收授权回调失败：{e}")),
            Err(_) => return Err("等待 Trae 授权回调超时（5 分钟），请重新发起".into()),
        };
        let mut buf = [0u8; 8192];
        let read = match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await
        {
            Ok(Ok(n)) => n,
            _ => continue,
        };
        let request = String::from_utf8_lossy(&buf[..read]).to_string();
        let Some(target) = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
        else {
            continue;
        };
        let Some((path, query)) = target.split_once('?') else {
            respond(&mut stream, "Trae 授权回调缺少参数").await;
            continue;
        };
        if path != "/authorize" {
            respond(&mut stream, "PointHub 已收到请求").await;
            continue;
        }

        let params = ph_providers::oauth::parse_query(query);
        let get = |key: &str| {
            params
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_empty())
        };
        let error = get("error_code").map(|code| {
            let msg = get("error_msg").unwrap_or_default();
            if msg.is_empty() {
                code
            } else {
                format!("{code}：{msg}")
            }
        });
        let auth_code = get("authCodeInfo")
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|v| {
                v.get("AuthCode")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            });
        let user_info = get("userInfo")
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .unwrap_or(serde_json::Value::Null);

        respond(&mut stream, "Trae 授权完成，可以关闭此页面。").await;
        return Ok(TraeCallback {
            auth_code,
            verifier: get("codeVerifier"),
            user_info,
            error,
        });
    }
}

async fn respond(stream: &mut tokio::net::TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.as_bytes().len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}
