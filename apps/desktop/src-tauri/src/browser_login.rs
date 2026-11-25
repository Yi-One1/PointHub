//! 用**你自己的浏览器**完成登录授权，再回到软件里用。
//!
//! 走的是客户端插件登录的同一套官方流程：
//!
//! ```text
//! POST copilot.tencent.com/v2/plugin/auth/state?platform=workbuddy
//!   -> { state, authUrl }
//! 用系统默认浏览器打开 authUrl（就是你平时那个浏览器，新开一个标签页）
//! GET  copilot.tencent.com/v2/plugin/auth/token?state=...
//!   -> 202 / code 11217 还在等；成功后给 accessToken + refreshToken
//! ```
//!
//! 为什么不用「内嵌窗口」，也不去读浏览器里存的令牌：
//!
//! - 内嵌 WebView2 在部分机器上整片空白；
//! - 网页版把登录态放在 **HttpOnly 的 session cookie** 里，`localStorage` 里根本没有
//!   令牌（实测键名全是埋点数据）。而 cookie 是随请求轮换的网页会话，拷出来放进
//!   软件里用不了几次就失效；
//! - 官方授权给的是 **refreshToken**，可以长期自续期，而且登录一次浏览器就记住了。
//!
//! 所以这里只做三件事：要一个授权链接 → 交给你自己的浏览器 → 轮询结果。

use ph_core::Vendor;
use ph_providers::workbuddy::{AuthPoll, WorkBuddyProvider};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use crate::login::CapturedAuth;

/// 授权链接有效期有限，等太久也没意义。
const DEADLINE: Duration = Duration::from_secs(600);
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

/// 发起一次授权登录。立刻返回，真正的等待在后台线程里。
pub fn start(app: AppHandle, vendor: Vendor) -> Result<(), String> {
    if vendor != Vendor::WorkBuddy {
        return Err(format!(
            "{} 暂未接入浏览器授权，请改用「手动令牌」导入。",
            vendor.display()
        ));
    }

    tauri::async_runtime::spawn(async move {
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
            // 打不开浏览器也把链接送回界面，让用户可以自己复制
            let _ = tauri::Emitter::emit(&app, "ph://auth-url", &start.auth_url);
            return;
        }
        // 链接也发一份给界面：浏览器被拦、或者用户想换台设备打开时有个备用入口
        let _ = tauri::Emitter::emit(&app, "ph://auth-url", &start.auth_url);
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
                    let _ = tauri::Emitter::emit(&app, "ph://login", "授权链接已失效，请重新点一次登录");
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
                    // 网络抖一下就放弃太脆，记一笔继续等
                    crate::login::logln(&app, &format!("轮询授权结果出错（会继续重试）：{}", e.user_message()));
                }
            }
        }
    });

    Ok(())
}
