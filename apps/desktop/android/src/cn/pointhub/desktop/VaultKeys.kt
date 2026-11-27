package cn.pointhub.desktop

import android.content.Context

/**
 * 把应用上下文交给 Rust：安卓 Keystore 调用需要它来加载 [VaultKeystore]。
 *
 * Activity 启动时与后台任务启动时各调用一次；Rust 侧只认第一次。
 * 后台任务可能由 WorkManager 冷启动进程（没有 Activity），不能依赖
 * Tauri 启动时才有的上下文。
 */
object VaultKeys {
    init {
        System.loadLibrary("pointhub_lib")
    }

    @JvmStatic
    fun init(context: Context) {
        nativeInit(context.applicationContext ?: context)
    }

    @JvmStatic
    private external fun nativeInit(context: Context): Boolean
}
