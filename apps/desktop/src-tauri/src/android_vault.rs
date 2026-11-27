//! 把应用 Context 交给 `ph-vault` 的安卓 Keystore 桥。
//!
//! 后台签到可能由 WorkManager 冷启动进程：这时没有 Activity，Tauri 启动流程
//! 不会走，`ndk-context` 也不保证已初始化。因此由 Kotlin 在 Activity 与
//! Worker 里各调一次这里，Rust 侧只认第一次。

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_cn_pointhub_desktop_VaultKeys_nativeInit(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    context: jni::objects::JObject,
) -> jni::sys::jboolean {
    // JNI 边界上 panic 会直接杀掉进程，先兜住。
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ph_vault::keystore::init(&mut env, &context).is_ok()
    }))
    .unwrap_or(false) as jni::sys::jboolean
}
