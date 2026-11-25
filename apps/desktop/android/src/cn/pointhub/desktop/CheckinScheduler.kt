package cn.pointhub.desktop

import android.content.Context
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import java.util.Calendar
import java.util.concurrent.TimeUnit

/**
 * 每日定时签到的排期。
 *
 * 固定在 00:16（默认签到时间 00:10 + 最多 5 分钟抖动之后）。
 * WorkManager 会持久化任务、开机后自动恢复；Doze 下可能晚几分钟，
 * 引擎按「今天还没签」判断，晚到也能补上。
 */
object CheckinScheduler {
    private const val WORK_NAME = "ph-daily-checkin"
    private const val RUN_HOUR = 0
    private const val RUN_MINUTE = 16

    /** 已经排过就保留原计划（KEEP），避免每次打开 App 都往后推一天。 */
    fun ensureScheduled(context: Context) {
        val request = PeriodicWorkRequestBuilder<CheckinWorker>(1, TimeUnit.DAYS)
            .setInitialDelay(initialDelayMillis(), TimeUnit.MILLISECONDS)
            .build()
        WorkManager.getInstance(context)
            .enqueueUniquePeriodicWork(WORK_NAME, ExistingPeriodicWorkPolicy.KEEP, request)
    }

    private fun initialDelayMillis(): Long {
        val now = Calendar.getInstance()
        val next = (now.clone() as Calendar).apply {
            set(Calendar.HOUR_OF_DAY, RUN_HOUR)
            set(Calendar.MINUTE, RUN_MINUTE)
            set(Calendar.SECOND, 0)
            set(Calendar.MILLISECOND, 0)
            if (!after(now)) {
                add(Calendar.DAY_OF_YEAR, 1)
            }
        }
        return next.timeInMillis - now.timeInMillis
    }
}
