use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use ph_core::{Error, Result};
use rand::RngCore;
use std::path::Path;

/// 加密后端标识。写进凭据仓文件头，防止跨平台误开。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealerKind {
    /// Windows DPAPI，绑定当前用户。
    Dpapi = 1,
    /// 本地密钥文件 + AES-256-GCM（开发 / 非 Windows 回退）。
    AesFile = 2,
    /// 安卓 AndroidKeyStore 托管密钥（AES-256-GCM），密钥不出系统密钥库。
    Keystore = 3,
}

impl SealerKind {
    pub fn id(self) -> u8 {
        self as u8
    }

    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(SealerKind::Dpapi),
            2 => Some(SealerKind::AesFile),
            3 => Some(SealerKind::Keystore),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SealerKind::Dpapi => "Windows DPAPI",
            SealerKind::AesFile => "本地密钥文件(AES-256-GCM)",
            SealerKind::Keystore => "安卓 Keystore(AES-256-GCM)",
        }
    }

    pub fn name_of(id: u8) -> &'static str {
        match id {
            1 => "Windows DPAPI",
            2 => "本地密钥文件(AES-256-GCM)",
            3 => "安卓 Keystore(AES-256-GCM)",
            _ => "未知后端",
        }
    }

    /// 旧版安卓把密钥放在本地文件里；升级后先用旧后端解开，
    /// 再换成 Keystore 重新封，用户不需要重新导入账号。
    pub fn migrates_from(self, stored: Self) -> bool {
        self == SealerKind::Keystore && stored == SealerKind::AesFile
    }

    pub fn for_current_platform() -> Self {
        #[cfg(windows)]
        {
            SealerKind::Dpapi
        }
        #[cfg(target_os = "android")]
        {
            SealerKind::Keystore
        }
        #[cfg(not(any(windows, target_os = "android")))]
        {
            SealerKind::AesFile
        }
    }
}

pub fn seal(kind: SealerKind, key_path: &Path, plain: &[u8]) -> Result<Vec<u8>> {
    match kind {
        SealerKind::Dpapi => dpapi_protect(plain),
        SealerKind::AesFile => aes_seal(key_path, plain),
        SealerKind::Keystore => keystore_seal(plain),
    }
}

pub fn open(kind: SealerKind, key_path: &Path, sealed: &[u8]) -> Result<Vec<u8>> {
    match kind {
        SealerKind::Dpapi => dpapi_unprotect(sealed),
        SealerKind::AesFile => aes_open(key_path, sealed),
        SealerKind::Keystore => keystore_open(sealed),
    }
}

// ---------------------------------------------------------------- 安卓 Keystore

#[cfg(target_os = "android")]
fn keystore_seal(plain: &[u8]) -> Result<Vec<u8>> {
    crate::keystore::seal(plain)
}

#[cfg(target_os = "android")]
fn keystore_open(sealed: &[u8]) -> Result<Vec<u8>> {
    crate::keystore::open(sealed)
}

#[cfg(not(target_os = "android"))]
fn keystore_seal(_plain: &[u8]) -> Result<Vec<u8>> {
    Err(Error::Unsupported("当前平台没有安卓 Keystore".into()))
}

#[cfg(not(target_os = "android"))]
fn keystore_open(_sealed: &[u8]) -> Result<Vec<u8>> {
    Err(Error::Unsupported("当前平台没有安卓 Keystore".into()))
}

// ---------------------------------------------------------------- AES 回退

fn load_or_create_key(key_path: &Path) -> Result<[u8; 32]> {
    if key_path.exists() {
        let raw = std::fs::read(key_path).map_err(|e| Error::Io(e.to_string()))?;
        if raw.len() != 32 {
            return Err(Error::Format("密钥文件长度异常".into()));
        }
        let mut k = [0u8; 32];
        k.copy_from_slice(&raw);
        return Ok(k);
    }
    let mut k = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut k);
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::Io(e.to_string()))?;
    }
    std::fs::write(key_path, k).map_err(|e| Error::Io(e.to_string()))?;
    Ok(k)
}

fn aes_seal(key_path: &Path, plain: &[u8]) -> Result<Vec<u8>> {
    let key = load_or_create_key(key_path)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| Error::Invalid(e.to_string()))?;
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|e| Error::Invalid(format!("加密失败：{e}")))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn aes_open(key_path: &Path, sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < 13 {
        return Err(Error::Format("密文过短".into()));
    }
    let key = load_or_create_key(key_path)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| Error::Invalid(e.to_string()))?;
    let (nonce, ct) = sealed.split_at(12);
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| Error::Auth("解密失败（密钥不匹配或数据损坏）".into()))
}

// ---------------------------------------------------------------- Windows DPAPI

#[cfg(windows)]
mod dpapi_impl {
    use super::*;
    use std::ffi::c_void;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    // 直接声明 Win32 导出，避免绑定 crates 的版本漂移。
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            p_data_in: *const DataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *const c_void,
            p_prompt_struct: *const c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;

        fn CryptUnprotectData(
            p_data_in: *const DataBlob,
            ppsz_data_descr: *mut *mut u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *const c_void,
            p_prompt_struct: *const c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
        fn GetLastError() -> u32;
    }

    /// CRYPTPROTECT_UI_FORBIDDEN：后台定时任务里绝不能弹系统框。
    const UI_FORBIDDEN: u32 = 0x1;

    pub fn protect(data: &[u8]) -> Result<Vec<u8>> {
        let mut input = DataBlob {
            cb_data: data.len() as u32,
            pb_data: data.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &mut input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return Err(Error::Invalid(format!("DPAPI 加密失败（错误码 {code}）")));
        }
        let bytes = unsafe {
            let slice = std::slice::from_raw_parts(output.pb_data, output.cb_data as usize);
            let v = slice.to_vec();
            LocalFree(output.pb_data as *mut c_void);
            v
        };
        Ok(bytes)
    }

    pub fn unprotect(sealed: &[u8]) -> Result<Vec<u8>> {
        let mut input = DataBlob {
            cb_data: sealed.len() as u32,
            pb_data: sealed.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        let mut descr: *mut u16 = std::ptr::null_mut();
        let ok = unsafe {
            CryptUnprotectData(
                &mut input,
                &mut descr,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return Err(Error::Auth(format!(
                "无法解密凭据仓（DPAPI 错误码 {code}）。若换过电脑或重装过系统，请重新导入账号。"
            )));
        }
        let bytes = unsafe {
            let slice = std::slice::from_raw_parts(output.pb_data, output.cb_data as usize);
            let v = slice.to_vec();
            LocalFree(output.pb_data as *mut c_void);
            if !descr.is_null() {
                LocalFree(descr as *mut c_void);
            }
            v
        };
        Ok(bytes)
    }
}

#[cfg(windows)]
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi_impl::protect(data)
}

#[cfg(windows)]
fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi_impl::unprotect(data)
}

#[cfg(not(windows))]
fn dpapi_protect(_data: &[u8]) -> Result<Vec<u8>> {
    Err(Error::Unsupported(
        "当前平台没有 DPAPI，请使用 AES 回退后端".into(),
    ))
}

#[cfg(not(windows))]
fn dpapi_unprotect(_data: &[u8]) -> Result<Vec<u8>> {
    Err(Error::Unsupported(
        "当前平台没有 DPAPI，请使用 AES 回退后端".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文件头里的后端 id 一旦变了，旧仓就会被误判成「别的平台」，所以钉死。
    #[test]
    fn backend_ids_and_migration_are_stable() {
        assert_eq!(SealerKind::Dpapi.id(), 1);
        assert_eq!(SealerKind::AesFile.id(), 2);
        assert_eq!(SealerKind::Keystore.id(), 3);
        assert_eq!(SealerKind::from_id(3), Some(SealerKind::Keystore));
        assert_eq!(SealerKind::from_id(9), None);

        assert!(SealerKind::Keystore.migrates_from(SealerKind::AesFile));
        assert!(!SealerKind::Keystore.migrates_from(SealerKind::Dpapi));
        assert!(!SealerKind::Dpapi.migrates_from(SealerKind::AesFile));
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn keystore_is_unavailable_off_android() {
        assert!(seal(SealerKind::Keystore, Path::new("unused"), b"x").is_err());
        assert!(open(SealerKind::Keystore, Path::new("unused"), b"x").is_err());
    }

    #[test]
    fn aes_roundtrip_and_tamper_detection() {
        let dir = std::env::temp_dir().join(format!("ph-sealer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("vault.key");

        let sealed = seal(SealerKind::AesFile, &key, b"hello point hub").unwrap();
        let back = open(SealerKind::AesFile, &key, &sealed).unwrap();
        assert_eq!(back, b"hello point hub");

        // 篡改一个字节必须失败，而不是返回垃圾
        let mut bad = sealed.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        assert!(open(SealerKind::AesFile, &key, &bad).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip() {
        let sealed = seal(SealerKind::Dpapi, Path::new("unused"), b"dpapi payload").unwrap();
        assert_ne!(sealed, b"dpapi payload");
        let back = open(SealerKind::Dpapi, Path::new("unused"), &sealed).unwrap();
        assert_eq!(back, b"dpapi payload");
    }
}
