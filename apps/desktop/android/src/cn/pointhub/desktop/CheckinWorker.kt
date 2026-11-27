package cn.pointhub.desktop

import android.content.Context
import androidx.work.Worker
import androidx.work.WorkerParameters
import org.json.JSONObject

/**
 * WorkManager 的每日任务：调一次 Rust，按返回的 JSON 决定要不要发通知。
 *
 * 永远返回 success——签到失败属于业务结果（已记进历史与通知），
 * 不该让 WorkManager 把它当成「任务本身失败」去重试。
 */
class CheckinWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    override fun doWork(): Result {
        // 必须与 Tauri 的 app_data_dir 一致：安卓上它解析成 context.dataDir
        // （/data/user/0/<包名>），不是 filesDir。
        val dataDir = applicationContext.dataDir.absolutePath
        val json = try {
            // 冷启动时没有 Activity，先把应用上下文交给 Rust，
            // 否则打不开由 AndroidKeyStore 加密的凭据仓。
            VaultKeys.init(applicationContext)
            CheckinBridge.runScheduledCheckin(dataDir)
        } catch (t: Throwable) {
            CheckinNotify.show(
                applicationContext,
                "积分枢纽 · 后台签到失败",
                "后台任务无法启动：${t.message ?: t.javaClass.simpleName}",
            )
            return Result.success()
        }
        val obj = runCatching { JSONObject(json) }.getOrNull() ?: return Result.success()
        android.util.Log.d("PointHubWorker", json)
        if (obj.optBoolean("notify", false)) {
            CheckinNotify.show(
                applicationContext,
                obj.optString("title", "积分枢纽"),
                obj.optString("body", ""),
            )
        }
        return Result.success()
    }
}
