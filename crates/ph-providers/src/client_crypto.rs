//! 读取本机客户端登录态所需的解密原语。
//!
//! 这些函数**只用于读**客户端自己保存的登录态（`DECISIONS.md` D16）：
//! - Qoder CN：`Local State` 里的 DPAPI 密钥 + `auth.v1.dat` 的 AES-256-GCM 密文；
//! - Trae CN：`storage.json` 里的 AES-128-CBC 密文（SHA-512 派生密钥与完整性校验）。
//!
//! 不写回客户端文件、不打印明文。

use ph_core::{Error, Result};

/// 用 DPAPI 解开当前 Windows 用户保护的数据。
#[cfg(windows)]
pub fn dpapi_unprotect(sealed: &[u8]) -> Result<Vec<u8>> {
    dpapi::unprotect(sealed)
}

#[cfg(not(windows))]
pub fn dpapi_unprotect(_sealed: &[u8]) -> Result<Vec<u8>> {
    Err(Error::Unsupported(
        "当前系统不是 Windows，无法读取客户端登录态".into(),
    ))
}

/// AES-256-GCM 解密。`ct_and_tag` 是「密文 + 16 字节认证标签」。
pub fn aes256_gcm_decrypt(key: &[u8], nonce: &[u8], ct_and_tag: &[u8]) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    if key.len() != 32 || nonce.len() != 12 {
        return Err(Error::Format("登录态密钥/随机数长度异常".into()));
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| Error::Format(e.to_string()))?;
    cipher
        .decrypt(Nonce::from_slice(nonce), ct_and_tag)
        .map_err(|_| Error::Auth("登录态解密失败（密钥不匹配或数据损坏）".into()))
}

/// SHA-512，返回 64 字节摘要。
pub fn sha512(data: &[u8]) -> [u8; 64] {
    use sha2::{Digest, Sha512};
    let mut hasher = Sha512::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// AES-128-CBC 解密并去掉 PKCS#7 填充。
pub fn aes128_cbc_decrypt_pkcs7(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    use aes::cipher::{BlockDecrypt, KeyInit};
    use aes::cipher::generic_array::GenericArray;
    use aes::Aes128;

    if key.len() != 16 || iv.len() != 16 {
        return Err(Error::Format("登录态密钥/初始向量长度异常".into()));
    }
    if data.is_empty() || data.len() % 16 != 0 {
        return Err(Error::Format("登录态密文长度异常".into()));
    }

    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        for i in 0..16 {
            block[i] ^= prev[i];
        }
        out.extend_from_slice(&block);
        prev.copy_from_slice(chunk);
    }

    let pad = *out.last().expect("密文非空") as usize;
    if pad == 0 || pad > 16 || pad > out.len() {
        return Err(Error::Format("登录态填充异常".into()));
    }
    if !out[out.len() - pad..].iter().all(|b| *b as usize == pad) {
        return Err(Error::Format("登录态填充异常".into()));
    }
    out.truncate(out.len() - pad);
    Ok(out)
}

#[cfg(windows)]
mod dpapi {
    use super::*;
    use std::ffi::c_void;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    extern "system" {
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

    // 只有测试需要造密文，正式构建不引入加密方向。
    #[cfg(test)]
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
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
        fn GetLastError() -> u32;
    }

    /// CRYPTPROTECT_UI_FORBIDDEN：后台任务里绝不弹系统框。
    const UI_FORBIDDEN: u32 = 0x1;

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
                "无法解密客户端登录态（DPAPI 错误码 {code}）。请用当前 Windows 用户重新登录客户端。"
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

    /// 只给测试构造夹具用：造一份与客户端同格式的 DPAPI 密文。
    #[cfg(test)]
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
}

#[cfg(all(windows, test))]
pub fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi::protect(data)
}

/// 只给测试构造夹具用：AES-256-GCM 加密（输出密文 + 标签）。
#[cfg(test)]
pub fn aes256_gcm_encrypt(key: &[u8], nonce: &[u8], plain: &[u8]) -> Vec<u8> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let cipher = Aes256Gcm::new_from_slice(key).expect("32 字节密钥");
    cipher
        .encrypt(Nonce::from_slice(nonce), plain)
        .expect("加密不会失败")
}

/// 只给测试构造夹具用：AES-128-CBC + PKCS#7 加密。
#[cfg(test)]
pub fn aes128_cbc_encrypt_pkcs7(key: &[u8], iv: &[u8], plain: &[u8]) -> Vec<u8> {
    use aes::cipher::generic_array::GenericArray;
    use aes::cipher::{BlockEncrypt, KeyInit};
    use aes::Aes128;

    let cipher = Aes128::new(GenericArray::from_slice(key));
    let pad = 16 - plain.len() % 16;
    let mut buf = plain.to_vec();
    buf.extend(std::iter::repeat(pad as u8).take(pad));
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);
    let mut out = Vec::with_capacity(buf.len());
    for chunk in buf.chunks(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        for i in 0..16 {
            block[i] ^= prev[i];
        }
        cipher.encrypt_block(&mut block);
        out.extend_from_slice(&block);
        prev.copy_from_slice(&block);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbc_roundtrip_and_padding_check() {
        let key = [7u8; 16];
        let iv = [9u8; 16];
        let plain = b"{\"token\":\"hello-qoder\"}";
        let ct = aes128_cbc_encrypt_pkcs7(&key, &iv, plain);
        assert_eq!(aes128_cbc_decrypt_pkcs7(&key, &iv, &ct).unwrap(), plain);

        let mut bad = ct.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        assert!(aes128_cbc_decrypt_pkcs7(&key, &iv, &bad).is_err());
    }

    #[test]
    fn gcm_roundtrip_and_tamper_detection() {
        let key = [3u8; 32];
        let nonce = [5u8; 12];
        let plain = b"{\"token\":\"secret\"}";
        let sealed = aes256_gcm_encrypt(&key, &nonce, plain);
        assert_eq!(aes256_gcm_decrypt(&key, &nonce, &sealed).unwrap(), plain);

        let mut bad = sealed.clone();
        bad[0] ^= 0x01;
        assert!(aes256_gcm_decrypt(&key, &nonce, &bad).is_err());
    }

    #[test]
    fn sha512_matches_known_vector() {
        let digest = sha512(b"abc");
        assert_eq!(
            digest[..8],
            [0xdd, 0xaf, 0x35, 0xa1, 0x93, 0x61, 0x7a, 0xba]
        );
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip() {
        let sealed = dpapi_protect(b"qoder-client-key").unwrap();
        assert_ne!(sealed, b"qoder-client-key");
        assert_eq!(dpapi_unprotect(&sealed).unwrap(), b"qoder-client-key");
    }
}
