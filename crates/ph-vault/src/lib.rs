//! 凭据加密仓。
//!
//! 设计约束（来自需求：令牌等同账号钥匙）：
//! 1. 令牌**只**以密文形态落盘，明文只在内存里存在极短时间；
//! 2. 加密密钥由操作系统的用户级密钥保护，程序自己不保管密钥；
//! 3. 换机 / 重装系统后密文必然解不开——这是设计意图，不是缺陷；
//! 4. 任何错误信息里都不得出现明文令牌。
//!
//! Windows 用 DPAPI（CryptProtectData，绑定当前用户），
//! 安卓用 Keystore（JNI 调 Kotlin，密钥不出系统密钥库），
//! 其它平台退回「本地密钥文件 + AES-256-GCM」，仅用于开发与自测。

use ph_core::{Credential, Error, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod sealer;
pub use sealer::SealerKind;

#[cfg(target_os = "android")]
pub mod keystore;

const MAGIC: &[u8; 4] = b"PHV1";

/// 加密仓：一个账号 id -> 凭据的映射，整体加密落盘。
pub struct Vault {
    path: PathBuf,
    /// 加密密钥文件（仅 AES 后端使用）。
    key_path: PathBuf,
    kind: SealerKind,
    cache: BTreeMap<String, Credential>,
    dirty: bool,
    /// 打开时密文解不开（系统密钥丢失）留下的说明，供上层写日志。
    notice: Option<String>,
}

impl Vault {
    /// 打开（或初始化）加密仓。文件不存在时视为空仓，不报错。
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let key_path = path.with_extension("key");
        let kind = SealerKind::for_current_platform();
        let mut vault = Self {
            path,
            key_path,
            kind,
            cache: BTreeMap::new(),
            dirty: false,
            notice: None,
        };
        vault.reload()?;
        // 旧版安卓的 AES 仓刚解开：立刻换成 Keystore 重封，并删掉旧的密钥文件。
        // 迁移失败不阻断启动——原文件仍可读，下次启动会再试一次。
        if vault.dirty && vault.flush().is_ok() && vault.kind == SealerKind::Keystore {
            let _ = std::fs::remove_file(&vault.key_path);
        }
        Ok(vault)
    }

    pub fn kind(&self) -> SealerKind {
        self.kind
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 本机密钥丢失导致旧密文解不开时，给上层写日志的一句话。
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn reload(&mut self) -> Result<()> {
        if !self.path.exists() {
            self.cache.clear();
            return Ok(());
        }
        let raw = std::fs::read(&self.path).map_err(|e| Error::Io(e.to_string()))?;
        if raw.len() < MAGIC.len() + 1 || &raw[..4] != MAGIC {
            return Err(Error::Format("凭据仓文件头损坏".into()));
        }
        // 第 5 字节是后端 id：若与当前平台不符，说明仓是从别的平台拷过来的。
        let stored = SealerKind::from_id(raw[4])
            .ok_or_else(|| Error::Format(format!("凭据仓使用了未知的加密后端（id={}）", raw[4])))?;
        let sealed = &raw[5..];
        let mut migrated = false;
        let plain = if stored == self.kind {
            match sealer::open(stored, &self.key_path, sealed) {
                Ok(p) => p,
                Err(e) => return self.handle_unreadable(e),
            }
        } else if self.kind.migrates_from(stored) {
            match sealer::open(stored, &self.key_path, sealed) {
                Ok(p) => {
                    migrated = true;
                    p
                }
                Err(e) => return self.handle_unreadable(e),
            }
        } else {
            return Err(Error::Invalid(format!(
                "凭据仓由 {} 加密，当前环境是 {}，无法解开。请在本机重新导入账号。",
                SealerKind::name_of(stored.id()),
                self.kind.name()
            )));
        };
        let map: BTreeMap<String, Credential> = serde_json::from_slice(&plain)
            .map_err(|e| Error::Format(format!("凭据仓内容无法解析：{e}")))?;
        self.cache = map;
        // 迁移时置脏，交给 open() 立即用新后端重封；平时读完就是干净的。
        self.dirty = migrated;
        Ok(())
    }

    /// 密文解不开时的处理。
    ///
    /// 安卓上系统密钥会随「卸载重装 / 清除数据 / 系统恢复」一起消失，
    /// 这时密文永远解不开。若直接报错，应用会卡在启动阶段打不开；
    /// 这里把原文件改名留档，按空仓继续——界面上账号会显示「缺凭据」，
    /// 用户重新导入即可。其它平台保持原有行为（报错）。
    #[cfg(target_os = "android")]
    fn handle_unreadable(&mut self, err: Error) -> Result<()> {
        match err {
            Error::Auth(_) => {
                let kept = self.path.with_extension("unreadable");
                let moved = std::fs::rename(&self.path, &kept);
                self.cache.clear();
                self.dirty = false;
                self.notice = Some(match moved {
                    Ok(()) => format!(
                        "凭据仓无法解密（{}），原文件已另存为 {}，请重新导入账号。",
                        err,
                        kept.display()
                    ),
                    Err(e) => format!("凭据仓无法解密（{err}），且原文件改名失败：{e}"),
                });
                Ok(())
            }
            other => Err(other),
        }
    }

    #[cfg(not(target_os = "android"))]
    fn handle_unreadable(&mut self, err: Error) -> Result<()> {
        Err(err)
    }

    pub fn get(&self, account_id: &str) -> Option<&Credential> {
        self.cache.get(account_id)
    }

    pub fn put(&mut self, account_id: &str, cred: Credential) {
        self.cache.insert(account_id.to_string(), cred);
        self.dirty = true;
    }

    pub fn remove(&mut self, account_id: &str) -> bool {
        let existed = self.cache.remove(account_id).is_some();
        if existed {
            self.dirty = true;
        }
        existed
    }

    pub fn ids(&self) -> Vec<String> {
        self.cache.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// 落盘。只有内容变化时才真正写。
    pub fn flush(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let plain = serde_json::to_vec(&self.cache)
            .map_err(|e| Error::Format(format!("凭据序列化失败：{e}")))?;
        let sealed = sealer::seal(self.kind, &self.key_path, &plain)?;
        let mut out = Vec::with_capacity(sealed.len() + 5);
        out.extend_from_slice(MAGIC);
        out.push(self.kind.id());
        out.extend_from_slice(&sealed);
        write_atomic(&self.path, &out)?;
        self.dirty = false;
        Ok(())
    }

    /// 彻底销毁（卸载时用）。先覆写再删除，降低被恢复的可能。
    pub fn destroy(self) -> Result<()> {
        if self.path.exists() {
            let len = std::fs::metadata(&self.path).map(|m| m.len() as usize).unwrap_or(0);
            let _ = std::fs::write(&self.path, vec![0u8; len]);
            std::fs::remove_file(&self.path).map_err(|e| Error::Io(e.to_string()))?;
        }
        if self.key_path.exists() {
            let _ = std::fs::remove_file(&self.key_path);
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
    // Windows 上 rename 不能覆盖已存在文件，先删再改；失败要能回退。
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| Error::Io(e.to_string()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| Error::Io(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ph-vault-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn roundtrip_persists_credentials() {
        let dir = tmpdir("rt");
        let path = dir.join("vault.bin");
        {
            let mut v = Vault::open(&path).unwrap();
            v.put("acct-1", Credential::from_refresh_token("secret-token-value"));
            v.flush().unwrap();
        }
        let v = Vault::open(&path).unwrap();
        let c = v.get("acct-1").expect("应当能取回");
        assert_eq!(c.refresh_token.as_deref(), Some("secret-token-value"));
        // 明文不得出现在密文文件里
        let raw = std::fs::read(&path).unwrap();
        let needle = b"secret-token-value";
        assert!(
            !raw.windows(needle.len()).any(|w| w == needle),
            "密文文件里不应出现明文令牌"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let dir = tmpdir("empty");
        let v = Vault::open(dir.join("nope.bin")).unwrap();
        assert!(v.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_header_is_rejected() {
        let dir = tmpdir("corrupt");
        let path = dir.join("vault.bin");
        std::fs::write(&path, b"XXXXbroken").unwrap();
        assert!(Vault::open(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_backend_id_is_rejected() {
        let dir = tmpdir("unknown-kind");
        let path = dir.join("vault.bin");
        let mut raw = MAGIC.to_vec();
        raw.push(9);
        raw.extend_from_slice(b"payload");
        std::fs::write(&path, raw).unwrap();
        let err = match Vault::open(&path) {
            Ok(_) => panic!("未知后端应当被拒绝"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("未知的加密后端"), "错误信息应说明后端未知：{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 桌面端打开安卓仓时必须明确告知「哪个后端、当前是什么」，而不是解密失败。
    #[cfg(not(target_os = "android"))]
    #[test]
    fn android_vault_is_named_in_mismatch_error() {
        let dir = tmpdir("android-kind");
        let path = dir.join("vault.bin");
        let mut raw = MAGIC.to_vec();
        raw.push(SealerKind::Keystore.id());
        raw.extend_from_slice(b"payload");
        std::fs::write(&path, raw).unwrap();
        let err = match Vault::open(&path) {
            Ok(_) => panic!("安卓仓在桌面端应当被拒绝"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("安卓 Keystore"), "错误信息应点名安卓 Keystore：{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
