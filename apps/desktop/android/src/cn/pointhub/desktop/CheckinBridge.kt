package cn.pointhub.desktop

/**
 * WorkManager 后台签到的 JNI 桥。
 *
 * 一次调用完成全部工作：Rust 负责调度判断、签到与文案，
 * Kotlin 只把结果弹成系统通知。
 */
object CheckinBridge {
    init {
        System.loadLibrary("pointhub_lib")
    }

    /** 返回 JSON：{ ran, reason?, claimed, already, failed, notify, title, body }。 */
    @JvmStatic
    external fun runScheduledCheckin(dataDir: String): String
}
