//! 安卓后台签到（WorkManager → JNI）。
//!
//! 定时任务可能在没有 Activity 的进程里启动：不经过 Tauri 的 `setup`，
//! 所以这里自己起一个轻量 tokio 运行时，按设置里的调度规则挑出「今天该签」的账号并执行。
//! 结果以 JSON 交给 Kotlin，由 Kotlin 发系统通知——通知属于外壳，不放进引擎。
//!
//! 只处理本机模式：远程模式下凭据在电脑上，签到该由电脑的调度器完成。

use ph_engine::Engine;
use serde_json::json;
use std::path::PathBuf;

/// 供 Kotlin `CheckinBridge.runScheduledCheckin` 调用。
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_cn_pointhub_desktop_CheckinBridge_runScheduledCheckin(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    data_dir: jni::objects::JString,
) -> jni::sys::jstring {
    let dir = match env.get_string(&data_dir) {
        Ok(s) => s.to_string_lossy().into_owned(),
        Err(_) => String::new(),
    };
    // panic 会直接杀掉进程，JNI 边界上必须先兜住。
    let out = std::panic::catch_unwind(|| run_once(&dir))
        .unwrap_or_else(|_| json!({"ran": false, "reason": "panic"}).to_string());
    match env.new_string(out) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// 执行一轮定时签到，返回给外壳的 JSON。
///
/// `ran=false` 表示没有活要干（没开调度、远程模式、没有到点账号），
/// 外壳据此决定是否弹通知——「没有到点」不该打扰用户。
fn run_once(data_dir: &str) -> String {
    if data_dir.trim().is_empty() {
        return json!({"ran": false, "reason": "empty-data-dir"}).to_string();
    }
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return json!({"ran": false, "reason": format!("运行时创建失败：{e}")}).to_string(),
    };
    rt.block_on(async move {
        let mut engine = match Engine::open(Some(PathBuf::from(data_dir))) {
            Ok(e) => e,
            Err(e) => {
                return json!({"ran": false, "reason": e.user_message()}).to_string();
            }
        };
        let settings = engine.settings().clone();
        if settings.mode != ph_core::AppMode::Local {
            return json!({"ran": false, "reason": "remote"}).to_string();
        }
        if !settings.scheduler_enabled {
            return json!({"ran": false, "reason": "disabled"}).to_string();
        }

        let now = chrono::Local::now().naive_local();
        let due: Vec<String> = engine
            .due_accounts(now)
            .into_iter()
            .filter(|a| {
                if settings.catch_up_missed {
                    return true;
                }
                // 不补签时只认「刚过点 30 分钟内」的窗口，与桌面端规则一致。
                let slot = a.schedule.slot_on(now.date(), &a.id);
                now.signed_duration_since(slot) <= chrono::Duration::minutes(30)
            })
            .map(|a| a.id)
            .collect();
        // 排障用：后台任务打不开界面，只能靠 logcat 看它读到了哪个目录、几个账号。
        let accounts = engine.accounts().len();
        if due.is_empty() {
            return json!({"ran": false, "reason": "idle", "accounts": accounts, "data": data_dir})
                .to_string();
        }

        let (mut claimed, mut already, mut failed) = (0u32, 0u32, 0u32);
        let mut notes: Vec<String> = Vec::new();
        for id in due {
            match engine.checkin_one(&id).await {
                Ok(o) => match o.status {
                    ph_core::CheckinStatus::Claimed => claimed += 1,
                    ph_core::CheckinStatus::AlreadyDone => already += 1,
                    ph_core::CheckinStatus::NotEligible => {}
                    _ => {
                        failed += 1;
                        notes.push(o.message);
                    }
                },
                Err(e) => {
                    failed += 1;
                    notes.push(e.user_message());
                }
            }
        }

        let attempted = claimed + already + failed;
        let notify = attempted > 0
            && if failed > 0 {
                settings.notify_on_failure
            } else {
                settings.notify_on_checkin
            };
        let mut body = format!("新领 {claimed} 个 · 已签 {already} 个");
        if failed > 0 {
            body.push_str(&format!(" · 失败 {failed} 个"));
            if let Some(first) = notes.first() {
                body.push_str(&format!("（{first}）"));
            }
        }
        json!({
            "ran": true,
            "claimed": claimed,
            "already": already,
            "failed": failed,
            "notify": notify,
            "title": "积分枢纽 · 签到完成",
            "body": body,
        })
        .to_string()
    })
}
