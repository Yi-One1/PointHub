//! 积分枢纽 PointHub —— 桌面端 / 移动端共用的外壳。
//!
//! 这一层只做三件事：把引擎装进状态、把能力暴露成命令、把窗口摆好。
//! 业务判断一律在 `ph-engine` 里，这里不写「签到该怎么重试」这类逻辑。
//!
//! **平台差异集中在这里**：托盘、悬浮球、开机自启只有桌面有；
//! 安卓没有「关窗口到托盘」这个概念。用 `cfg(desktop)` / `cfg(mobile)`
//! 显式分叉，而不是靠运行时判断——编不过就早点暴露，别等到用户手机上白屏。

pub mod browser_login;
pub mod lan;
pub mod commands;
pub mod login;

#[cfg_attr(mobile, allow(unused_imports))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg_attr(mobile, allow(unused_imports))]
use tauri::Manager;

use ph_engine::Engine;

/// 全局状态。
///
/// 为什么不把 settings 也放进 tokio 锁：窗口关闭事件是同步回调，
/// 那里 `await` 不了。用一个原子标志单独承载「关闭时是否最小化到托盘」，
/// 是这一层唯一允许的冗余。
pub struct AppState {
    pub engine: tokio::sync::Mutex<Engine>,
    /// 数据目录。发布版没有控制台，诊断信息要落到这里的 `logs/`。
    pub data_dir: std::path::PathBuf,
    pub minimize_to_tray: AtomicBool,
    pub ball_enabled: AtomicBool,
    /// 局域网服务开关与端口（从设置里同步过来，供同步上下文的 setup 读取）。
    pub sync_enabled: AtomicBool,
    pub sync_port: std::sync::atomic::AtomicU16,
    /// 运行中的局域网服务；未启动时为 None。
    pub lan: std::sync::Mutex<Option<Arc<lan::LanCtx>>>,
}

pub type SharedState = Arc<AppState>;

/// 安卓上的数据目录。
///
/// 必须问系统要（`app_data_dir()` 返回应用私有目录）。早期版本写死了
/// `/data/data/com.pointhub.app/files`——包名其实是 `cn.pointhub.desktop`，
/// 目录建不出来，`Engine::open` 失败后直接 `exit(1)`，表现就是**一打开就闪退**。
fn mobile_data_dir(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    #[cfg(mobile)]
    {
        return app.path().app_data_dir().ok();
    }
    #[cfg(desktop)]
    {
        let _ = app;
        None
    }
}

// 这个宏负责生成 `Java_..._Rust_onCreate` 之类的 JNI 导出。
// 少了它，so 能加载但找不到入口，安卓一启动就是 UnsatisfiedLinkError。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg_attr(mobile, allow(unused_variables))]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::refresh_all,
            commands::checkin,
            commands::travel,
            commands::add_account,
            commands::browser_login,
            commands::remove_account,
            commands::set_account_enabled,
            commands::save_settings,
            commands::open_data_dir,
            commands::relocate_data,
            commands::set_ball,
            commands::set_ball_expanded,
            commands::set_ball_account,
            commands::show_main,
            commands::quit_app,
            commands::lan_info,
        ])
        .setup(|app| {
            // 数据目录：命令行指定 > 安卓应用私有目录 > 各平台默认。
            // 引擎放在这里建，是因为安卓的目录只有拿到 AppHandle 才问得到。
            let engine = match Engine::open(data_dir_arg().or_else(|| mobile_data_dir(app.handle()))) {
                Ok(e) => e,
                Err(e) => {
                    let msg = format!("数据目录初始化失败：{}", e.user_message());
                    eprintln!("{msg}");
                    return Err(msg.into());
                }
            };
            let state: SharedState = Arc::new(AppState {
                data_dir: engine.data_dir().to_path_buf(),
                minimize_to_tray: AtomicBool::new(engine.settings().minimize_to_tray),
                // 移动端没有悬浮球，标志位保留但不起作用。
                ball_enabled: AtomicBool::new(engine.settings().ball.enabled),
                sync_enabled: AtomicBool::new(engine.settings().sync.enabled),
                sync_port: std::sync::atomic::AtomicU16::new(engine.settings().sync.port),
                lan: std::sync::Mutex::new(None),
                engine: tokio::sync::Mutex::new(engine),
            });
            app.manage(state);

            #[cfg(desktop)]
            {
                build_tray(app.handle())?;
                if app.state::<SharedState>().ball_enabled.load(Ordering::Relaxed) {
                    if let Err(e) = open_ball(app.handle()) {
                        // 悬浮球失败不能拖垮主窗口。
                        eprintln!("悬浮球创建失败（不影响主窗口）：{e}");
                    }
                }
                sync_autostart(app.handle());
                spawn_scheduler(app.handle().clone());

                // 局域网服务：手机和电脑同一个 Wi-Fi 时，手机浏览器就能看账本。
                if app.state::<SharedState>().sync_enabled.load(Ordering::Relaxed) {
                    match lan::start(app.handle().clone()) {
                        Ok(ctx) => {
                            if let Ok(mut slot) = app.state::<SharedState>().lan.lock() {
                                *slot = Some(ctx);
                            }
                        }
                        Err(e) => eprintln!("局域网服务未启动：{e}"),
                    }
                }

                // 调试入口：`--login workbuddy` 启动即发起浏览器授权。
                // 界面自动化要按坐标点按钮，窗口位置一变就会点空；直接给个入口，
                // 排查「授权到底卡在哪一步」时不必先跟坐标较劲。
                if let Some(v) = login_arg() {
                    if let Err(e) = browser_login::start(app.handle().clone(), v) {
                        eprintln!("打开浏览器登录失败：{e}");
                    }
                }
            }
            Ok(())
        });

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec!["--autostart"]),
            ))
            .on_window_event(|window, event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let state = window.state::<SharedState>();
                    if window.label() == "main" && state.minimize_to_tray.load(Ordering::Relaxed) {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            });
    }

    builder
        .run(tauri::generate_context!())
        .expect("Tauri 启动失败");
}

// ------------------------------------------------------------------ 启动参数

/// 解析 `--data-dir <路径>` / `--data-dir=<路径>`。
///
/// 用途有两个：便携版把数据放在自己旁边，以及开发时用演示数据看界面——
/// 不必为了看一眼截图去动用户真正的数据目录。
fn data_dir_arg() -> Option<std::path::PathBuf> {
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--data-dir" {
            return it.next().map(std::path::PathBuf::from);
        }
        if let Some(rest) = a.strip_prefix("--data-dir=") {
            if !rest.trim().is_empty() {
                return Some(std::path::PathBuf::from(rest));
            }
        }
    }
    None
}

/// 解析 `--login <平台>`：启动后立刻打开该平台的登录窗口。
///
/// 只在桌面端有意义（安卓没有多窗口），值不合法时静默忽略，
/// 免得一个手误把启动流程带崩。
fn login_arg() -> Option<ph_core::Vendor> {
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--login" {
            return it.next().and_then(|v| ph_core::Vendor::parse(v.trim()));
        }
        if let Some(rest) = a.strip_prefix("--login=") {
            return ph_core::Vendor::parse(rest.trim());
        }
    }
    None
}

// ------------------------------------------------------------------ 桌面专属

#[cfg(desktop)]
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

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

#[cfg(desktop)]
pub fn bring_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg(mobile)]
pub fn bring_main_window(_app: &tauri::AppHandle) {
    // 移动端只有一个窗口，且总是前台。
}

/// 打开（或聚焦）悬浮球窗口。移动端无此概念，调用即为空操作。
#[cfg(desktop)]
pub fn open_ball(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

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
    //
    // 位置必须按**窗口实际尺寸**算，不能按 96 常量算：WebView 实际尺寸
    // 可能和请求值有出入，按常量算会让球贴出屏幕边缘（被裁掉一条）。
    if let Ok(Some(monitor)) = w.current_monitor() {
        let size = monitor.size();
        let origin = monitor.position();
        let scale = monitor.scale_factor();
        let win_w = w
            .outer_size()
            .map(|s| s.width as i32)
            .unwrap_or((96.0 * scale) as i32);
        let margin = (24.0 * scale) as i32;
        let x = origin.x + size.width as i32 - win_w - margin;
        let y = origin.y + (60.0 * scale) as i32;
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
    Ok(())
}

#[cfg(mobile)]
pub fn open_ball(_app: &tauri::AppHandle) -> tauri::Result<()> {
    Ok(())
}

#[cfg(desktop)]
pub fn close_ball(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("ball") {
        let _ = w.close();
    }
}

#[cfg(mobile)]
pub fn close_ball(_app: &tauri::AppHandle) {}

#[cfg(desktop)]
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

// ------------------------------------------------------------------ 常驻调度

/// 后台签到循环。
///
/// 设计取舍：
/// - **45 秒一轮**。签到时间本身带 0~5 分钟确定性抖动，秒级精度没有意义；
///   轮询太密只是在空转锁。
/// - 每轮只做「判断谁该走了」这一件小事，真正签到复用 `run_checkin_inner`，
///   保证手动点按钮和自动跑走的是同一条代码路径（含通知）。
/// - 跑完 emit 一次 `ph://changed`，界面收到就重拉状态，不靠前端轮询。
#[cfg(desktop)]
fn spawn_scheduler(app: tauri::AppHandle) {
    use std::time::Duration;
    use tauri::Emitter;

    tauri::async_runtime::spawn(async move {
        // 起步先等一会：首屏正在读数据，没必要抢同一把锁。
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            let now = chrono::Local::now().naive_local();
            let ids: Vec<String> = {
                let state = app.state::<SharedState>();
                let engine = state.engine.lock().await;
                let s = engine.settings();
                if !s.scheduler_enabled {
                    Vec::new()
                } else {
                    let catch_up = s.catch_up_missed;
                    engine
                        .due_accounts(now)
                        .into_iter()
                        .filter(|a| {
                            if catch_up {
                                return true;
                            }
                            // 不补签时只认「刚过点 30 分钟内」的窗口。
                            let slot = a.schedule.slot_on(now.date(), &a.id);
                            now.signed_duration_since(slot) <= chrono::Duration::minutes(30)
                        })
                        .map(|a| a.id)
                        .collect()
                }
            };

            if !ids.is_empty() {
                let _ = commands::run_checkin_inner(&app, Some(ids)).await;
                let _ = app.emit("ph://changed", ());
            }
            tokio::time::sleep(Duration::from_secs(45)).await;
        }
    });
}
