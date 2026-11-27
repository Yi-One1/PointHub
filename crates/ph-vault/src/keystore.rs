//! 安卓 Keystore 密钥托管（JNI 桥）。
//!
//! 密钥由 AndroidKeyStore 生成并保管，Rust 只把明文交给 Kotlin 做一次
//! AES-256-GCM 加解密，密钥本身永远不出系统密钥库。Kotlin 实现见
//! `apps/desktop/android/src/cn/pointhub/desktop/VaultKeystore.kt`。
//!
//! 反调路径与安装器一致：用应用 ClassLoader 加载 Kotlin 类，而不是 `FindClass`
//! （附着线程没有 Java 调用栈，`FindClass` 找不到应用类）。
//!
//! JVM 与应用 Context 由 `VaultKeys.nativeInit` 在 Activity 或后台任务启动时写入。
//! 后台任务可能在没有 Activity 的进程里被 WorkManager 冷启动，此时 `ndk-context`
//! 不保证已初始化，所以这里不依赖它。

use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use ph_core::{Error, Result};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// JVM 句柄，进程内只记一次。
static VM: OnceLock<JavaVM> = OnceLock::new();
/// 应用 Context 的全局引用，存成裸指针避免 `GlobalRef` 的线程约束。
static CONTEXT: AtomicUsize = AtomicUsize::new(0);

/// 记录 JVM 与应用 Context。可重复调用，只有第一次生效。
pub fn init(env: &mut JNIEnv, context: &JObject) -> Result<()> {
    if VM.get().is_some() {
        return Ok(());
    }
    let vm = env
        .get_java_vm()
        .map_err(|e| Error::Unsupported(format!("取 JVM 失败：{e}")))?;
    let global = env
        .new_global_ref(context)
        .map_err(|e| Error::Unsupported(format!("保存应用上下文失败：{e}")))?;
    let raw = global.as_obj().as_raw() as usize;
    // 进程生命周期内不释放：只是让 Context 不被回收，量级是一个对象。
    std::mem::forget(global);
    CONTEXT.store(raw, Ordering::SeqCst);
    let _ = VM.set(vm);
    Ok(())
}

pub fn seal(plain: &[u8]) -> Result<Vec<u8>> {
    call("seal", plain, "加密")
}

pub fn open(sealed: &[u8]) -> Result<Vec<u8>> {
    call("open", sealed, "解密")
}

fn call(method: &str, input: &[u8], action: &str) -> Result<Vec<u8>> {
    let vm = VM.get().ok_or_else(|| {
        Error::Unsupported("系统密钥服务尚未就绪，请重新打开应用".into())
    })?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| Error::Unsupported(format!("JNI 线程挂载失败：{e}")))?;
    let class = vault_class(&mut env)?;
    let input = env
        .byte_array_from_slice(input)
        .map_err(|e| Error::Invalid(format!("参数转换失败：{e}")))?;
    let value = match env.call_static_method(&class, method, "([B)[B", &[JValue::Object(&input)]) {
        Ok(v) => v,
        Err(_) => return Err(java_error(&mut env, action)),
    };
    let obj = value
        .l()
        .map_err(|e| Error::Invalid(format!("{action}返回类型异常：{e}")))?;
    if obj.is_null() {
        return Err(Error::Auth(format!("{action}失败：系统密钥返回空结果")));
    }
    let arr = JByteArray::from(obj);
    env.convert_byte_array(&arr)
        .map_err(|e| Error::Auth(format!("{action}失败：{e}")))
}

fn vault_class<'local>(env: &mut JNIEnv<'local>) -> Result<JClass<'local>> {
    let raw = CONTEXT.load(Ordering::SeqCst);
    if raw == 0 {
        return Err(Error::Unsupported(
            "系统密钥服务尚未就绪，请重新打开应用".into(),
        ));
    }
    // SAFETY: 存进去的是 NewGlobalRef 的产物，进程内不会被释放；包装成
    // JObject 只是借用来调方法，不会触发删除全局引用。
    let context = unsafe { JObject::from_raw(raw as jni::sys::jobject) };
    let loader = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
        .map_err(|e| Error::Unsupported(format!("取应用类加载器失败：{e}")))?;
    let name = env
        .new_string("cn.pointhub.desktop.VaultKeystore")
        .map_err(|e| Error::Invalid(format!("类名转换失败：{e}")))?;
    env.call_method(
        &loader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[JValue::Object(&name)],
    )
    .and_then(|v| v.l())
    .map(JClass::from)
    .map_err(|e| Error::Unsupported(format!("找不到密钥服务类：{e}")))
}

/// 把 Kotlin 抛出的异常转成给用户看的一句话，并清掉挂起异常，
/// 否则后续 JNI 调用会全部失败。
fn java_error(env: &mut JNIEnv, action: &str) -> Error {
    if !matches!(env.exception_check(), Ok(true)) {
        return Error::Auth(format!(
            "{action}失败：无法访问系统密钥，请重新导入账号"
        ));
    }
    // 先打到 logcat，真机排障只能靠这个。
    let _ = env.exception_describe();
    let mut detail = String::new();
    if let Ok(ex) = env.exception_occurred() {
        let _ = env.exception_clear();
        if let Ok(v) = env.call_method(&ex, "toString", "()Ljava/lang/String;", &[]) {
            if let Ok(o) = v.l() {
                let s = JString::from(o);
                let owned = env
                    .get_string(&s)
                    .map(|js| js.to_string_lossy().into_owned());
                if let Ok(text) = owned {
                    detail = text;
                }
            }
        }
    } else {
        let _ = env.exception_clear();
    }
    if detail.is_empty() {
        Error::Auth(format!(
            "{action}失败：系统密钥可能已随恢复安装丢失，请重新导入账号"
        ))
    } else {
        Error::Auth(format!("{action}失败：{detail}"))
    }
}
