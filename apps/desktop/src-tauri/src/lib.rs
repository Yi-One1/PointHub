//! 积分枢纽 PointHub —— 桌面端装配层。
//!
//! 这一层只做三件事：把引擎装进状态、把能力暴露成命令、把窗口/托盘/悬浮球摆好。
//! 业务判断一律在 `ph-engine` 里，这里不写「签到该怎么重试」这类逻辑。

pub mod commands;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;

use ph_engine::Engine;

/// 全局状态。
///
/// 为什么不把 settings 也放进 tokio 锁：窗口关闭事件是同步回调，
/// 那里 `await` 不了。用一个原子标志单独承载「关闭时是否最小化到托盘」，
/// 是这一层唯一允许的冗余。
pub struct AppState {
    pub engine: tokio::sync::Mutex<Engine>,
    pub minimize_to_tray: AtomicBool,
    pub ball_enabled: AtomicBool,
}

pub type SharedState = Arc<AppState>;

pub fn run() {
    let engine = Engine::open(None).unwrap_or_else(|e| {
        // 数据目录打不开是致命问题，但也不能直接闪退——把原因打出来再退出。
        eprintln!("初始化失败：{}", e.user_message());
        std::process::exit(1);
    });
    let minimize = engine.settings().minimize_to_tray;
    let ball = engine.settings().ball.enabled;

    let state: SharedState = Arc::new(AppState {
        engine: tokio::sync::Mutex::new(engine),
        minimize_to_tray: AtomicBool::new(minimize),
        ball_enabled: AtomicBool::new(ball),
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::refresh_all,
            commands::checkin,
            commands::travel,
            commands::add_account,
            commands::remove_account,
            commands::set_account_enabled,
            commands::save_settings,
            commands::open_data_dir,
            commands::relocate_data,
            commands::set_ball,
            commands::show_main,
            commands::quit_app,
        ])
        .setup(|app| {
            build_tray(app.handle())?;
            // 悬浮球：默认开启，但失败不能让主窗口起不来。
            if app.state::<SharedState>().ball_enabled.load(Ordering::Relaxed) {
                if let Err(e) = open_ball(app.handle()) {
                    eprintln!("悬浮球创建失败（不影响主窗口）：{e}");
                }
            }
            // 开机自启状态与设置对齐（用户可能在别处改过）。
            sync_autostart(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<SharedState>();
                if window.label() == "main" && state.minimize_to_tray.load(Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Tauri 启动失败");
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开主窗口", true, None::<&str>)?;
    let checkin = MenuItem::with_id(app, "checkin", "一键全部签到", true, None::<&str>)?;
    let ball = MenuItem::with_id(app, "ball", "显示/隐藏悬浮球", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &checkin, &ball, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("积分枢纽 PointHub")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => bring_main_window(app),
            "checkin" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = commands::run_checkin_inner(&app, None).await;
                });
            }
            "ball" => {
                let state = app.state::<SharedState>();
                let now = state.ball_enabled.load(Ordering::Relaxed);
                let _ = commands::apply_ball(app, !now);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                bring_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn bring_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 打开（或聚焦）悬浮球窗口。
pub fn open_ball(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("ball") {
        let _ = w.show();
        return Ok(());
    }
    let w = WebviewWindowBuilder::new(app, "ball", WebviewUrl::App("index.html?ball=1".into()))
        .title("PointHub Ball")
        .inner_size(96.0, 96.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .build()?;
    // 默认贴右上角；具体坐标由前端首次拖拽后写回设置。
    if let Ok(Some(monitor)) = w.current_monitor() {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let x = (size.width as f64 / scale) as i32 - 120;
        let _ = w.set_position(tauri::PhysicalPosition::new(
            (x as f64 * scale) as i32,
            (60.0 * scale) as i32,
        ));
    }
    Ok(())
}

pub fn close_ball(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("ball") {
        let _ = w.close();
    }
}

fn sync_autostart(app: &tauri::AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let state = app.state::<SharedState>();
    // 只读设置，用 try_lock 避免阻塞启动流程。
    let wanted = match state.engine.try_lock() {
        Ok(engine) => engine.settings().autostart,
        Err(_) => false,
    };
    let manager = app.autolaunch();
    let current = manager.is_enabled().unwrap_or(false);
    match (wanted, current) {
        (true, false) => {
            let _ = manager.enable();
        }
        (false, true) => {
            let _ = manager.disable();
        }
        _ => {}
    }
}
