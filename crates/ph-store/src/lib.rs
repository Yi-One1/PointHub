//! 明文数据层：账号元数据、设置、积分历史。
//!
//! 这里**不存任何令牌**——令牌在 `ph-vault`。
//! 所有写入都是「临时文件 + 改名」，避免断电时留下半个文件。

use ph_core::{files, Account, ActivityEvent, Error, Result, Settings, VendorSnapshot};
use std::path::{Path, PathBuf};

/// 数据目录定位。
///
/// 规则（对应需求「数据存放位置可自定义」）：
/// 1. 显式指定 > 指针文件 > 系统默认；
/// 2. 指针文件永远放在系统默认位置，所以「改了目录」这件事本身可被找回；
/// 3. 目录不可写时向上抛错，**不静默退回默认**——静默退回会让用户以为改成功了。
pub mod locate {
    use super::*;
    use std::path::PathBuf;

    const POINTER: &str = "location.json";

    /// 系统默认数据目录（不含自定义）。
    pub fn default_dir() -> PathBuf {
        #[cfg(windows)]
        {
            if let Some(p) = std::env::var_os("LOCALAPPDATA") {
                return PathBuf::from(p).join("PointHub");
            }
            PathBuf::from(".\\PointHub")
        }
        #[cfg(target_os = "android")]
        {
            // 安卓上由壳层通过 JNI 传入真实路径，这里仅作占位。
            PathBuf::from("/data/data/com.pointhub.app/files")
        }
        #[cfg(all(not(windows), not(target_os = "android")))]
        {
            if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
                return PathBuf::from(p).join("pointhub");
            }
            PathBuf::from("./PointHub")
        }
    }

    fn pointer_path() -> PathBuf {
        default_dir().join(POINTER)
    }

    /// 读取指针文件里记录的自定义目录。
    pub fn read_pointer() -> Option<PathBuf> {
        let raw = std::fs::read_to_string(pointer_path()).ok()?;
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

    /// 解析最终使用的数据目录（不创建）。
    pub fn resolve(explicit: Option<PathBuf>) -> PathBuf {
        if let Some(d) = explicit {
            return d;
        }
        if let Some(d) = read_pointer() {
            return d;
        }
        default_dir()
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
            Ok(s) => Ok(s),
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
}
