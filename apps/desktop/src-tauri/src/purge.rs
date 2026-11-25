//! 卸载清理（仅桌面端 Windows）。
//!
//! NSIS 卸载器在删除程序文件前调用 `pointhub.exe --purge-data`
//! （用户勾选「保留账号数据」时调用 `--purge-app`）。这里不启动 Tauri、
//! 不初始化 WebView2，只做文件与注册表清理，所以可以在窗口创建前同步跑完并退出。
//!
//! 账号数据由 `ph_store::purge` 删（只删白名单文件）；本模块负责应用痕迹：
//! 自启项、开始菜单/桌面快捷方式、计划任务、WebView2 缓存。

#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// 子进程不弹控制台窗口：发布版是 GUI 子系统，但 schtasks/taskkill 会闪黑框。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Tauri 的 WebView2 用户数据目录名，必须与 tauri.conf.json 的 identifier 一致。
const WEBVIEW_ID: &str = "cn.pointhub.desktop";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 删除账号数据 + 应用痕迹（默认）。
    Data,
    /// 只删应用痕迹，保留账号数据（用户勾选「保留账号数据」）。
    AppOnly,
}

/// 执行清理并返回进程退出码。清理失败也返回 0：
/// 卸载器不该因为「某个文件删不掉」而中断整个卸载。
pub fn run(mode: Mode) -> i32 {
    let mut failures: Vec<String> = Vec::new();

    // 自启项无论是否保留数据都要删，否则下次开机会启动一个已经删掉的 exe。
    if let Err(e) = clean_autostart() {
        failures.push(format!("清理自启失败：{e}"));
    }

    // 快捷方式由卸载器自己删，这里是兜底（例如安装目录被用户改过）。
    remove_shortcuts(&mut failures);

    // 当前版本没有创建计划任务；只清理旧版本可能留下的同名任务。
    for task in ["PointHub", "积分枢纽 PointHub"] {
        let _ = delete_task(task);
    }

    // WebView2 缓存属于程序缓存，不存在「保留账号数据」的语义，两种模式都删。
    purge_cache_dirs(&webview_dirs(), &mut failures);

    if matches!(mode, Mode::Data) {
        let mut extra = Vec::new();
        // 兼容极早期版本把数据放在 Roaming 下的情况。
        if let Some(p) = std::env::var_os("APPDATA") {
            extra.push(PathBuf::from(p).join("PointHub"));
        }
        // 三个位置都要清：当前安装目录下的 data、旧固定目录、
        // 以及用户通过「更换数据目录」指到别处的那个目录。
        let dirs = ph_store::purge::data_dirs(
            &ph_store::locate::legacy_dir(),
            &ph_store::locate::default_dir(),
            &extra,
        );
        let report = ph_store::purge::purge_data_dirs(&dirs);
        for (p, e) in report.failed {
            failures.push(format!("删除 {} 失败：{e}", p.display()));
        }
    }

    if failures.is_empty() {
        0
    } else {
        // 发布版没有控制台；写一份临时日志，出问题时还能查。
        let log = std::env::temp_dir().join("pointhub-uninstall.log");
        let _ = std::fs::write(&log, failures.join("\r\n"));
        0
    }
}

/// 直接删整棵缓存目录（WebView2 profile），和账号数据白名单不同。
fn purge_cache_dirs(dirs: &[PathBuf], failures: &mut Vec<String>) {
    for p in dirs {
        if !p.exists() {
            continue;
        }
        if let Err(e) = std::fs::remove_dir_all(p) {
            failures.push(format!("删除 {} 失败：{e}", p.display()));
        }
    }
}

/// `%LOCALAPPDATA%\PointHub` 是默认数据目录，也是安装包的默认安装目录。
/// 这里只处理 WebView2 缓存目录，账号数据的删除交给 `ph_store::purge`。
fn webview_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        out.push(PathBuf::from(p).join(WEBVIEW_ID));
    }
    if let Some(p) = std::env::var_os("APPDATA") {
        out.push(PathBuf::from(p).join(WEBVIEW_ID));
    }
    out
}

const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_KEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// 删除自启注册表项（Run + 任务管理器里的启动批准记录）。
fn clean_autostart() -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();
    for path in [RUN_KEY, APPROVED_KEY] {
        // 必须带 write 权限：`Key::open` 默认只读，remove_value 会拿到「拒绝访问」。
        let key = match windows_registry::CURRENT_USER
            .options()
            .read()
            .write()
            .open(path)
        {
            Ok(k) => k,
            // 键不存在：没有可清理的。
            Err(_) => continue,
        };
        let mut names: Vec<String> = Vec::new();
        match key.values() {
            Ok(values) => {
                for (name, value) in values {
                    if is_ours(&name, &value) {
                        names.push(name);
                    }
                }
            }
            Err(e) => {
                errors.push(format!("枚举 {path} 失败：{e}"));
                continue;
            }
        }
        for name in names {
            if let Err(e) = key.remove_value(&name) {
                errors.push(format!("删除 {path}\\{name} 失败：{e}"));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// 值名或值内容里带 pointhub 的都算自己的。
///
/// 值名匹配用于正常情况（autostart 插件写入的是产品名）；
/// 值内容匹配用于旧版本或用户手动改过值名的情况。
fn is_ours(name: &str, value: &windows_registry::Value) -> bool {
    if name.to_lowercase().contains("pointhub") || name.contains("积分枢纽") {
        return true;
    }
    let text: Option<String> = value.clone().try_into().ok();
    text.map(|s| s.to_lowercase().contains("pointhub.exe"))
        .unwrap_or(false)
}

fn remove_shortcuts(failures: &mut Vec<String>) {
    let mut targets: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("APPDATA") {
        let programs = PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs");
        for name in ["PointHub.lnk", "积分枢纽 PointHub.lnk"] {
            targets.push(programs.join(name));
        }
        // 配了 startMenuFolder 时快捷方式在文件夹里。
        targets.push(programs.join("PointHub"));
    }
    if let Some(p) = std::env::var_os("USERPROFILE") {
        let desktop = PathBuf::from(p).join("Desktop");
        for name in ["PointHub.lnk", "积分枢纽 PointHub.lnk"] {
            targets.push(desktop.join(name));
        }
    }
    for t in targets {
        if !t.exists() {
            continue;
        }
        let res = if t.is_dir() {
            std::fs::remove_dir_all(&t)
        } else {
            std::fs::remove_file(&t)
        };
        if let Err(e) = res {
            failures.push(format!("删除快捷方式 {} 失败：{e}", t.display()));
        }
    }
}

/// 删除计划任务。返回是否真的删掉（任务不存在时 schtasks 返回非 0）。
fn delete_task(name: &str) -> bool {
    Command::new("schtasks")
        .args(["/Delete", "/TN", name, "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_own_autostart_values() {
        // windows_registry::Value 的字段是私有的，这里只测值名分支。
        let v = windows_registry::Value::from("C:\\other\\app.exe");
        assert!(is_ours("PointHub", &v));
        assert!(is_ours("积分枢纽 PointHub", &v));
        assert!(!is_ours("WeChat", &v));
        assert!(is_ours("Something", &windows_registry::Value::from(
            "C:\\Program Files\\PointHub\\pointhub.exe --autostart"
        )));
    }
}
