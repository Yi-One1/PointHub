package cn.pointhub.desktop

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import org.json.JSONObject
import java.io.File

/**
 * 用系统安装器安装已下载的 APK。
 *
 * 安装确认界面由系统提供（应用无法静默安装），这里只负责把私有目录里的
 * 文件通过 FileProvider 暴露成临时可读的 content:// URI，再交给系统。
 * Android 8 起还需要「安装未知应用」授权；没授权时跳到系统设置页，
 * 用户开好返回时由 [resumePending] 接着装，不用再点一次。
 */
object ApkInstaller {
    /** 等待系统设置授权后继续安装的 APK 路径。 */
    private var pending: String? = null

    /** 返回 JSON：{"ok":true} / {"needPermission":true,"message":"..."} / {"message":"..."}。 */
    @JvmStatic
    fun install(context: Context, path: String): String {
        val file = File(path)
        if (!file.isFile || file.length() == 0L) {
            return fail("安装包不存在或不完整，请重新下载")
        }
        if (!canInstall(context)) {
            pending = path
            openUnknownSources(context)
            return JSONObject()
                .put("needPermission", true)
                .put("message", "已打开系统设置，请允许「积分枢纽」安装应用，返回后会自动继续")
                .toString()
        }
        pending = null
        return try {
            val uri = FileProvider.getUriForFile(
                context,
                "${context.packageName}.fileprovider",
                file,
            )
            val intent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, "application/vnd.android.package-archive")
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
            JSONObject().put("ok", true).toString()
        } catch (e: Exception) {
            fail("无法打开系统安装器：${e.message ?: "未知错误"}")
        }
    }

    /** 从系统设置页返回时调用：权限已开就继续安装。 */
    @JvmStatic
    fun resumePending(context: Context) {
        val path = pending ?: return
        if (!canInstall(context)) {
            return
        }
        install(context, path)
    }

    private fun canInstall(context: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.O ||
            context.packageManager.canRequestPackageInstalls()

    private fun fail(message: String): String = JSONObject().put("message", message).toString()

    private fun openUnknownSources(context: Context) {
        val packageUri = Uri.parse("package:${context.packageName}")
        try {
            context.startActivity(
                Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, packageUri)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
        } catch (_: Exception) {
            // 个别 ROM 没有这个页面，退回应用详情页
            try {
                context.startActivity(
                    Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, packageUri)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                )
            } catch (_: Exception) {
                // 设置页也打不开时只能让用户自己找；上层文案已说明要做什么
            }
        }
    }
}
