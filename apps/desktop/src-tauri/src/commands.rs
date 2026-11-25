//! Tauri 命令层：前端能调的全部能力都在这里，一个不多。
//!
//! 约定：
//! - 命令只做「参数整形 + 调引擎 + 转成前端友好的结构」；
//! - 任何用户可见的错误都返回人话，不把 Rust 错误类型漏给前端；
//! - 需要通知用户的动作（签到完成/失败）在这里统一发系统通知。

#[cfg_attr(mobile, allow(unused_imports))]
use crate::{close_ball, open_ball, SharedState};
use ph_core::{Credential, Settings, Vendor};
use ph_engine::Aggregate;
use ph_providers::{provider_for, TravelAction};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State};

// ------------------------------------------------------------------ 视图结构

#[derive(Serialize)]
pub struct PlatformCapView {
    pub vendor: String,
    pub display: String,
    pub checkin: bool,
    pub travel: bool,
    pub balance: bool,
    pub token_refresh: bool,
    pub verified: bool,
}

#[derive(Serialize)]
pub struct AccountView {
    pub id: String,
    pub vendor: String,
    pub vendor_display: String,
    pub label: String,
    pub hint: String,
    pub enabled: bool,
    pub checkin_enabled: bool,
    pub travel_enabled: bool,
    pub schedule_text: String,
    pub balance: Option<i64>,
    pub checked_in_today: Option<bool>,
    pub streak_days: Option<i64>,
    pub today_gain: Option<i64>,
    pub travel_state: Option<String>,
    pub travel_location: Option<String>,
    pub captured_at: Option<i64>,
    pub last_error: Option<String>,
    pub has_credential: bool,
    pub can_checkin: bool,
    pub can_travel: bool,
    pub verified: bool,
}

#[derive(Serialize)]
pub struct EventView {
    pub at: i64,
    pub account_id: String,
    pub label: String,
    pub vendor: String,
    pub kind: String,
    pub delta: Option<i64>,
    pub text: String,
}

#[derive(Serialize)]
pub struct StateView {
    pub version: String,
    pub data_dir: String,
    pub vault_kind: String,
    pub history_count: usize,
    pub accounts: Vec<AccountView>,
    pub aggregate: Aggregate,
    pub trend: Vec<ph_engine::DayPoint>,
    pub events: Vec<EventView>,
    /// 下一次自动签到的时刻（epoch 秒）；没有启用账号时为 None。
    pub next_run_at: Option<i64>,
    pub settings: Settings,
    pub platforms: Vec<PlatformCapView>,
}

#[derive(Serialize)]
pub struct OutcomeView {
    pub account_id: String,
    pub label: String,
    pub status: String,
    pub status_text: String,
    pub message: String,
    pub credit: Option<i64>,
}

fn build_state(engine: &ph_engine::Engine) -> StateView {
    let latest = engine.latest_snapshots();
    let accounts = engine
        .accounts()
        .iter()
        .map(|a| {
            let provider = provider_for(a.vendor);
            let cap = provider.capabilities();
            let snap = latest.get(&a.id);
            AccountView {
                id: a.id.clone(),
                vendor: a.vendor.id().to_string(),
                vendor_display: a.vendor.display().to_string(),
                label: a.label.clone(),
                hint: a.hint.clone(),
                enabled: a.enabled,
                checkin_enabled: a.checkin_enabled,
                travel_enabled: a.travel_enabled,
                schedule_text: a.schedule.describe(),
                balance: snap.and_then(|s| s.balance),
                checked_in_today: snap.and_then(|s| s.checked_in_today),
                streak_days: snap.and_then(|s| s.streak_days),
                today_gain: snap.and_then(|s| s.today_gain),
                travel_state: snap
                    .and_then(|s| s.travel.as_ref())
                    .map(|t| t.state.label().to_string()),
                travel_location: snap.and_then(|s| s.travel.as_ref()).and_then(|t| t.location.clone()),
                captured_at: snap.map(|s| s.captured_at),
                last_error: a.last_error.clone(),
                has_credential: engine.has_credential(&a.id),
                can_checkin: cap.checkin,
                can_travel: cap.travel && a.travel_enabled,
                verified: cap.verified,
            }
        })
        .collect();

    let platforms = ph_providers::all_capabilities()
        .into_iter()
        .map(|(v, c)| PlatformCapView {
            vendor: v.id().to_string(),
            display: v.display().to_string(),
            checkin: c.checkin,
            travel: c.travel,
            balance: c.balance,
            token_refresh: c.token_refresh,
            verified: c.verified,
        })
        .collect();

    StateView {
        version: ph_core::VERSION.to_string(),
        data_dir: engine.data_dir().to_string_lossy().to_string(),
        vault_kind: engine.vault_kind().to_string(),
        history_count: engine.history_count(),
        accounts,
        aggregate: engine.aggregate(),
        trend: engine.daily_series(7),
        events: engine
            .recent_events(12)
            .into_iter()
            .map(|e| EventView {
                at: e.at,
                account_id: e.account_id,
                label: e.label,
                vendor: e.vendor.id().to_string(),
                kind: e.kind,
                delta: e.delta,
                text: e.text,
            })
            .collect(),
        next_run_at: engine.next_run_at(),
        settings: engine.settings().clone(),
        platforms,
    }
}

// ------------------------------------------------------------------ 查询

#[tauri::command]
pub async fn bootstrap(state: State<'_, SharedState>) -> Result<StateView, String> {
    let engine = state.engine.lock().await;
    Ok(build_state(&engine))
}

/// 全量采样后刷新视图。单个账号失败不影响其它账号。
#[tauri::command]
pub async fn refresh_all(state: State<'_, SharedState>) -> Result<StateView, String> {
    let mut engine = state.engine.lock().await;
    let (_snaps, _errs) = engine.snapshot_all().await;
    Ok(build_state(&engine))
}

// ------------------------------------------------------------------ 签到

#[tauri::command]
pub async fn checkin(
    app: AppHandle,
    ids: Option<Vec<String>>,
) -> Result<Vec<OutcomeView>, String> {
    run_checkin_inner(&app, ids).await
}

/// 供托盘菜单与命令共用的签到执行体（含通知）。
pub async fn run_checkin_inner(
    app: &AppHandle,
    ids: Option<Vec<String>>,
) -> Result<Vec<OutcomeView>, String> {
    let state = app.state::<SharedState>();
    let mut engine = state.engine.lock().await;
    let settings = engine.settings().clone();

    let raw = match ids {
        Some(list) if !list.is_empty() => {
            let mut out = Vec::new();
            for id in list {
                out.push(engine.checkin_one(&id).await);
            }
            out.into_iter().map(|r| r.map_err(|e| e.user_message())).collect::<Vec<_>>()
        }
        _ => engine
            .checkin_all()
            .await
            .into_iter()
            .map(Ok)
            .collect::<Vec<_>>(),
    };

    let mut views = Vec::new();
    let mut claimed = 0;
    let mut already = 0;
    let mut failed = 0;
    for r in raw {
        match r {
            Ok(o) => {
                let label = engine
                    .find(&o.account_id)
                    .map(|a| a.label.clone())
                    .unwrap_or_else(|| o.account_id.clone());
                match o.status {
                    ph_core::CheckinStatus::Claimed => claimed += 1,
                    ph_core::CheckinStatus::AlreadyDone => already += 1,
                    ph_core::CheckinStatus::NotEligible => {}
                    _ => failed += 1,
                }
                views.push(OutcomeView {
                    account_id: o.account_id,
                    label,
                    status: format!("{:?}", o.status).to_lowercase(),
                    status_text: o.status.label().to_string(),
                    message: o.message,
                    credit: o.credit,
                });
            }
            Err(msg) => {
                failed += 1;
                views.push(OutcomeView {
                    account_id: String::new(),
                    label: "未知账号".into(),
                    status: "failed".into(),
                    status_text: "失败".into(),
                    message: msg,
                    credit: None,
                });
            }
        }
    }
    drop(engine);

    // 系统通知：签到是后台行为，不给反馈用户会以为没跑。
    let should_notify = if failed > 0 {
        settings.notify_on_failure
    } else {
        settings.notify_on_checkin
    };
    if should_notify {
        use tauri_plugin_notification::NotificationExt;
        let body = format!(
            "新领 {claimed} 个 · 已签 {already} 个{}",
            if failed > 0 {
                format!(" · 失败 {failed} 个")
            } else {
                String::new()
            }
        );
        let _ = app
            .notification()
            .builder()
            .title("积分枢纽 · 签到完成")
            .body(body)
            .show();
    }

    Ok(views)
}

// ------------------------------------------------------------------ 派猫猫

#[tauri::command]
pub async fn travel(
    state: State<'_, SharedState>,
    id: String,
    action: Option<String>,
    location: Option<u32>,
) -> Result<String, String> {
    let act = match action.as_deref().unwrap_or("auto") {
        "status" => TravelAction::Status,
        "claim" => TravelAction::ClaimOnly,
        "depart" => TravelAction::DepartOnly,
        _ => TravelAction::Auto,
    };
    let mut engine = state.engine.lock().await;
    let snap = engine
        .travel_one(&id, act, location)
        .await
        .map_err(|e| e.user_message())?;
    Ok(format!(
        "{} {}{}",
        snap.state.label(),
        snap.location.unwrap_or_default(),
        snap.note
            .map(|n| format!("（{n}）"))
            .unwrap_or_default()
    ))
}

// ------------------------------------------------------------------ 账号

#[derive(Deserialize)]
pub struct AddAccountPayload {
    pub vendor: String,
    pub label: String,
    /// 令牌本体（长效或短期）。
    pub secret: String,
    /// "refresh" | "access"
    pub secret_kind: String,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub cookie: Option<String>,
}

#[tauri::command]
pub async fn add_account(
    state: State<'_, SharedState>,
    payload: AddAccountPayload,
) -> Result<String, String> {
    let vendor = Vendor::parse(&payload.vendor)
        .ok_or_else(|| format!("不认识的平台：{}", payload.vendor))?;
    if payload.secret.trim().is_empty() {
        return Err("令牌不能为空".into());
    }
    let mut cred = Credential::default();
    if payload.secret_kind == "access" {
        cred.access_token = Some(payload.secret.trim().to_string());
    } else {
        cred.refresh_token = Some(payload.secret.trim().to_string());
    }
    cred.device_id = payload.device_id.filter(|s| !s.trim().is_empty());
    cred.domain = payload.domain.filter(|s| !s.trim().is_empty());
    cred.cookie = payload.cookie.filter(|s| !s.trim().is_empty());

    let mut engine = state.engine.lock().await;
    let acc = engine
        .add_account(vendor, payload.label, cred)
        .map_err(|e| e.user_message())?;
    Ok(acc.id)
}

#[tauri::command]
pub async fn remove_account(state: State<'_, SharedState>, id: String) -> Result<(), String> {
    let mut engine = state.engine.lock().await;
    engine.remove_account(&id).map_err(|e| e.user_message())
}

#[tauri::command]
pub async fn set_account_enabled(
    state: State<'_, SharedState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let mut engine = state.engine.lock().await;
    engine
        .set_account_enabled(&id, enabled)
        .map_err(|e| e.user_message())
}

// ------------------------------------------------------------------ 设置

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    settings: Settings,
) -> Result<StateView, String> {
    {
        let mut engine = state.engine.lock().await;
        engine
            .update_settings(settings.clone())
            .map_err(|e| e.user_message())?;
    }

    state
        .minimize_to_tray
        .store(settings.minimize_to_tray, Ordering::Relaxed);

    // 自启状态对齐（仅桌面；安卓没有这个机制）
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let manager = app.autolaunch();
        let current = manager.is_enabled().unwrap_or(false);
        if settings.autostart && !current {
            let _ = manager.enable();
        } else if !settings.autostart && current {
            let _ = manager.disable();
        }
    }

    // 悬浮球开关
    if settings.ball.enabled != state.ball_enabled.load(Ordering::Relaxed) {
        let _ = apply_ball(&app, settings.ball.enabled);
    }

    let engine = state.engine.lock().await;
    Ok(build_state(&engine))
}

#[tauri::command]
pub fn set_ball(app: AppHandle, enabled: bool) -> Result<(), String> {
    apply_ball(&app, enabled)
}

/// 悬浮球收起时的直径、展开后的面板尺寸（逻辑像素）。
const BALL_SIZE: f64 = 96.0;
const BALL_PANEL_W: f64 = 336.0;
const BALL_PANEL_H: f64 = 384.0;

/// 展开 / 收起悬浮球窗口。
///
/// 关键点：**右边缘锚定**。球贴在屏幕右侧时，若向左扩展且不锚定，
/// 面板会直接飞出屏幕；锚定后球体位置看不出变化，面板从它下面长出来。
/// 再对四边做一次夹紧，防止贴边时露到屏幕外。
#[tauri::command]
pub fn set_ball_expanded(app: AppHandle, expanded: bool) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri::{PhysicalPosition, PhysicalSize};
        let w = match app.get_webview_window("ball") {
            Some(w) => w,
            None => return Ok(()),
        };
        let scale = w.scale_factor().map_err(|e| e.to_string())?;
        let pos = w.outer_position().map_err(|e| e.to_string())?;
        let size = w.outer_size().map_err(|e| e.to_string())?;

        let (lw, lh) = if expanded {
            (BALL_PANEL_W, BALL_PANEL_H)
        } else {
            (BALL_SIZE, BALL_SIZE)
        };
        let new_w = (lw * scale).round().max(1.0) as i32;
        let new_h = (lh * scale).round().max(1.0) as i32;

        let right = pos.x + size.width as i32;
        let mut x = right - new_w;
        let mut y = pos.y;

        if let Ok(Some(m)) = w.current_monitor() {
            let ms = m.size();
            let mp = m.position();
            let left = mp.x;
            let top = mp.y;
            let right_lim = mp.x + ms.width as i32;
            let bottom_lim = mp.y + ms.height as i32;
            if x + new_w > right_lim {
                x = right_lim - new_w;
            }
            if x < left {
                x = left;
            }
            if y + new_h > bottom_lim {
                y = bottom_lim - new_h;
            }
            if y < top {
                y = top;
            }
        }

        // 先定位再改尺寸：反过来的话窗口会先缩小/放大再跳一下，肉眼可见。
        w.set_position(PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
        w.set_size(PhysicalSize::new(new_w as u32, new_h as u32))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(mobile)]
    {
        let _ = (app, expanded);
    }
    Ok(())
}

/// 悬浮球绑定哪个账号（`None` = 全部合计）。
///
/// 单独开一个命令而不是复用 `save_settings`：球窗口若拿旧快照整包回写，
/// 会把主窗口刚改的别的设置覆盖掉。
#[tauri::command]
pub async fn set_ball_account(
    state: State<'_, SharedState>,
    account_id: Option<String>,
) -> Result<(), String> {
    let mut engine = state.engine.lock().await;
    let mut s = engine.settings().clone();
    s.ball.selected = account_id;
    engine.update_settings(s).map_err(|e| e.user_message())
}

pub fn apply_ball(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let state = app.state::<SharedState>();
    state.ball_enabled.store(enabled, Ordering::Relaxed);
    #[cfg(desktop)]
    {
        if enabled {
            open_ball(app).map_err(|e| format!("悬浮球创建失败：{e}"))?;
        } else {
            close_ball(app);
        }
    }
    #[cfg(mobile)]
    {
        let _ = (open_ball, close_ball, enabled);
    }
    // 记住开关，下次启动沿用
    let engine_locked = state.engine.try_lock();
    if let Ok(mut engine) = engine_locked {
        let mut s = engine.settings().clone();
        s.ball.enabled = enabled;
        let _ = engine.update_settings(s);
    }
    Ok(())
}

// ------------------------------------------------------------------ 目录 / 退出

#[tauri::command]
pub async fn open_data_dir(state: State<'_, SharedState>) -> Result<(), String> {
    let engine = state.engine.lock().await;
    let dir = engine.data_dir().to_path_buf();
    drop(engine);
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开目录失败：{e}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = &dir;
    }
    Ok(())
}

/// 搬迁数据目录：复制现有文件到新位置，写指针，重启后生效。
#[tauri::command]
pub async fn relocate_data(
    state: State<'_, SharedState>,
    path: String,
) -> Result<String, String> {
    let target = std::path::PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("路径不能为空".into());
    }
    let engine = state.engine.lock().await;
    let from = engine.data_dir().to_path_buf();
    drop(engine);

    if from == target {
        return Ok("目标与当前目录相同，无需搬迁。".into());
    }
    if target.starts_with(&from) {
        return Err("目标目录不能在当前数据目录内部".into());
    }

    std::fs::create_dir_all(&target).map_err(|e| format!("无法创建目标目录：{e}"))?;
    let mut copied = 0usize;
    copy_dir(&from, &target, &mut copied).map_err(|e| format!("复制失败：{e}"))?;
    ph_store::locate::write_pointer(Some(&target)).map_err(|e| e.user_message())?;
    Ok(format!(
        "已复制 {copied} 个文件到 {}，重启程序后生效。",
        target.display()
    ))
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path, count: &mut usize) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let dest = to.join(entry.file_name());
        if path.is_dir() {
            std::fs::create_dir_all(&dest)?;
            copy_dir(&path, &dest, count)?;
        } else {
            std::fs::copy(&path, &dest)?;
            *count += 1;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn show_main(app: AppHandle) {
    crate::bring_main_window(&app);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
