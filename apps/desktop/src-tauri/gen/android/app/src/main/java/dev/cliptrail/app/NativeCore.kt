package dev.cliptrail.app

import android.content.Context
import android.os.Build
import android.provider.Settings
import android.util.Log

/**
 * JNI surface into the Rust core: the same .so the Tauri UI runs from (src-tauri/src/mobile/jni.rs).
 * Lets the accessibility service, tile and share activities save clips while no UI is running.
 * Call the ingest functions off the main thread.
 */
object NativeCore {
    private const val TAG = "ClipTrail"

    const val STORED = 1
    const val BUMPED = 2
    const val SKIPPED = 0
    const val FAILED = -1

    @Volatile
    private var ready = false
    private val lock = Any()

    init {
        System.loadLibrary("cliptrail_desktop_lib")
    }

    /** Opens the clip store under filesDir (once per process) and starts sync if paired. */
    fun ensureInit(ctx: Context): Boolean {
        if (ready) return true
        synchronized(lock) {
            if (ready) return true
            val app = ctx.applicationContext
            ready = try {
                nativeInit(app.filesDir.absolutePath, app.noBackupFilesDir.absolutePath, deviceName(app))
            } catch (t: Throwable) {
                Log.e(TAG, "native init failed: ${t.javaClass.simpleName}")
                false
            }
            if (!ready) Log.e(TAG, "clip store unavailable")
            return ready
        }
    }

    /** The name people know the phone by ("Galaxy S23 FE"), shown on the computer after pairing. */
    fun deviceName(ctx: Context): String {
        val name = try {
            Settings.Global.getString(ctx.contentResolver, Settings.Global.DEVICE_NAME)
        } catch (_: Exception) {
            null
        }
        if (!name.isNullOrBlank()) return name.trim()
        val maker = Build.MANUFACTURER.replaceFirstChar { it.uppercase() }
        return if (Build.MODEL.startsWith(Build.MANUFACTURER, ignoreCase = true)) Build.MODEL else "$maker ${Build.MODEL}"
    }

    /** "Saved" / "already saved" / ... for a toast after a manual save. */
    fun message(ctx: Context, code: Int): String = ctx.getString(
        when (code) {
            STORED -> R.string.msg_saved
            BUMPED -> R.string.msg_already_saved
            SKIPPED -> R.string.msg_not_saved
            else -> R.string.msg_failed
        },
    )

    @JvmStatic
    private external fun nativeInit(dataDir: String, keysDir: String, deviceName: String): Boolean

    /** Returns [STORED], [BUMPED], [SKIPPED] or [FAILED]. */
    @JvmStatic
    external fun ingestText(text: String, html: String?, sourcePkg: String?, capturedAt: Long): Int

    /** PNG bytes. Returns [STORED], [BUMPED], [SKIPPED] or [FAILED]. */
    @JvmStatic
    external fun ingestImage(png: ByteArray, sourcePkg: String?, capturedAt: Long): Int

    /** The user paused capture in the app. */
    @JvmStatic
    external fun isPaused(): Boolean
}
