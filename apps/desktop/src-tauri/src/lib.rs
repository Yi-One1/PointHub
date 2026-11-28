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
pub mod firewall;
pub mod commands;
pub mod login;
#[cfg(target_os = "android")]
pub mod android_worker;
#[cfg(target_os = "android")]
pub mod android_apk;
#[cfg(target_os = "android")]
pub mod android_vault;
#[cfg(all(desktop, windows))]
pub mod purge;

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
    /// 远程模式下的客户端（配对信息 + HTTP 调用）。
    pub client: tokio::sync::Mutex<ph_client::Client>,
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
    // 安装完成后：由安装包钩子带参数启动本 exe，给当前安装路径补防火墙放行规则。
    // 已放行时直接返回，原地升级不会重复弹 UAC。
    #[cfg(all(desktop, windows))]
    if std::env::args().any(|a| a == "--firewall-install") {
        let quiet = std::env::args().any(|a| a == "--quiet");
        // `--elevated` 表示本进程就是被提权拉起来的那个，直接执行、避免二次 UAC。
        let elevated = std::env::args().any(|a| a == "--elevated");
        std::process::exit(match firewall::install(quiet, elevated) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        });
    }

    // 卸载清理：安装包钩子会在删程序文件前带参数启动本 exe。
    // 必须在 Engine::open 之前退出，否则打开引擎会重新创建数据目录。
    #[cfg(all(desktop, windows))]
    if let Some(mode) = purge_mode_arg() {
        // 静默卸载没有交互界面，无法弹 UAC 清理防火墙规则。
        let quiet = std::env::args().any(|a| a == "--quiet");
        let elevated = std::env::args().any(|a| a == "--elevated");
        std::process::exit(purge::run(mode, quiet, elevated));
    }

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    // 单实例插件必须最先注册：第二次启动时把已有窗口唤到前台，本进程随后退出。
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            bring_main_window(app);
        }));
    }

    let mut builder = builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::refresh_all,
            commands::checkin,
            commands::travel,
            commands::add_account,
            commands::local_import_scan,
            commands::local_import,
            commands::browser_login,
            commands::remove_account,
            commands::rename_account,
            commands::credit_days,
            commands::credit_year_months,
            commands::set_account_enabled,
            commands::save_settings,
            commands::default_settings,
            commands::open_data_dir,
            commands::relocate_data,
            commands::reset_data_dir,
            commands::default_data_dir,
            commands::pick_data_dir,
            commands::set_ball,
            commands::set_ball_expanded,
            commands::set_ball_account,
            commands::show_main,
            commands::quit_app,
            commands::lan_info,
            commands::client_status,
            commands::client_pair,
            commands::client_unpair,
            commands::update_check,
            commands::update_apply,
            commands::update_open_page,
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
            // 上次换过数据目录的话，现在引擎已经在新目录上，可以安全清理旧目录了。
            if let Some(report) = ph_store::locate::run_pending_cleanup(engine.data_dir()) {
                if !report.failed.is_empty() {
                    eprintln!(
                        "旧数据目录有 {} 项未能删除，将在下次启动时重试",
                        report.failed.len()
                    );
                }
            }
            let state: SharedState = Arc::new(AppState {
                data_dir: engine.data_dir().to_path_buf(),
                minimize_to_tray: AtomicBool::new(engine.settings().minimize_to_tray),
                // 移动端没有悬浮球，标志位保留但不起作用。
                ball_enabled: AtomicBool::new(engine.settings().ball.enabled),
                sync_enabled: AtomicBool::new(engine.settings().sync.enabled),
                sync_port: std::sync::atomic::AtomicU16::new(engine.settings().sync.port),
                lan: std::sync::Mutex::new(None),
                client: match ph_client::Client::open(engine.data_dir()) {
                    Ok(c) => tokio::sync::Mutex::new(c),
                    Err(e) => {
                        return Err(format!("客户端配对信息初始化失败：{}", e.user_message()).into())
                    }
                },
                engine: tokio::sync::Mutex::new(engine),
            });
            let data_dir_for_icons = state.data_dir.clone();
            app.manage(state);

            // 覆盖升级后桌面图标可能还停在旧版本（Explorer 缓存 .lnk 图标）。
            // 放在 manage 之后执行：路径取自真实数据目录，失败也不影响启动。
            #[cfg(desktop)]
            refresh_shortcut_icons(&data_dir_for_icons);

            // 双端都要：发现新版本发一条系统通知，不自动安装。
            spawn_update_check(app.handle().clone());

            // 安卓没有常驻调度循环：打开 App 时补一次今天漏掉的签到（D17）。
            #[cfg(mobile)]
            spawn_mobile_catchup(app.handle().clone());

            #[cfg(desktop)]
            {
                // 窗口/任务栏图标固定取 icons/icon.png，不依赖 codegen 的挑选规则；
                // include_bytes! 也让 cargo 在图标变化时必然重编。
                if let Some(window) = app.get_webview_window("main") {
                    if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png")) {
                        let _ = window.set_icon(icon);
                    }
                }

                build_tray(app.handle())?;
                if app.state::<SharedState>().ball_enabled.load(Ordering::Relaxed) {
                    if let Err(e) = open_ball(app.handle()) {
                        // 悬浮球失败不能拖垮主窗口。
                        eprintln!("悬浮球创建失败（不影响主窗口）：{e}");
                    }
                }
                sync_autostart(app.handle());
                spawn_scheduler(app.handle().clone());
                spawn_auto_refresh(app.handle().clone());

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

/// 快捷方式文件名 = 安装包里的产品名（`tauri.conf.json` 的 `productName`）。
#[cfg(all(desktop, windows))]
const SHORTCUT_NAME: &str = "PointHub.lnk";

/// 覆盖升级后 Explorer 会继续用旧的 `.lnk` 图标缓存，桌面看起来还是上个版本。
/// 版本变化时通知 shell 重读这几个快捷方式；每个版本只做一次，避免每次启动闪图标。
#[cfg(all(desktop, windows))]
fn refresh_shortcut_icons(data_dir: &std::path::Path) {
    let marker = data_dir.join(".icons-version");
    let current = env!("CARGO_PKG_VERSION");
    if std::fs::read_to_string(&marker).ok().as_deref() == Some(current) {
        return;
    }

    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNE_UPDATEITEM, SHCNF_IDLIST, SHCNF_PATHW,
    };

    for p in shortcut_paths() {
        if !p.exists() {
            continue;
        }
        let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: 宽字符串以 NUL 结尾，且在调用期间保持有效。
        unsafe {
            SHChangeNotify(
                SHCNE_UPDATEITEM as i32,
                SHCNF_PATHW,
                wide.as_ptr().cast(),
                std::ptr::null(),
            )
        };
    }
    // 再通知一次关联变更：部分系统只认这条路径。
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        )
    };

    let _ = std::fs::write(marker, current);
}

/// 其它桌面平台没有这个问题，保留空实现让调用点不用加 cfg。
#[cfg(all(desktop, not(windows)))]
fn refresh_shortcut_icons(_: &std::path::Path) {}

#[cfg(all(desktop, windows))]
fn shortcut_paths() -> Vec<std::path::PathBuf> {
    shortcut_paths_from(
        std::env::var("USERPROFILE").unwrap_or_default(),
        std::env::var("PUBLIC").unwrap_or_default(),
        std::env::var("APPDATA").unwrap_or_default(),
    )
}

/// 桌面（当前用户 / 公共）+ 开始菜单，三处都是安装包会创建的位置。
#[cfg(all(desktop, windows))]
fn shortcut_paths_from(user: String, public: String, appdata: String) -> Vec<std::path::PathBuf> {
    let p = std::path::Path::new;
    vec![
        p(&user).join("Desktop").join(SHORTCUT_NAME),
        p(&public).join("Desktop").join(SHORTCUT_NAME),
        p(&appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join(SHORTCUT_NAME),
    ]
}

/// 解析卸载清理参数：`--purge-data` 删账号数据，`--purge-app` 只删应用痕迹。
#[cfg(all(desktop, windows))]
fn purge_mode_arg() -> Option<purge::Mode> {
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--purge-data" => return Some(purge::Mode::Data),
            "--purge-app" => return Some(purge::Mode::AppOnly),
            _ => {}
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

    const TRAY_LIGHT: &[u8] = include_bytes!("../icons/tray-32.png");
    const TRAY_DARK: &[u8] = include_bytes!("../icons/tray-dark-32.png");

    fn tray_image(dark: bool) -> Option<tauri::image::Image<'static>> {
        let bytes = if dark { TRAY_DARK } else { TRAY_LIGHT };
        tauri::image::Image::from_bytes(bytes).ok()
    }

    let window = app.get_webview_window("main");
    let dark = window
        .as_ref()
        .and_then(|w| w.theme().ok())
        .map(|t| t == tauri::Theme::Dark)
        .unwrap_or(false);

    // 托盘用透明底的品牌标记：浅绿底板在浅色任务栏上看不清，看起来像个空白方块。
    // 深色任务栏换成提亮版，绿色才不会发闷。
    let tray_icon = tray_image(dark).or_else(|| app.default_window_icon().cloned());
    if let Some(icon) = tray_icon {
        builder = builder.icon(icon);
    }
    builder.build(app)?;

    // 系统切换深浅色时同步托盘图标；不监听的话只有重启才更新。
    if let Some(window) = window {
        let handle = app.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::ThemeChanged(theme) = event {
                if let (Some(tray), Some(icon)) = (
                    handle.tray_by_id("main"),
                    tray_image(*theme == tauri::Theme::Dark),
                ) {
                    let _ = tray.set_icon(Some(icon));
                }
            }
        });
    }
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

/// 今天该签、且已过执行时间的账号 id。
///
/// 桌面常驻循环与安卓「打开时补签」共用这一条规则，
/// 避免两边对「补签窗口」的理解慢慢分叉。
fn due_ids(engine: &ph_engine::Engine, now: chrono::NaiveDateTime) -> Vec<String> {
    let s = engine.settings();
    if !s.scheduler_enabled || s.mode != ph_core::AppMode::Local {
        return Vec::new();
    }
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

/// 安卓启动补签：没有常驻循环，打开 App 时补一次今天漏掉的（D17）。
#[cfg(mobile)]
fn spawn_mobile_catchup(app: tauri::AppHandle) {
    use std::time::Duration;

    tauri::async_runtime::spawn(async move {
        // 首屏先读数据；补签晚十几秒没有影响。
        tokio::time::sleep(Duration::from_secs(12)).await;
        let ids = {
            let state = app.state::<SharedState>();
            let engine = state.engine.lock().await;
            due_ids(&engine, chrono::Local::now().naive_local())
        };
        if !ids.is_empty() {
            let _ = commands::run_checkin_inner(&app, Some(ids)).await;
        }
    });
}

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
                due_ids(&engine, now)
            };

            if !ids.is_empty() {
                let _ = commands::run_checkin_inner(&app, Some(ids)).await;
                let _ = app.emit("ph://changed", ());
            }
            tokio::time::sleep(Duration::from_secs(45)).await;
        }
    });
}

// ------------------------------------------------------------------ 自动刷新

/// 到点没有：0 表示关闭，否则看距上次采样是否已过设定分钟数（D18）。
#[cfg(desktop)]
fn auto_refresh_due(minutes: u32, elapsed: std::time::Duration) -> bool {
    if minutes == 0 {
        return false;
    }
    elapsed >= std::time::Duration::from_secs(u64::from(minutes) * 60)
}

/// 主窗口可见时按设置间隔自动采样积分（D18）。
///
/// 每 30 秒只做一次判断；真正采样复用 `run_refresh_inner`，与手动刷新同一条路。
/// 窗口最小化或隐藏时暂停，重新可见后按上次时间判断是否立刻补一次。
#[cfg(desktop)]
fn spawn_auto_refresh(app: tauri::AppHandle) {
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    tauri::async_runtime::spawn(async move {
        // 首屏自己会刷一次，这里从启动时刻起算，避免重复请求。
        let mut last = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;

            let visible = app
                .get_webview_window("main")
                .map(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
                .unwrap_or(false);
            if !visible {
                continue;
            }
            let minutes = {
                let state = app.state::<SharedState>();
                let engine = state.engine.lock().await;
                engine.settings().auto_refresh_minutes
            };
            if !auto_refresh_due(minutes, last.elapsed()) {
                continue;
            }
            let state = app.state::<SharedState>();
            if commands::run_refresh_inner(&state).await.is_ok() {
                let _ = app.emit("ph://changed", ());
            }
            last = Instant::now();
        }
    });
}

#[cfg(all(test, desktop))]
mod auto_refresh_tests {
    use super::auto_refresh_due;
    use std::time::Duration;

    #[test]
    fn zero_minutes_means_off() {
        assert!(!auto_refresh_due(0, Duration::from_secs(24 * 3600)));
    }

    #[test]
    fn waits_for_the_interval() {
        assert!(!auto_refresh_due(10, Duration::from_secs(9 * 60)));
        assert!(auto_refresh_due(10, Duration::from_secs(10 * 60)));
    }

    #[test]
    fn honours_custom_interval() {
        assert!(!auto_refresh_due(30, Duration::from_secs(29 * 60)));
        assert!(auto_refresh_due(30, Duration::from_secs(30 * 60)));
    }
}

// ------------------------------------------------------------------ 更新提示

/// 启动后查一次更新。
///
/// 只发通知、不自动下载：装不装由用户决定。失败不打扰用户（写 stderr 即可），
/// 因为断网、GitHub 被墙都属于日常情况，弹窗只会制造噪音。
pub fn spawn_update_check(app: tauri::AppHandle) {
    use std::time::Duration;

    tauri::async_runtime::spawn(async move {
        // 让首屏把本地数据读完；更新不是启动链路的一部分。
        tokio::time::sleep(Duration::from_secs(15)).await;
        let settings = {
            let state = app.state::<SharedState>();
            let engine = state.engine.lock().await;
            engine.settings().update.clone()
        };
        if !settings.enabled {
            return;
        }
        match ph_update::check(ph_core::VERSION, &settings).await {
            Ok(Some(r)) => {
                use tauri_plugin_notification::NotificationExt;
                let _ = app
                    .notification()
                    .builder()
                    .title("积分枢纽 · 发现新版本")
                    .body(format!("v{} 已发布，打开「设置 → 更新」下载", r.version))
                    .show();
            }
            Ok(None) => {}
            Err(e) => eprintln!("更新检查失败：{}", e.user_message()),
        }
    });
}

#[cfg(test)]
mod single_instance_tests {
    #[test]
    fn plugin_registered_and_focuses_main_window() {
        let src = include_str!("lib.rs");
        let si = src.find("tauri_plugin_single_instance::init").expect("未注册单实例插件");
        let other = src.find("tauri_plugin_notification::init").unwrap();
        assert!(si < other, "单实例插件必须先于其它插件注册");
        assert!(
            src.contains("bring_main_window(app)"),
            "第二次启动没有唤起主窗口"
        );
    }
}

#[cfg(all(test, desktop, windows))]
mod icon_cache_tests {
    use super::shortcut_paths_from;
    use std::path::PathBuf;

    #[test]
    fn shortcuts_cover_both_desktops_and_start_menu() {
        let paths: Vec<PathBuf> = shortcut_paths_from(
            r"C:\Users\a".into(),
            r"C:\Users\Public".into(),
            r"C:\Users\a\AppData\Roaming".into(),
        );
        assert_eq!(paths.len(), 3);
        assert!(paths[0].ends_with(PathBuf::from(r"Desktop\PointHub.lnk")));
        assert!(paths[1].ends_with(PathBuf::from(r"Desktop\PointHub.lnk")));
        assert!(paths[2].ends_with(PathBuf::from(r"Start Menu\Programs\PointHub.lnk")));
    }
}
