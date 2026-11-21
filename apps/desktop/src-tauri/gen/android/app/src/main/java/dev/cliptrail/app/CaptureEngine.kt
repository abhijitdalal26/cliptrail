package dev.cliptrail.app

import android.content.ClipboardManager
import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import java.util.concurrent.Executors

/** Clips worth handing to the core: new ones, and re-copies (the core bumps those to the top). */
val Verdict.saves: Boolean get() = this == Verdict.ACCEPTED || this == Verdict.DUPLICATE_CONTENT

/**
 * Process-wide capture coordinator. Every capture is one the user starts (tile, notification button,
 * opening the app, the in-app button, Share, the text menu); reads happen on the main thread while we
 * are the focused app, saving (image decode + JNI ingest into the Rust store) runs on one worker thread.
 *
 * Privacy: clip text never goes to logcat (only signal, verdict, kind and length). Clips flagged
 * EXTRA_IS_SENSITIVE are dropped.
 */
object CaptureEngine {
    private const val TAG = "ClipTrail"

    val main = Handler(Looper.getMainLooper())
    private val io = Executors.newSingleThreadExecutor { r -> Thread(r, "cliptrail-save") }
    private lateinit var appContext: Context
    lateinit var salt: String
        private set
    private val guard = ClipGuard()

    @Volatile
    private var initialized = false

    @Synchronized
    fun init(context: Context) {
        if (initialized) return
        appContext = context.applicationContext
        NativeCore.ensureInit(appContext)
        salt = Prefs.salt(appContext)
        guard.restore(emptyList(), Prefs.lastClipTs(appContext))
        initialized = true
    }

    fun readClipboard(ctx: Context): ClipContent? = ClipReader.read(ctx, salt)

    // ---- capture paths ----------------------------------------------------------------------------

    /**
     * A read we were allowed to do because we are the focused app (tile, notification, app opened, the
     * in-app button). [done] (main thread) gets a short message for a toast; null = stay quiet.
     */
    fun manualRead(signal: Signal, read: ClipContent?, focused: Boolean, done: ((String?) -> Unit)?) {
        val verdict = if (!focused && read == null) Verdict.NO_FOCUS
        else guard.evaluate(read?.snapshot, System.currentTimeMillis(), requireFresh = false)
        Prefs.setLastClipTs(appContext, guard.lastSeenTimestamp)
        Log.i(TAG, "manual signal=$signal verdict=$verdict kind=${read?.kind} len=${read?.length}")
        when {
            read != null && verdict.saves && (read.kind == ContentKind.TEXT || read.kind == ContentKind.IMAGE) ->
                save(read, null, signal) { code -> done?.invoke(NativeCore.message(appContext, code)) }
            verdict == Verdict.DUPLICATE_SAME_CLIP -> done?.invoke(appContext.getString(R.string.msg_already_saved))
            verdict == Verdict.SENSITIVE -> done?.invoke(appContext.getString(R.string.msg_sensitive))
            read?.kind == ContentKind.OTHER -> done?.invoke(appContext.getString(R.string.msg_unsupported))
            else -> done?.invoke(appContext.getString(R.string.msg_nothing))
        }
    }

    /** We just put a clip on the clipboard ourselves (Copy in the app): don't save it back. */
    fun noteOwnClip(ctx: Context) {
        val ts = try {
            ctx.getSystemService(ClipboardManager::class.java)?.primaryClipDescription?.timestamp ?: 0L
        } catch (_: Exception) {
            0L
        }
        val wall = ClipTime.toWallClock(ts, System.currentTimeMillis(), SystemClock.elapsedRealtime())
        if (wall > 0) {
            guard.noteOwnClip(wall)
            Prefs.setLastClipTs(appContext, wall)
        }
    }

    /** Share target / text-selection menu: content arrives in the intent, no clipboard involved. */
    fun saveSharedText(text: String, pkg: String?, signal: Signal, done: (String) -> Unit) {
        io.execute {
            val code = try {
                NativeCore.ingestText(text, null, pkg, System.currentTimeMillis())
            } catch (t: Throwable) {
                Log.w(TAG, "save failed: ${t.javaClass.simpleName}")
                NativeCore.FAILED
            }
            finishSave(signal, "text", text.length, code)
            main.post { done(NativeCore.message(appContext, code)) }
        }
    }

    /** Shared images: read now, while the share grant is alive (the caller keeps its activity open). */
    fun saveSharedImages(uris: List<Uri>, pkg: String?, done: (String) -> Unit) {
        io.execute {
            var best = NativeCore.SKIPPED
            for (uri in uris.take(20)) {
                val png = ClipReader.imagePng(appContext, uri)
                val code = if (png == null) NativeCore.SKIPPED else try {
                    NativeCore.ingestImage(png, pkg, System.currentTimeMillis())
                } catch (t: Throwable) {
                    Log.w(TAG, "save failed: ${t.javaClass.simpleName}")
                    NativeCore.FAILED
                }
                finishSave(Signal.SHARE, "image", png?.size ?: 0, code)
                if (code == NativeCore.STORED || (code == NativeCore.BUMPED && best != NativeCore.STORED)) best = code
                if (code == NativeCore.FAILED && best == NativeCore.SKIPPED) best = code
            }
            main.post { done(NativeCore.message(appContext, best)) }
        }
    }

    private fun save(read: ClipContent, pkg: String?, signal: Signal, done: ((Int) -> Unit)?) {
        val ts = if (read.snapshot.timestampMs > 0) read.snapshot.timestampMs else System.currentTimeMillis()
        io.execute {
            val code = try {
                when (read.kind) {
                    ContentKind.TEXT -> NativeCore.ingestText(read.text ?: "", read.html, pkg, ts)
                    ContentKind.IMAGE -> read.uri?.let { ClipReader.imagePng(appContext, it) }
                        ?.let { NativeCore.ingestImage(it, pkg, ts) } ?: NativeCore.SKIPPED
                    else -> NativeCore.SKIPPED
                }
            } catch (t: Throwable) {
                Log.w(TAG, "save failed: ${t.javaClass.simpleName}")
                NativeCore.FAILED
            }
            finishSave(signal, read.kind.name.lowercase(), read.length, code)
            main.post { done?.invoke(code) }
        }
    }

    private fun finishSave(signal: Signal, kind: String, len: Int, code: Int) {
        Log.i(TAG, "saved signal=$signal kind=$kind len=$len result=$code")
        if (code == NativeCore.STORED || code == NativeCore.BUMPED) Prefs.recordCapture(appContext, signal)
    }
}
