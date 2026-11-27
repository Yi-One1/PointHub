//! 明文数据层：账号元数据、设置、积分历史。
//!
//! 这里**不存任何令牌**——令牌在 `ph-vault`。
//! 所有写入都是「临时文件 + 改名」，避免断电时留下半个文件。

use ph_core::{files, Account, ActivityEvent, Error, Result, Settings, VendorSnapshot};
use std::path::{Path, PathBuf};

/// 数据目录定位。
///
/// 规则（对应需求「数据存放位置可自定义」）：
/// 1. 显式指定 > 指针文件 > 默认目录；
/// 2. 发行版的默认目录是 `<exe 所在目录>\data`，所以安装到 D 盘、便携版换位置，
///    数据都跟着程序走；调试版仍用 `%LOCALAPPDATA%\PointHub`，避免把开发时的
///    真实账号搬进 `target\debug`；
/// 3. 指针文件永远放在旧的固定位置 `%LOCALAPPDATA%\PointHub`，所以「改了目录」
///    这件事本身可被找回；
/// 4. 指针和显式路径都不可写时向上抛错，**不静默退回默认**——静默退回会让用户
///    以为改成功了。
pub mod locate {
    use super::*;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    pub(crate) const POINTER: &str = "location.json";
    /// 搬迁后留在新目录里的待清理标记，下次启动时删旧目录。
    pub(crate) const CLEANUP: &str = "relocate-cleanup.json";

    /// 旧版固定数据目录，也是指针文件永远所在的位置。
    pub fn legacy_dir() -> PathBuf {
        #[cfg(windows)]
        {
            if let Some(p) = std::env::var_os("LOCALAPPDATA") {
                return PathBuf::from(p).join("PointHub");
            }
            PathBuf::from(".\\PointHub")
        }
        #[cfg(target_os = "android")]
        {
            // 安卓上壳层会用 `app_data_dir()` 显式传进来，正常走不到这里。
            // 注意必须与 tauri.conf.json 的 identifier 一致，否则 create_dir_all 会失败。
            PathBuf::from("/data/data/cn.pointhub.desktop/files")
        }
        #[cfg(all(not(windows), not(target_os = "android")))]
        {
            if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
                return PathBuf::from(p).join("pointhub");
            }
            PathBuf::from("./PointHub")
        }
    }

    /// exe 同目录下的 `data`。
    pub fn data_dir_for_exe(exe: &Path) -> PathBuf {
        exe.parent()
            .unwrap_or_else(|| Path::new("."))
            .join("data")
    }

    /// 默认数据目录。
    ///
    /// 发行版跟着 exe 走；调试版保持旧位置，否则一键预览会把真实账号搬到构建目录。
    pub fn default_dir() -> PathBuf {
        // 一键预览通过环境变量把默认目录指到项目的 dist\data，避免显示成 C 盘；
        // 指针文件优先级更高，所以搬迁后仍然跟指针走。
        if let Some(p) = std::env::var_os("POINTHUB_DEFAULT_DATA_DIR") {
            let p = PathBuf::from(p);
            if !p.as_os_str().is_empty() {
                return p;
            }
        }
        #[cfg(all(not(debug_assertions), not(target_os = "android")))]
        {
            if let Ok(exe) = std::env::current_exe() {
                return data_dir_for_exe(&exe);
            }
        }
        legacy_dir()
    }

    /// 指针文件所在目录。
    ///
    /// 一键预览用 `POINTHUB_POINTER_DIR` 把它指到构建目录：预览里「更换数据目录」
    /// 只写预览自己的记录，不会覆盖正式版保存的位置；正式版也不会被预览影响。
    fn pointer_dir() -> PathBuf {
        if let Some(p) = std::env::var_os("POINTHUB_POINTER_DIR") {
            let p = PathBuf::from(p);
            if !p.as_os_str().is_empty() {
                return p;
            }
        }
        legacy_dir()
    }

    fn pointer_path() -> PathBuf {
        pointer_dir().join(POINTER)
    }

    /// 读取指针文件里记录的自定义目录。
    pub fn read_pointer() -> Option<PathBuf> {
        read_pointer_in(&pointer_dir())
    }

    /// 从指定目录读取指针文件。
    ///
    /// 单独开一个带参数的版本：卸载清理时旧目录可能马上要被删掉，
    /// 必须先把指针读出来；测试也不能依赖真实的 `%LOCALAPPDATA%`。
    pub fn read_pointer_in(dir: &Path) -> Option<PathBuf> {
        let raw = std::fs::read_to_string(dir.join(POINTER)).ok()?;
        let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
        v.get("data_dir")
            .and_then(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
    }

    /// 写入指针文件。传 None 表示恢复默认。
    pub fn write_pointer(dir: Option<&Path>) -> Result<()> {
        let p = pointer_path();
        match dir {
            None => {
                if p.exists() {
                    std::fs::remove_file(&p).map_err(|e| Error::Io(e.to_string()))?;
                }
            }
            Some(d) => {
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| Error::Io(e.to_string()))?;
                }
                let body = serde_json::json!({ "data_dir": d.to_string_lossy() });
                write_atomic(&p, serde_json::to_vec_pretty(&body).unwrap_or_default().as_slice())?;
            }
        }
        Ok(())
    }

    /// 记录「搬到新目录后，旧目录待清理」。
    pub fn write_cleanup_marker(dir: &Path, old: &Path) -> Result<()> {
        let body = serde_json::json!({ "old": old.to_string_lossy() });
        write_atomic(
            &dir.join(CLEANUP),
            &serde_json::to_vec(&body).map_err(|e| Error::Format(e.to_string()))?,
        )
    }

    /// 启动时执行待清理任务。`None` 表示没有待办。
    pub fn run_pending_cleanup(dir: &Path) -> Option<super::purge::Report> {
        run_pending_cleanup_at(&pointer_dir(), dir)
    }

    pub(crate) fn run_pending_cleanup_at(pointer_dir: &Path, dir: &Path) -> Option<super::purge::Report> {
        run_pending_cleanup_with(pointer_dir, dir, &default_dir())
    }

    /// `default` 用于识别「恢复默认目录」：此时指针已清空，但当前目录就是默认目录，
    /// 清理旧目录仍然安全。
    pub(crate) fn run_pending_cleanup_with(
        pointer_dir: &Path,
        dir: &Path,
        default: &Path,
    ) -> Option<super::purge::Report> {
        let marker = dir.join(CLEANUP);
        let raw = std::fs::read_to_string(&marker).ok()?;
        let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let old = PathBuf::from(v.get("old")?.as_str()?);
        if old == dir || old.parent().is_none() {
            let _ = std::fs::remove_file(&marker);
            return None;
        }
        // 指针已经指回旧目录（用户又搬回去了）时不能删；没有指针时只有当前
        // 目录就是默认目录才说明是「恢复默认」，否则不能碰。
        let pinned = read_pointer_in(pointer_dir);
        if pinned.as_deref() != Some(dir) && !(pinned.is_none() && dir == default) {
            return None;
        }
        let report = super::purge::purge_data_dirs(&[old.clone()]);
        // 旧目录正好是固定目录时，里面还放着指向新目录的指针，清理后补回去。
        if old == pointer_dir && read_pointer_in(pointer_dir).is_none() {
            let _ = write_pointer(Some(dir));
        }
        if report.failed.is_empty() {
            let _ = std::fs::remove_file(&marker);
        }
        Some(report)
    }

    /// 解析最终使用的数据目录（不创建）。
    pub fn resolve(explicit: Option<PathBuf>) -> PathBuf {
        if let Some(d) = explicit {
            return d;
        }
        if let Some(d) = read_pointer() {
            return d;
        }
        let target = default_dir();
        let legacy = legacy_dir();
        if target == legacy {
            return target;
        }
        // 预览的默认目录是显式指定的，不要把用户真实的旧目录搬进构建目录。
        if std::env::var_os("POINTHUB_DEFAULT_DATA_DIR").is_some() {
            return target;
        }
        match migrate_legacy(&legacy, &target) {
            Ok(_) => target,
            // 迁不动就先用旧目录，别让账号看起来「丢了」。
            Err(_) => legacy,
        }
    }

    /// 把旧固定目录里的数据搬到新目录；`Ok(false)` 表示没有需要搬的。
    ///
    /// 先复制到同盘的临时目录，再整体改名，成功后才删旧文件：
    /// 中途失败时旧数据仍然完整，下一次启动还能重试。
    pub(crate) fn migrate_legacy(legacy: &Path, target: &Path) -> Result<bool> {
        if target == legacy || has_data(target) || !has_data(legacy) {
            return Ok(false);
        }
        let parent = target.parent().ok_or_else(|| {
            Error::Io(format!("数据目录没有父目录：{}", target.display()))
        })?;
        let staging = parent.join(".point-hub-migrating");
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|e| Error::Io(e.to_string()))?;
        }
        std::fs::create_dir_all(&staging).map_err(|e| Error::Io(e.to_string()))?;

        let mut moved: Vec<std::ffi::OsString> = Vec::new();
        let entries = std::fs::read_dir(legacy).map_err(|e| Error::Io(e.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::Io(e.to_string()))?;
            let name = entry.file_name();
            if skip_migration_entry(&name) {
                continue;
            }
            let src = entry.path();
            let dst = staging.join(&name);
            let copied = if src.is_dir() {
                copy_recursive(&src, &dst)
            } else {
                std::fs::copy(&src, &dst)
                    .map(|_| ())
                    .map_err(|e| Error::Io(e.to_string()))
            };
            if let Err(e) = copied {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(e);
            }
            moved.push(name);
        }
        if moved.is_empty() {
            let _ = std::fs::remove_dir_all(&staging);
            return Ok(false);
        }

        if target.exists() {
            let empty = std::fs::read_dir(target)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false);
            if !empty {
                let _ = std::fs::remove_dir_all(&staging);
                return Ok(false);
            }
            std::fs::remove_dir(target).map_err(|e| Error::Io(e.to_string()))?;
        }
        std::fs::rename(&staging, target).map_err(|e| Error::Io(e.to_string()))?;

        // 新位置已经落盘，再清旧文件；清理失败只会留下重复文件，不影响使用。
        for name in &moved {
            let p = legacy.join(name);
            if p.is_dir() {
                let _ = std::fs::remove_dir_all(&p);
            } else {
                let _ = std::fs::remove_file(&p);
            }
        }
        let _ = std::fs::remove_file(legacy.join(POINTER));
        // 极早期尝试留下的 data 子目录；默认安装时 target 就是它，不能删。
        if target != legacy.join("data") {
            let _ = std::fs::remove_dir_all(legacy.join("data"));
        }
        let _ = std::fs::remove_dir(legacy);
        Ok(true)
    }

    /// 迁移时跳过程序文件：默认安装目录可能和旧数据目录是同一层。
    fn skip_migration_entry(name: &OsStr) -> bool {
        let n = name.to_string_lossy().to_ascii_lowercase();
        n == POINTER
            || n == "pointhub.exe"
            || n == "uninstall.exe"
            || n.ends_with(".dll")
            || n == "resources"
            || n == "data"
            || n == ".point-hub-migrating"
    }

    fn has_data(dir: &Path) -> bool {
        [
            files::ACCOUNTS,
            files::SETTINGS,
            files::VAULT,
            files::HISTORY,
            files::EVENTS,
        ]
        .iter()
        .any(|n| dir.join(n).is_file())
    }

    fn copy_recursive(src: &Path, dst: &Path) -> Result<()> {
        std::fs::create_dir_all(dst).map_err(|e| Error::Io(e.to_string()))?;
        let entries = std::fs::read_dir(src).map_err(|e| Error::Io(e.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::Io(e.to_string()))?;
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if from.is_dir() {
                copy_recursive(&from, &to)?;
            } else {
                std::fs::copy(&from, &to).map_err(|e| Error::Io(e.to_string()))?;
            }
        }
        Ok(())
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::Io(e.to_string()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data).map_err(|e| Error::Io(e.to_string()))?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| Error::Io(e.to_string()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| Error::Io(e.to_string()))?;
    Ok(())
}

/// 卸载清理。
///
/// 安装包的 NSIS 钩子会在删除程序文件前调用 `pointhub.exe --purge-data`，
/// 这里负责判断「要删哪些目录、目录里哪些文件属于账号数据」。
///
/// 只删白名单里的数据文件，不做整目录 `remove_dir_all`：
/// 数据目录可能被用户改成 `D:\` 之类的自定义位置，也可能和安装目录相邻，
/// 白名单能保证账号文件删干净、无关文件（甚至程序文件）不误伤。
pub mod purge {
    use super::*;
    use std::path::{Path, PathBuf};

    #[derive(Debug, Default)]
    pub struct Report {
        /// 成功删掉的路径数（文件/子目录都算一个）。
        pub removed: usize,
        pub failed: Vec<(PathBuf, String)>,
    }

    /// 要清理的数据目录，顺序固定：指针指向的自定义目录、当前默认目录、
    /// 旧固定目录、最后是额外目录。
    ///
    /// 必须在旧目录还在时读指针；把旧目录删了再读就找不到自定义位置了。
    pub fn data_dirs(pointer_dir: &Path, default_dir: &Path, extra: &[PathBuf]) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        // 盘符根目录/空路径不能删：自定义目录可能被填成 `D:\`。
        if let Some(custom) = super::locate::read_pointer_in(pointer_dir) {
            push_unique(&mut out, custom);
        }
        push_unique(&mut out, default_dir.to_path_buf());
        push_unique(&mut out, pointer_dir.to_path_buf());
        for e in extra {
            push_unique(&mut out, e.clone());
        }
        out
    }

    fn push_unique(out: &mut Vec<PathBuf>, p: PathBuf) {
        if !safe_target(&p) || out.contains(&p) {
            return;
        }
        out.push(p);
    }

    fn safe_target(p: &Path) -> bool {
        !p.as_os_str().is_empty() && p.parent().is_some()
    }

    /// 删除数据目录里的账号数据；未知文件一律保留。
    ///
    /// 幂等：目录/文件不存在时直接跳过，重复调用不会报错。
    pub fn purge_data_dirs(dirs: &[PathBuf]) -> Report {
        let mut r = Report::default();
        for dir in dirs {
            purge_one(dir, &mut r);
        }
        r
    }

    fn purge_one(dir: &Path, r: &mut Report) {
        if !dir.exists() {
            return;
        }
        for name in DATA_FILES {
            let p = dir.join(name);
            if !p.exists() {
                continue;
            }
            match std::fs::remove_file(&p) {
                Ok(()) => r.removed += 1,
                Err(e) => r.failed.push((p, e.to_string())),
            }
        }
        for name in DATA_DIRS {
            let p = dir.join(name);
            if !p.exists() {
                continue;
            }
            match std::fs::remove_dir_all(&p) {
                Ok(()) => r.removed += 1,
                Err(e) => r.failed.push((p, e.to_string())),
            }
        }
        // 历史文件的滚动备份（history.jsonl.bak-20261125 之类）不在固定名单里。
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !DATA_BACKUP_PREFIXES.iter().any(|p| name.starts_with(p)) {
                    continue;
                }
                let p = entry.path();
                if !p.is_file() {
                    continue;
                }
                match std::fs::remove_file(&p) {
                    Ok(()) => r.removed += 1,
                    Err(e) => r.failed.push((p, e.to_string())),
                }
            }
        }
        // 只删空目录：默认目录同时也是安装目录时，exe 还在，这里会失败且不应算错。
        let _ = std::fs::remove_dir(dir);
    }

    /// 数据文件的滚动备份前缀，卸载时一并清掉，避免留下半份历史。
    const DATA_BACKUP_PREFIXES: &[&str] = &[
        "history.jsonl.bak",
        "events.jsonl.bak",
        "accounts.json.bak",
        "settings.json.bak",
    ];

    /// 账号数据的文件名。新增数据文件时记得同步这里，否则卸载会留下它。
    const DATA_FILES: &[&str] = &[
        files::ACCOUNTS,
        files::SETTINGS,
        files::VAULT,
        files::HISTORY,
        files::EVENTS,
        files::DEVICE,
        files::DEVICES,
        files::TOMBSTONES,
        files::CLIENT,
        locate::POINTER,
        locate::CLEANUP,
        // 设置损坏时另存的文件，以及原子写的中间文件。
        "settings.broken.json",
        "settings.tmp",
        "location.tmp",
        // 图标缓存版本标记，删了下次启动会重新生成。
        ".icons-version",
    ];

    /// 账号数据的子目录。
    const DATA_DIRS: &[&str] = &[
        files::LOGS_DIR,
        files::UPDATES_DIR,
        // 浏览器登录用过的 profile（旧版本/手工创建），里面可能留登录态。
        "browser-profile",
    ];
}

/// 数据仓。
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// 打开数据仓，目录不存在时自动创建。
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root).map_err(|e| {
            Error::Io(format!("无法创建数据目录 {}：{e}", root.display()))
        })?;
        let logs = root.join(files::LOGS_DIR);
        std::fs::create_dir_all(&logs).map_err(|e| Error::Io(e.to_string()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_of(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    // ------------------------------------------------------------ 设置

    pub fn load_settings(&self) -> Result<Settings> {
        let p = self.path_of(files::SETTINGS);
        if !p.exists() {
            return Ok(Settings::default());
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| Error::Io(e.to_string()))?;
        if raw.trim().is_empty() {
            return Ok(Settings::default());
        }
        // 设置文件损坏不应让整个程序打不开：退回默认并保留现场。
        match serde_json::from_str::<Settings>(&raw) {
            Ok(mut s) => {
                // 老版本的 settings.json 里镜像前缀是空串，补成默认地址。
                if s.update.mirror.trim().is_empty() {
                    s.update.mirror = ph_core::settings::default_mirror();
                }
                Ok(s)
            }
            Err(e) => {
                let _ = std::fs::rename(&p, p.with_extension("broken.json"));
                Err(Error::Format(format!(
                    "settings.json 无法解析（{e}），已另存为 settings.broken.json 并恢复默认"
                )))
            }
        }
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let body = serde_json::to_vec_pretty(s)
            .map_err(|e| Error::Format(format!("设置序列化失败：{e}")))?;
        write_atomic(&self.path_of(files::SETTINGS), &body)
    }

    // ------------------------------------------------------------ 账号

    pub fn load_accounts(&self) -> Result<Vec<Account>> {
        let p = self.path_of(files::ACCOUNTS);
        if !p.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| Error::Io(e.to_string()))?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str::<Vec<Account>>(&raw)
            .map_err(|e| Error::Format(format!("accounts.json 无法解析：{e}")))
    }

    pub fn save_accounts(&self, accounts: &[Account]) -> Result<()> {
        let body = serde_json::to_vec_pretty(accounts)
            .map_err(|e| Error::Format(format!("账号序列化失败：{e}")))?;
        write_atomic(&self.path_of(files::ACCOUNTS), &body)
    }

    // ------------------------------------------------------------ 历史

    /// 追加一条快照。JSONL 追加写，不会因并发读而损坏已有内容。
    pub fn append_snapshot(&self, snap: &VendorSnapshot) -> Result<()> {
        use std::io::Write;
        let p = self.path_of(files::HISTORY);
        let line = serde_json::to_string(snap)
            .map_err(|e| Error::Format(format!("快照序列化失败：{e}")))?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .map_err(|e| Error::Io(e.to_string()))?;
        writeln!(f, "{line}").map_err(|e| Error::Io(e.to_string()))?;
        Ok(())
    }

    /// 读取最近 N 条快照（按文件顺序，不做时间排序——文件本身就是时序的）。
    pub fn load_snapshots(&self, limit: usize) -> Result<Vec<VendorSnapshot>> {
        let p = self.path_of(files::HISTORY);
        if !p.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| Error::Io(e.to_string()))?;
        let mut out: Vec<VendorSnapshot> = Vec::new();
        // 倒着取最后 limit 行，避免把几年历史全读进内存。
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        let start = lines.len().saturating_sub(limit);
        for line in &lines[start..] {
            if let Ok(s) = serde_json::from_str::<VendorSnapshot>(line) {
                out.push(s);
            }
        }
        Ok(out)
    }

    /// 每个账号的最后一条快照。
    pub fn latest_by_account(&self) -> Result<std::collections::BTreeMap<String, VendorSnapshot>> {
        let all = self.load_snapshots(4000)?;
        let mut map = std::collections::BTreeMap::new();
        for s in all {
            map.insert(s.account_id.clone(), s);
        }
        Ok(map)
    }

    /// 追加一条活动流水（签到 / 派猫猫）。同样是 JSONL 追加写。
    pub fn append_event(&self, ev: &ActivityEvent) -> Result<()> {
        use std::io::Write;
        let p = self.path_of(files::EVENTS);
        let line = serde_json::to_string(ev)
            .map_err(|e| Error::Format(format!("活动序列化失败：{e}")))?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .map_err(|e| Error::Io(e.to_string()))?;
        writeln!(f, "{line}").map_err(|e| Error::Io(e.to_string()))?;
        Ok(())
    }

    /// 读取最近 N 条活动（按时间倒序，最新在前）。
    ///
    /// 排序不依赖文件顺序：追加写和外部写入混在一起时，文件顺序不等于时间顺序。
    pub fn load_events(&self, limit: usize) -> Result<Vec<ActivityEvent>> {
        let p = self.path_of(files::EVENTS);
        if !p.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| Error::Io(e.to_string()))?;
        // 全量读后排序会随文件增长变慢，这里只保留末尾 limit*4 行做候选。
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        let start = lines.len().saturating_sub(limit.saturating_mul(4).max(limit));
        let mut out: Vec<ActivityEvent> = lines[start..]
            .iter()
            .filter_map(|l| serde_json::from_str::<ActivityEvent>(l).ok())
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at));
        out.truncate(limit);
        Ok(out)
    }

    /// 追加一行运行日志（给用户看的那种，不含令牌）。
    pub fn log(&self, line: &str) {
        use std::io::Write;
        let p = self
            .path_of(files::LOGS_DIR)
            .join(format!("{}.log", chrono::Local::now().format("%Y-%m-%d")));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(f, "[{ts}] {line}");
        }
    }

    /// 历史文件是否超过给定行数（用于提示体积）。
    pub fn history_line_count(&self) -> usize {
        std::fs::read_to_string(self.path_of(files::HISTORY))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    // ------------------------------------------------------------ 本机身份与配对设备

    /// 本机身份。首次使用时由引擎创建后写入。
    pub fn load_device(&self) -> Result<Option<ph_core::DeviceInfo>> {
        let p = self.path_of(files::DEVICE);
        if !p.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| Error::Io(e.to_string()))?;
        if raw.trim().is_empty() {
            return Ok(None);
        }
        serde_json::from_str(&raw).map(Some).map_err(|e| {
            Error::Format(format!("device.json 无法解析：{e}"))
        })
    }

    pub fn save_device(&self, d: &ph_core::DeviceInfo) -> Result<()> {
        let body = serde_json::to_vec_pretty(d)
            .map_err(|e| Error::Format(format!("设备信息序列化失败：{e}")))?;
        write_atomic(&self.path_of(files::DEVICE), &body)
    }

    /// 已配对设备清单（本机视角）。
    pub fn load_devices(&self) -> Result<Vec<ph_core::DeviceInfo>> {
        let p = self.path_of(files::DEVICES);
        if !p.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| Error::Io(e.to_string()))?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw).map_err(|e| {
            Error::Format(format!("devices.json 无法解析：{e}"))
        })
    }

    pub fn save_devices(&self, v: &[ph_core::DeviceInfo]) -> Result<()> {
        let body = serde_json::to_vec_pretty(v)
            .map_err(|e| Error::Format(format!("设备清单序列化失败：{e}")))?;
        write_atomic(&self.path_of(files::DEVICES), &body)
    }

    /// 按 id 覆盖或追加一台设备。
    pub fn upsert_device(&self, d: &ph_core::DeviceInfo) -> Result<()> {
        let mut all = self.load_devices()?;
        match all.iter_mut().find(|x| x.id == d.id) {
            Some(slot) => *slot = d.clone(),
            None => all.push(d.clone()),
        }
        self.save_devices(&all)
    }

    /// 撤销配对。返回是否真的删掉了一台设备。
    pub fn remove_device(&self, id: &str) -> Result<bool> {
        let mut all = self.load_devices()?;
        let before = all.len();
        all.retain(|d| d.id != id);
        if all.len() == before {
            return Ok(false);
        }
        self.save_devices(&all)?;
        Ok(true)
    }

    /// 记录一次「这台设备来过」，用于界面展示最后在线时间。
    pub fn touch_device(&self, id: &str, at: i64) -> Result<()> {
        let mut all = self.load_devices()?;
        let Some(d) = all.iter_mut().find(|x| x.id == id) else {
            return Ok(());
        };
        d.last_seen_at = Some(at);
        self.save_devices(&all)
    }

    // ------------------------------------------------------------ 删除标记

    pub fn load_tombstones(&self) -> Result<Vec<ph_core::Tombstone>> {
        let p = self.path_of(files::TOMBSTONES);
        if !p.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| Error::Io(e.to_string()))?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw).map_err(|e| {
            Error::Format(format!("tombstones.json 无法解析：{e}"))
        })
    }

    pub fn save_tombstones(&self, v: &[ph_core::Tombstone]) -> Result<()> {
        let body = serde_json::to_vec_pretty(v)
            .map_err(|e| Error::Format(format!("删除标记序列化失败：{e}")))?;
        write_atomic(&self.path_of(files::TOMBSTONES), &body)
    }

    /// 记下一条删除标记；同一个账号只保留最新的一条。
    pub fn add_tombstone(&self, t: &ph_core::Tombstone) -> Result<()> {
        let mut all = self.load_tombstones()?;
        match all.iter_mut().find(|x| x.account_id == t.account_id) {
            Some(slot) => {
                if t.deleted_at > slot.deleted_at {
                    *slot = t.clone();
                }
            }
            None => all.push(t.clone()),
        }
        self.save_tombstones(&all)
    }

    /// 账号被重新导入时清掉它的删除标记。
    pub fn clear_tombstone(&self, account_id: &str) -> Result<()> {
        let mut all = self.load_tombstones()?;
        let before = all.len();
        all.retain(|t| t.account_id != account_id);
        if all.len() != before {
            self.save_tombstones(&all)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ 同步合并

    /// 全量快照（同步去重用；日常取数请用 `load_snapshots`）。
    pub fn load_all_snapshots(&self) -> Result<Vec<VendorSnapshot>> {
        let p = self.path_of(files::HISTORY);
        if !p.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| Error::Io(e.to_string()))?;
        Ok(raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<VendorSnapshot>(l).ok())
            .collect())
    }

    pub fn load_all_events(&self) -> Result<Vec<ActivityEvent>> {
        let p = self.path_of(files::EVENTS);
        if !p.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| Error::Io(e.to_string()))?;
        Ok(raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<ActivityEvent>(l).ok())
            .collect())
    }

    /// 合并外部快照。
    ///
    /// 去重键是「账号 + 采样时刻」：同一账号同一秒只会有一个采样，
    /// 重复推送同一条是幂等的，不会把历史越滚越长。
    pub fn merge_snapshots(&self, snaps: &[VendorSnapshot]) -> Result<usize> {
        if snaps.is_empty() {
            return Ok(0);
        }
        let mut seen: std::collections::HashSet<String> = self
            .load_all_snapshots()?
            .into_iter()
            .map(|s| format!("{}@{}", s.account_id, s.captured_at))
            .collect();
        let mut added = 0;
        for s in snaps {
            let key = format!("{}@{}", s.account_id, s.captured_at);
            if seen.insert(key) {
                self.append_snapshot(s)?;
                added += 1;
            }
        }
        Ok(added)
    }

    /// 合并外部活动流水。去重键：账号 + 时刻 + 类型 + 文案。
    pub fn merge_events(&self, evs: &[ActivityEvent]) -> Result<usize> {
        if evs.is_empty() {
            return Ok(0);
        }
        let mut seen: std::collections::HashSet<String> = self
            .load_all_events()?
            .into_iter()
            .map(|e| format!("{}@{}#{}#{}", e.account_id, e.at, e.kind, e.text))
            .collect();
        let mut added = 0;
        for e in evs {
            let key = format!("{}@{}#{}#{}", e.account_id, e.at, e.kind, e.text);
            if seen.insert(key) {
                self.append_event(e)?;
                added += 1;
            }
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ph_core::Vendor;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ph-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn accounts_roundtrip() {
        let dir = tmpdir("acct");
        let s = Store::open(&dir).unwrap();
        assert!(s.load_accounts().unwrap().is_empty());

        let mut a = Account::new(Vendor::WorkBuddy, "主号", "abc...1234");
        a.travel_enabled = true;
        s.save_accounts(std::slice::from_ref(&a)).unwrap();

        let back = s.load_accounts().unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].label, "主号");
        assert_eq!(back[0].vendor, Vendor::WorkBuddy);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshots_append_and_latest() {
        let dir = tmpdir("snap");
        let s = Store::open(&dir).unwrap();
        let a = Account::new(Vendor::WorkBuddy, "a", "h");
        let mut snap = VendorSnapshot::empty(&a, 100);
        snap.balance = Some(1000.0);
        s.append_snapshot(&snap).unwrap();
        snap.captured_at = 200;
        snap.balance = Some(1100.0);
        s.append_snapshot(&snap).unwrap();

        let latest = s.latest_by_account().unwrap();
        assert_eq!(latest.get(&a.id).unwrap().balance, Some(1100.0));
        assert_eq!(s.history_line_count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_default_when_missing() {
        let dir = tmpdir("settings");
        let s = Store::open(&dir).unwrap();
        let st = s.load_settings().unwrap();
        assert!(st.scheduler_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_is_idempotent() {
        let dir = tmpdir("merge");
        let s = Store::open(&dir).unwrap();
        let a = Account::new(Vendor::TraeCn, "t", "h");
        let mut snap = VendorSnapshot::empty(&a, 500);
        snap.balance = Some(12.5);
        let ev = ph_core::ActivityEvent::checkin(&a, 500, "已领取", Some(10));

        assert_eq!(s.merge_snapshots(std::slice::from_ref(&snap)).unwrap(), 1);
        assert_eq!(s.merge_snapshots(std::slice::from_ref(&snap)).unwrap(), 0);
        assert_eq!(s.merge_events(std::slice::from_ref(&ev)).unwrap(), 1);
        assert_eq!(s.merge_events(std::slice::from_ref(&ev)).unwrap(), 0);
        assert_eq!(s.history_line_count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn devices_and_tombstones_roundtrip() {
        let dir = tmpdir("devices");
        let s = Store::open(&dir).unwrap();
        assert!(s.load_device().unwrap().is_none());

        let me = ph_core::DeviceInfo::new("书房电脑", "pc");
        s.save_device(&me).unwrap();
        assert_eq!(s.load_device().unwrap().unwrap().id, me.id);

        let mut phone = ph_core::DeviceInfo::new("Pixel", "android");
        phone.token_hash = Some(ph_core::DeviceInfo::hash_token("abc"));
        s.upsert_device(&phone).unwrap();
        assert_eq!(s.load_devices().unwrap().len(), 1);
        assert!(s.remove_device(&phone.id).unwrap());
        assert!(!s.remove_device(&phone.id).unwrap());

        let t = ph_core::Tombstone {
            account_id: "a1".into(),
            deleted_at: 100,
            device_id: me.id.clone(),
        };
        s.add_tombstone(&t).unwrap();
        assert_eq!(s.load_tombstones().unwrap().len(), 1);
        s.clear_tombstone("a1").unwrap();
        assert!(s.load_tombstones().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn purge_removes_only_data_and_is_idempotent() {
        let default = tmpdir("purge-default");
        let custom = tmpdir("purge-custom");

        // 模拟「安装目录 = 默认数据目录」：程序文件和数据文件混在一起。
        std::fs::write(default.join("accounts.json"), "{}").unwrap();
        std::fs::write(default.join("settings.broken.json"), "{}").unwrap();
        std::fs::write(default.join("history.jsonl.bak-20261125"), "x").unwrap();
        std::fs::write(default.join("pointhub.exe"), "binary").unwrap();
        std::fs::create_dir(default.join("logs")).unwrap();
        std::fs::write(default.join("logs/a.log"), "x").unwrap();
        let pointer = serde_json::json!({ "data_dir": custom }).to_string();
        std::fs::write(default.join("location.json"), pointer).unwrap();

        std::fs::write(custom.join("history.jsonl"), "x").unwrap();
        std::fs::write(custom.join("notes.txt"), "用户的其它文件").unwrap();

        let dirs = purge::data_dirs(&default, &default, &[]);
        assert_eq!(dirs.len(), 2, "默认目录与指针目录都要清");
        assert_eq!(dirs[0], custom, "先清自定义目录，再清默认目录");

        let r = purge::purge_data_dirs(&dirs);
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(!default.join("accounts.json").exists());
        assert!(!default.join("settings.broken.json").exists());
        assert!(!default.join("history.jsonl.bak-20261125").exists());
        assert!(!default.join("location.json").exists());
        assert!(!default.join("logs").exists());
        // 程序文件不归这里删：卸载器还要用它清理，删了会删到正在运行的自己。
        assert!(default.join("pointhub.exe").exists());

        assert!(!custom.join("history.jsonl").exists());
        // 未知文件保守保留，避免用户自定义目录里误删无关内容。
        assert!(custom.join("notes.txt").exists());

        // 幂等：第二次没有可删的东西，也不报错。
        let again = purge::purge_data_dirs(&purge::data_dirs(&default, &default, &[]));
        assert!(again.failed.is_empty());
        assert_eq!(again.removed, 0);

        let _ = std::fs::remove_dir_all(&default);
        let _ = std::fs::remove_dir_all(&custom);
    }

    #[test]
    fn pending_cleanup_removes_old_dir_after_relocation() {
        let base = tmpdir("relocate-cleanup");
        let old = base.join("old");
        let target = base.join("target");
        let pointer = base.join("pointer");
        std::fs::create_dir_all(old.join("logs")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&pointer).unwrap();
        std::fs::write(old.join(files::ACCOUNTS), "{}").unwrap();
        std::fs::write(old.join("logs").join("a.log"), "x").unwrap();

        let ptr = serde_json::json!({ "data_dir": target.to_string_lossy() }).to_string();
        std::fs::write(pointer.join(locate::POINTER), ptr).unwrap();
        locate::write_cleanup_marker(&target, &old).unwrap();

        let report = locate::run_pending_cleanup_at(&pointer, &target).expect("应有待清理任务");
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert!(!old.join(files::ACCOUNTS).exists());
        assert!(!old.exists(), "旧目录应被删除");
        assert!(
            !target.join(locate::CLEANUP).exists(),
            "清理完成后标记应移除"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cleanup_after_reset_to_default_needs_no_pointer() {
        let base = tmpdir("reset-cleanup");
        let old = base.join("custom");
        let default = base.join("default");
        let pointer = base.join("pointer");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&default).unwrap();
        std::fs::create_dir_all(&pointer).unwrap();
        std::fs::write(old.join(files::ACCOUNTS), "{}").unwrap();
        locate::write_cleanup_marker(&default, &old).unwrap();

        // 指针已清空且当前就是默认目录：这是「恢复默认目录」，可以清理旧目录。
        let report = locate::run_pending_cleanup_with(&pointer, &default, &default)
            .expect("恢复默认后应执行清理");
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert!(!old.exists(), "旧目录应被删除");

        // 没有指针、当前也不是默认目录时不能动：可能只是指针丢了。
        let other = base.join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join(files::ACCOUNTS), "{}").unwrap();
        locate::write_cleanup_marker(&other, &old).unwrap();
        assert!(locate::run_pending_cleanup_with(&pointer, &other, &default).is_none());
        assert!(other.join(files::ACCOUNTS).exists(), "数据不能被误删");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn old_empty_mirror_gets_default() {
        let dir = tmpdir("mirror-default");
        let store = Store::open(&dir).unwrap();
        std::fs::write(dir.join(files::SETTINGS), r#"{"update":{"mirror":""}}"#).unwrap();
        let s = store.load_settings().unwrap();
        assert_eq!(s.update.mirror, "https://ghfast.top/");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn purge_refuses_drive_root_and_dedups() {
        let root = std::path::Path::new(if cfg!(windows) { "C:\\" } else { "/" });
        assert!(
            purge::data_dirs(root, root, &[]).is_empty(),
            "盘符根目录不能作为清理目标"
        );

        let dir = tmpdir("purge-dedup");
        let extra = vec![dir.clone(), dir.clone()];
        let dirs = purge::data_dirs(&dir, &dir, &extra);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0], dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn data_dir_follows_exe() {
        let dir = tmpdir("exe-dir");
        let exe = dir.join("pointhub.exe");
        assert_eq!(locate::data_dir_for_exe(&exe), dir.join("data"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_legacy_data_and_keeps_program_files() {
        let base = tmpdir("migrate");
        let legacy = base.join("legacy");
        let target = base.join("install").join("data");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("accounts.json"), "{}").unwrap();
        std::fs::write(legacy.join("vault.bin"), "v").unwrap();
        std::fs::create_dir_all(legacy.join("logs")).unwrap();
        std::fs::write(legacy.join("logs/a.log"), "x").unwrap();
        std::fs::create_dir_all(legacy.join("browser-profile")).unwrap();
        std::fs::write(legacy.join("browser-profile/cookie"), "c").unwrap();
        std::fs::write(legacy.join("history.jsonl.bak-1"), "h").unwrap();
        std::fs::write(legacy.join("pointhub.exe"), "binary").unwrap();
        std::fs::write(legacy.join("location.json"), "broken").unwrap();

        assert!(locate::migrate_legacy(&legacy, &target).unwrap());
        assert!(target.join("accounts.json").is_file());
        assert!(target.join("vault.bin").is_file());
        assert!(target.join("logs/a.log").is_file());
        assert!(target.join("browser-profile/cookie").is_file());
        assert!(target.join("history.jsonl.bak-1").is_file());
        assert!(!target.join("pointhub.exe").exists());
        assert!(!target.join("location.json").exists());

        assert!(!legacy.join("accounts.json").exists());
        assert!(!legacy.join("location.json").exists());
        assert!(legacy.join("pointhub.exe").is_file(), "程序文件必须留在原处");

        // 第二次没有可搬的，直接返回 false。
        assert!(!locate::migrate_legacy(&legacy, &target).unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrates_into_data_subdir_when_exe_is_in_legacy_dir() {
        let base = tmpdir("migrate-subdir");
        let legacy = base.join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("accounts.json"), "{}").unwrap();
        std::fs::write(legacy.join("pointhub.exe"), "exe").unwrap();
        let target = legacy.join("data");

        assert!(locate::migrate_legacy(&legacy, &target).unwrap());
        assert!(target.join("accounts.json").is_file());
        assert!(legacy.join("pointhub.exe").is_file());
        assert!(!legacy.join("accounts.json").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migration_does_not_overwrite_existing_target() {
        let base = tmpdir("migrate-existing");
        let legacy = base.join("legacy");
        let target = base.join("data");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(legacy.join("accounts.json"), "old").unwrap();
        std::fs::write(target.join("accounts.json"), "new").unwrap();

        assert!(!locate::migrate_legacy(&legacy, &target).unwrap());
        assert_eq!(
            std::fs::read_to_string(target.join("accounts.json")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(legacy.join("accounts.json")).unwrap(),
            "old"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
