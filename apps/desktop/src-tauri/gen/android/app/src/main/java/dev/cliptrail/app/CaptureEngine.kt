package dev.cliptrail.app

import android.content.ClipboardManager
import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import java.lang.ref.WeakReference
import java.util.concurrent.Executors

/** Clips worth handing to the core: new ones, and re-copies (the core bumps those to the top). */
val Verdict.saves: Boolean get() = this == Verdict.ACCEPTED || this == Verdict.DUPLICATE_CONTENT

/**
 * Process-wide capture coordinator, ported from the M1 spike. Triggers and focus grabs run on the main
 * thread (accessibility events, activity callbacks and WindowManager calls all arrive there); saving
 * (image decode + JNI ingest into the Rust store) runs on one worker thread.
 *
 * Privacy: clip text never goes to logcat (only signal, verdict, kind and length). Clips flagged
 * EXTRA_IS_SENSITIVE are dropped. Triggers from password managers never read the clipboard.
 */
object CaptureEngine {
    private const val TAG = "ClipTrail"

    /** Give the source app time to finish setPrimaryClip (WebViews copy asynchronously). */
    private const val TRIGGER_DELAY_MS = 120L
    private const val RETRY_DELAY_MS = 450L
    /** Non-click triggers right after a grab are ignored, so our own read cannot re-trigger us. */
    private const val COOLDOWN_MS = 1_500L
    private const val OVERLAY_TIMEOUT_MS = 600L
    private const val ACTIVITY_TIMEOUT_MS = 1_500L

    val main = Handler(Looper.getMainLooper())
    private val io = Executors.newSingleThreadExecutor { r -> Thread(r, "cliptrail-save") }
    private lateinit var appContext: Context
    private lateinit var matcher: SignalMatcher
    lateinit var salt: String
        private set
    private val guard = ClipGuard()

    @Volatile
    private var initialized = false

    private var serviceRef: WeakReference<CaptureAccessibilityService>? = null
    val serviceConnected: Boolean get() = serviceRef?.get() != null

    /** Last app in front (from window events), used as the source of toast-triggered and manual saves. */
    @Volatile
    var foregroundPkg: String? = null
        private set
    private var grabInProgress = false
    private var grabStartedAt = 0L
    private var lastGrabEnd = 0L
    private var nextId = 1L
    private var pendingFallback: PendingGrab? = null

    private data class PendingGrab(val id: Long, val signal: Signal, val pkg: String?, val t0: Long, val attempt: Int)

    @Synchronized
    fun init(context: Context) {
        if (initialized) return
        appContext = context.applicationContext
        NativeCore.ensureInit(appContext)
        salt = Prefs.salt(appContext)
        val labels = listOf(android.R.string.copy, android.R.string.cut, android.R.string.copyUrl)
            .map { appContext.getString(it) }
        matcher = SignalMatcher(labels, appContext.packageName, listOf(appContext.getString(R.string.app_name)))
        guard.restore(emptyList(), Prefs.lastClipTs(appContext))
        initialized = true
    }

    fun readClipboard(ctx: Context): ClipContent? = ClipReader.read(ctx, salt)

    // ---- accessibility service lifecycle -------------------------------------------------------

    fun onServiceConnected(svc: CaptureAccessibilityService) {
        serviceRef = WeakReference(svc)
    }

    fun onServiceDisconnected(svc: CaptureAccessibilityService) {
        if (serviceRef?.get() === svc) serviceRef = null
    }

    // ---- trigger layer --------------------------------------------------------------------------

    fun onEvent(e: A11yEvent) {
        val t0 = SystemClock.uptimeMillis()
        if (matcher.tracksForeground(e)) foregroundPkg = e.packageName
        if (matcher.isAccessToast(e)) return
        val signal = matcher.match(e) ?: return
        if (!Prefs.autoCapture(appContext)) return
        Log.d(TAG, "trigger $signal pkg=${e.packageName}")
        val source = if (matcher.isAppPackage(e.packageName)) e.packageName else foregroundPkg
        onTrigger(signal, source, t0)
    }

    private fun onTrigger(signal: Signal, pkg: String?, t0: Long) {
        if (pkg != null && SignalMatcher.isBlockedPackage(pkg)) {
            Log.i(TAG, "trigger from a password manager; clipboard not read")
            return
        }
        if (NativeCore.isPaused()) return
        if (grabInProgress && t0 - grabStartedAt > 5_000) grabInProgress = false // watchdog: lost callback
        if (grabInProgress) return
        if (signal != Signal.CLICK_COPY && t0 - lastGrabEnd < COOLDOWN_MS) return
        grabInProgress = true
        grabStartedAt = t0
        val g = PendingGrab(nextId++, signal, pkg, t0, attempt = 0)
        main.postDelayed({ grab(g) }, TRIGGER_DELAY_MS)
    }

    private fun grab(g: PendingGrab) {
        val svc = serviceRef?.get()
        if (svc == null) {
            launchFallback(g, "service not connected")
            return
        }
        svc.grabFocusAndRead(OVERLAY_TIMEOUT_MS) { focused, read, error ->
            if (!focused && read == null) {
                launchFallback(g, error ?: "overlay got no focus")
            } else {
                finishAuto(g, read, focused = true)
            }
        }
    }

    /** Transparent activity fallback. From the background this may be blocked unless "Appear on top" is allowed. */
    private fun launchFallback(g: PendingGrab, reason: String) {
        pendingFallback = g
        try {
            appContext.startActivity(ReadClipActivity.intent(appContext, g.signal, g.id))
        } catch (e: Exception) {
            pendingFallback = null
            Log.i(TAG, "$reason; fallback threw ${e.javaClass.simpleName}")
            finishAuto(g, null, focused = false)
            return
        }
        main.postDelayed({
            if (pendingFallback?.id == g.id) {
                pendingFallback = null
                Log.i(TAG, "$reason; activity fallback blocked or no focus")
                finishAuto(g, null, focused = false)
            }
        }, ACTIVITY_TIMEOUT_MS)
    }

    private fun finishAuto(g: PendingGrab, read: ClipContent?, focused: Boolean) {
        val verdict = if (!focused && read == null) Verdict.NO_FOCUS
        else guard.evaluate(read?.snapshot, System.currentTimeMillis(), requireFresh = true)
        if (verdict == Verdict.STALE && g.attempt == 0 && g.signal == Signal.CLICK_COPY) {
            // The app may not have written the clip yet (async copy). One more try.
            main.postDelayed({ grab(g.copy(attempt = 1)) }, RETRY_DELAY_MS)
            return
        }
        grabInProgress = false
        lastGrabEnd = SystemClock.uptimeMillis()
        Prefs.setLastClipTs(appContext, guard.lastSeenTimestamp)
        Log.i(TAG, "auto signal=${g.signal} verdict=$verdict kind=${read?.kind} len=${read?.length} latency=${lastGrabEnd - g.t0}ms")
        if (read != null && verdict.saves) save(read, g.pkg, g.signal, null)
    }

    // ---- manual paths -----------------------------------------------------------------------------

    /** Called by [ReadClipActivity]. [done] gets a toast message for manual reads, null for fallback reads. */
    fun onActivityRead(signal: Signal, requestId: Long, read: ClipContent?, focused: Boolean, done: (String?) -> Unit) {
        if (requestId > 0) {
            val g = pendingFallback
            if (g != null && g.id == requestId) {
                pendingFallback = null
                finishAuto(g, read, focused)
            }
            done(null) // a late fallback for a request that already timed out is ignored
            return
        }
        manualRead(signal, read, focused, done)
    }

    /**
     * A read we were allowed to do because we are the focused app (tile, notification, app opened, the
     * in-app button). [done] (main thread) gets a short message for a toast; null = stay quiet.
     */
    fun manualRead(signal: Signal, read: ClipContent?, focused: Boolean, done: ((String?) -> Unit)?) {
        val verdict = if (!focused && read == null) Verdict.NO_FOCUS
        else guard.evaluate(read?.snapshot, System.currentTimeMillis(), requireFresh = false)
        Prefs.setLastClipTs(appContext, guard.lastSeenTimestamp)
        Log.i(TAG, "manual signal=$signal verdict=$verdict kind=${read?.kind} len=${read?.length}")
        if (verdict == Verdict.ACCEPTED && Prefs.autoCapture(appContext) && serviceConnected) {
            Prefs.recordMiss(appContext)
        }
        when {
            read != null && verdict.saves && (read.kind == ContentKind.TEXT || read.kind == ContentKind.IMAGE) ->
                save(read, foregroundPkg, signal) { code -> done?.invoke(NativeCore.message(appContext, code)) }
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
