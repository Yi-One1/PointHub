//! 安卓应用内安装：把下载好的 APK 交给系统安装器。
//!
//! 应用无法静默安装（那需要设备管理员权限），但可以省掉「跳浏览器 → 等下载 →
//! 回通知栏点安装包」这一圈：Rust 先把 APK 下到应用私有目录，再通过 FileProvider
//! 交给 `Intent.ACTION_VIEW`，系统安装器会直接弹出确认界面。
//!
//! 原生部分在 `apps/desktop/android/src/cn/pointhub/desktop/ApkInstaller.kt`，
//! 构建时由 `scripts/patch-android-sources.ps1` 注入 `gen/`（`gen/` 不入库）。

use std::path::Path;

/// Kotlin 原理结果，字段与 `ApkInstaller.kt` 返回的 JSON 一一对应。
pub struct InstallOutcome {
    /// 安装器已打开。
    pub ok: bool,
    /// 缺少「安装未知应用」授权，已打开系统设置页。
    pub need_permission: bool,
    /// 面向用户的提示（成功时为空）。
    pub message: String,
}

/// 调起系统安装器。失败时返回可以直接展示给用户的错误文案。
pub fn install(path: &Path) -> Result<InstallOutcome, String> {
    let json = call_apk_installer(&path.to_string_lossy())?;
    let v: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("安装器返回内容无法解析：{e}"))?;
    Ok(InstallOutcome {
        ok: v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false),
        need_permission: v
            .get("needPermission")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        message: v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// 从 Rust 反调 Kotlin。
///
/// 不直接用 `FindClass`：JNI 附着上来的原生线程没有 Java 调用栈，
/// `FindClass` 会走系统类加载器，找不到应用自己的类。改为拿 Application 的
/// `getClassLoader().loadClass(...)`，这条路在任意线程都成立。
fn call_apk_installer(path: &str) -> Result<String, String> {
    let ctx = ndk_context::android_context();
    // SAFETY: ndk_context 由 tao 在 Activity onCreate 时初始化，
    // 其 JVM 与 Application Context 在进程存活期间保持有效。
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| format!("JNI 初始化失败：{e}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| format!("JNI 线程挂载失败：{e}"))?;

    let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    let loader = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
        .map_err(|e| format!("取应用类加载器失败：{e}"))?;
    let class_name = env
        .new_string("cn.pointhub.desktop.ApkInstaller")
        .map_err(|e| format!("类名转换失败：{e}"))?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[jni::objects::JValue::Object(&class_name)],
        )
        .and_then(|v| v.l())
        .map(jni::objects::JClass::from)
        .map_err(|e| format!("找不到安装器类：{e}"))?;

    let arg = env
        .new_string(path)
        .map_err(|e| format!("路径转换失败：{e}"))?;
    let result = env
        .call_static_method(
            &class,
            "install",
            "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
            &[
                jni::objects::JValue::Object(&context),
                jni::objects::JValue::Object(&arg),
            ],
        )
        .map_err(|e| format!("调用安装器失败：{e}"))?;
    let obj = result.l().map_err(|e| format!("安装器返回错误：{e}"))?;
    let text = jni::objects::JString::from(obj);
    let value = env
        .get_string(&text)
        .map_err(|e| format!("安装器返回值读取失败：{e}"))?;
    Ok(value.to_string_lossy().into_owned())
}
