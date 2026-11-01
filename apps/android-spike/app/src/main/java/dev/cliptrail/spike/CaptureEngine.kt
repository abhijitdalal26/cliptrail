package dev.cliptrail.spike

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import java.lang.ref.WeakReference
import java.security.SecureRandom
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Result of one clipboard read. Text never goes to logcat; only kind and length do. */
data class ClipRead(
    val snapshot: ClipSnapshot,
    val kind: String,
    val preview: String?,
    val mime: String,
    val length: Int,
    val isSelfTest: Boolean,
)

/**
 * Process-wide coordinator. Everything runs on the main thread (accessibility events, activity
 * callbacks and WindowManager calls all arrive there), so there is no locking.
 */
object CaptureEngine {
    private const val TAG = "ClipTrailSpike"
    const val TEST_PREFIX = "ClipTrail spike test"
    private const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE" // ClipDescription.EXTRA_IS_SENSITIVE (API 33)

    /** Give the source app time to finish setPrimaryClip (WebViews copy asynchronously). */
    private const val TRIGGER_DELAY_MS = 120L
    private const val RETRY_DELAY_MS = 450L
    /** Non-click triggers right after a grab are ignored, so our own read cannot re-trigger us. */
    private const val COOLDOWN_MS = 1_500L
    private const val OVERLAY_TIMEOUT_MS = 600L
    private const val ACTIVITY_TIMEOUT_MS = 1_500L

    val main = Handler(Looper.getMainLooper())
    lateinit var store: Store
        private set
    private lateinit var appContext: Context
    private lateinit var matcher: SignalMatcher
    private lateinit var salt: String
    private val guard = ClipGuard()
    private var initialized = false

    private var serviceRef: WeakReference<CaptureAccessibilityService>? = null
    val serviceConnected: Boolean get() = serviceRef?.get() != null

    private var foregroundPkg: String? = null
    private var grabInProgress = false
    private var grabStartedAt = 0L
    private var lastGrabEnd = 0L
    private var nextId = 1L
    private var pendingFallback: PendingGrab? = null
    private var testCounter = 0
    private val listeners = LinkedHashSet<() -> Unit>()

    private data class PendingGrab(val id: Long, val signal: Signal, val pkg: String?, val t0: Long, val attempt: Int)

    fun init(context: Context) {
        if (initialized) return
        appContext = context.applicationContext
        store = Store(appContext)
        salt = store.text("salt") ?: newSalt().also { store.putText("salt", it) }
        val labels = listOf(android.R.string.copy, android.R.string.cut, android.R.string.copyUrl)
            .map { appContext.getString(it) }
        matcher = SignalMatcher(labels, appContext.packageName, listOf(appContext.getString(R.string.app_name)))
        guard.restore(store.acceptedHashes(500), store.long("guard.lastTs"))
        initialized = true
    }

    // ---- UI listeners ---------------------------------------------------------------------------

    fun addListener(l: () -> Unit) { listeners.add(l) }
    fun removeListener(l: () -> Unit) { listeners.remove(l) }
    fun changed() { listeners.toList().forEach { it() } }

    // ---- accessibility service lifecycle -------------------------------------------------------

    fun onServiceConnected(svc: CaptureAccessibilityService) {
        serviceRef = WeakReference(svc)
        store.inc("svc.connects")
        if (store.long("svc.firstConnectedAt") == 0L) store.putLong("svc.firstConnectedAt", System.currentTimeMillis())
        store.putLong("svc.connectedAt", System.currentTimeMillis())
        changed()
    }

    fun onServiceDisconnected(svc: CaptureAccessibilityService) {
        if (serviceRef?.get() === svc) serviceRef = null
        changed()
    }

    // ---- trigger layer --------------------------------------------------------------------------

    fun onEvent(e: A11yEvent) {
        val t0 = SystemClock.uptimeMillis()
        if (matcher.tracksForeground(e)) foregroundPkg = e.packageName
        if (matcher.isAccessToast(e)) {
            store.inc("side.access_toast")
            changed()
            return
        }
        val signal = matcher.match(e)
        if (signal != null || matcher.isDiagnostic(e)) {
            // Text of real notifications (message bodies!) is never stored; toast/button text is, encrypted.
            val text = if (e.kind == EventKind.NOTIFICATION && !e.isToast) null
            else e.texts.joinToString(" | ").take(60).ifEmpty { null }
            val type = if (e.kind == EventKind.NOTIFICATION) (if (e.isToast) "TOAST" else "NOTIFICATION") else e.kind.name
            store.recordSignature(type, e.packageName, e.className, text, signal?.name, System.currentTimeMillis())
        }
        if (signal == null) return
        Log.d(TAG, "trigger $signal pkg=${e.packageName} cls=${e.className}")
        val source = if (matcher.isAppPackage(e.packageName)) e.packageName else foregroundPkg
        onTrigger(signal, source, t0)
    }

    private fun onTrigger(signal: Signal, pkg: String?, t0: Long) {
        store.inc("trig.${signal.name}")
        if (pkg != null && SignalMatcher.isBlockedPackage(pkg)) {
            record(signal, pkg, Verdict.BLOCKED_APP, null, 0, "trigger from blocked app; clipboard not read")
            return
        }
        if (grabInProgress && t0 - grabStartedAt > 5_000) grabInProgress = false // watchdog: lost callback
        if (grabInProgress) {
            store.inc("trig.merged")
            changed()
            return
        }
        if (signal != Signal.CLICK_COPY && t0 - lastGrabEnd < COOLDOWN_MS) {
            store.inc("trig.cooldown")
            changed()
            return
        }
        grabInProgress = true
        grabStartedAt = t0
        val g = PendingGrab(nextId++, signal, pkg, t0, attempt = 0)
        main.postDelayed({ grab(g) }, TRIGGER_DELAY_MS)
        changed()
    }

    private fun grab(g: PendingGrab) {
        val svc = serviceRef?.get()
        if (svc == null) {
            launchFallback(g, "service not connected")
            return
        }
        val started = SystemClock.uptimeMillis()
        svc.grabFocusAndRead(OVERLAY_TIMEOUT_MS) { focused, read, error ->
            val focusMs = SystemClock.uptimeMillis() - started
            if (!focused && read == null) {
                store.inc("overlay.fail")
                launchFallback(g, error ?: "overlay got no focus")
            } else {
                if (focused) {
                    store.inc("overlay.focus_count")
                    store.inc("overlay.focus_ms_sum", focusMs)
                    store.setMax("overlay.focus_ms_max", focusMs)
                }
                finishAuto(g, read, focused = true, note = "overlay, focus after ${focusMs} ms")
            }
        }
    }

    /** Transparent activity fallback. From the background this may be blocked unless "Appear on top" is allowed. */
    private fun launchFallback(g: PendingGrab, reason: String) {
        store.inc("fallback.launch")
        pendingFallback = g
        try {
            appContext.startActivity(ReadClipActivity.intent(appContext, g.signal, g.id))
        } catch (e: Exception) {
            pendingFallback = null
            finishAuto(g, null, focused = false, note = "$reason; fallback threw ${e.javaClass.simpleName}")
            return
        }
        main.postDelayed({
            if (pendingFallback?.id == g.id) {
                pendingFallback = null
                store.inc("fallback.timeout")
                finishAuto(g, null, focused = false, note = "$reason; activity fallback blocked or no focus")
            }
        }, ACTIVITY_TIMEOUT_MS)
    }

    private fun finishAuto(g: PendingGrab, read: ClipRead?, focused: Boolean, note: String) {
        val verdict = if (!focused && read == null) Verdict.NO_FOCUS
        else guard.evaluate(read?.snapshot, System.currentTimeMillis(), requireFresh = true)
        if (verdict == Verdict.STALE && g.attempt == 0 && g.signal == Signal.CLICK_COPY) {
            // The app may not have written the clip yet (async copy). One more try.
            store.inc("retry.stale")
            main.postDelayed({ grab(g.copy(attempt = 1)) }, RETRY_DELAY_MS)
            return
        }
        val latency = SystemClock.uptimeMillis() - g.t0
        record(g.signal, g.pkg, verdict, read, latency, if (g.attempt > 0) "$note; after retry" else note)
        grabInProgress = false
        lastGrabEnd = SystemClock.uptimeMillis()
    }

    // ---- manual paths -----------------------------------------------------------------------------

    /** Called by [ReadClipActivity] after it got (or did not get) window focus. */
    fun onActivityRead(signal: Signal, requestId: Long, read: ClipRead?, focused: Boolean): Verdict? {
        if (requestId > 0) {
            val g = pendingFallback
            if (g != null && g.id == requestId) {
                pendingFallback = null
                store.inc("fallback.ok")
                finishAuto(g, read, focused, "activity fallback")
            }
            return null // a late fallback for a request that already timed out: ignore
        }
        return manualRead(signal, read, focused)
    }

    /** A read we were allowed to do because we are the focused app (tile, notification, app opened, test). */
    fun manualRead(signal: Signal, read: ClipRead?, focused: Boolean): Verdict {
        val verdict = if (!focused && read == null) Verdict.NO_FOCUS
        else guard.evaluate(read?.snapshot, System.currentTimeMillis(), requireFresh = false)
        var note = ""
        if (verdict == Verdict.ACCEPTED && signal != Signal.SELF_TEST && read?.isSelfTest != true) {
            // A new clip that only a manual path found = the automatic layer missed a copy.
            val since = store.long("svc.firstConnectedAt")
            val ts = read?.snapshot?.timestampMs ?: 0L
            if (since > 0 && (ts == 0L || ts >= since)) {
                store.inc("miss")
                note = "MISSED by automatic layer"
            }
        }
        val pkg = if (signal == Signal.SELF_TEST) appContext.packageName else foregroundPkg?.let { "~$it" }
        record(signal, pkg, verdict, read, 0, note)
        return verdict
    }

    /** Share target / PROCESS_TEXT: content arrives in the intent, no clipboard involved. */
    fun recordShared(signal: Signal, kind: String, content: String, preview: String, mime: String, pkg: String?): Verdict {
        val snap = ClipSnapshot(0, ContentHash.of(salt, kind, content), sensitive = false)
        val verdict = guard.evaluate(snap, System.currentTimeMillis(), requireFresh = false)
        record(signal, pkg, verdict, ClipRead(snap, kind, preview, mime, content.length, false), 0, "")
        return verdict
    }

    private fun record(signal: Signal, pkg: String?, verdict: Verdict, read: ClipRead?, latencyMs: Long, note: String) {
        val now = System.currentTimeMillis()
        store.putLong("guard.lastTs", guard.lastSeenTimestamp)
        store.inc("v.${verdict.name}")
        store.inc((if (signal.automatic) "v.auto." else "v.manual.") + verdict.name)
        if (verdict == Verdict.ACCEPTED) {
            store.inc("ok.${signal.name}")
            if (signal.automatic) {
                store.inc("lat.count")
                store.inc("lat.sum", latencyMs)
                store.setMax("lat.max", latencyMs)
            }
        }
        val ts = read?.snapshot?.timestampMs ?: 0L
        val showPreview = verdict.outcome == Outcome.OK || verdict.outcome == Outcome.DUPLICATE
        store.addAttempt(
            Attempt(
                at = now, signal = signal.name, verdict = verdict.name, pkg = pkg,
                preview = if (showPreview) read?.preview else null, kind = read?.kind,
                latencyMs = latencyMs, ageMs = if (ts > 0) now - ts else -1,
                hash = if (verdict == Verdict.ACCEPTED) read?.snapshot?.hash else null, note = note,
            ),
        )
        Log.i(TAG, "capture signal=$signal verdict=$verdict pkg=$pkg kind=${read?.kind} len=${read?.length} latency=${latencyMs}ms")
        changed()
    }

    // ---- clipboard ----------------------------------------------------------------------------------

    /** Only succeeds while our app (activity or overlay window) has input focus. */
    fun readClipboard(ctx: Context): ClipRead? {
        val cm = ctx.getSystemService(ClipboardManager::class.java) ?: return null
        val clip = try { cm.primaryClip } catch (_: Exception) { null } ?: return null
        val desc = clip.description
        val ts = ClipTime.toWallClock(desc.timestamp, System.currentTimeMillis(), SystemClock.elapsedRealtime())
        val sensitive = desc.extras?.getBoolean(EXTRA_IS_SENSITIVE, false) == true
        val mime = if (desc.mimeTypeCount > 0) desc.getMimeType(0) else "?"
        if (clip.itemCount == 0) return ClipRead(ClipSnapshot(ts, null, sensitive), "empty", null, mime, 0, false)
        val item = clip.getItemAt(0)
        val text = item.text?.toString()?.takeIf { it.isNotEmpty() } ?: item.htmlText?.takeIf { it.isNotEmpty() }
        val uri = item.uri
        val intent = item.intent
        return when {
            text != null -> ClipRead(
                ClipSnapshot(ts, if (sensitive) null else ContentHash.of(salt, "text", text), sensitive),
                "text", if (sensitive) null else Preview.of(text), mime, text.length, text.startsWith(TEST_PREFIX),
            )
            uri != null -> {
                val kind = if (mime.startsWith("image/")) "image" else "uri"
                val s = uri.toString()
                ClipRead(ClipSnapshot(ts, if (sensitive) null else ContentHash.of(salt, kind, s), sensitive),
                    kind, if (sensitive) null else "[$kind $mime]", mime, s.length, false)
            }
            intent != null -> {
                val s = intent.toUri(0)
                ClipRead(ClipSnapshot(ts, if (sensitive) null else ContentHash.of(salt, "intent", s), sensitive),
                    "intent", if (sensitive) null else "[intent]", mime, s.length, false)
            }
            else -> ClipRead(ClipSnapshot(ts, null, sensitive), "empty", null, mime, 0, false)
        }
    }

    // ---- copy tests ---------------------------------------------------------------------------------

    private fun testText(what: String): String {
        testCounter++
        val time = SimpleDateFormat("HH:mm:ss", Locale.US).format(Date())
        return "$TEST_PREFIX #$testCounter ($what) $time"
    }

    /** Copies a known string now, then reads it back 1.5 s later from our (focused) activity. */
    fun copyTestNow(activity: Context) {
        val cm = activity.getSystemService(ClipboardManager::class.java) ?: return
        cm.setPrimaryClip(ClipData.newPlainText("ClipTrail test", testText("in-app")))
        main.postDelayed({ manualRead(Signal.SELF_TEST, readClipboard(activity), focused = true) }, 1_500)
    }

    /** Copies a known string after 5 s, while the user is in another app: does the phone emit a toast/overlay? */
    fun copyTestDelayed() {
        val text = testText("background")
        main.postDelayed({
            val cm = appContext.getSystemService(ClipboardManager::class.java) ?: return@postDelayed
            cm.setPrimaryClip(ClipData.newPlainText("ClipTrail test", text))
            store.inc("test.background_copies")
            changed()
        }, 5_000)
    }

    fun reset() {
        store.resetAll()
        guard.clear()
        if (serviceConnected) {
            store.putLong("svc.firstConnectedAt", System.currentTimeMillis())
            store.putLong("svc.connectedAt", System.currentTimeMillis())
        }
        changed()
    }

    // ---- report -----------------------------------------------------------------------------------------

    /** Plain-text report for the user to share manually. Contains no clip text. */
    fun report(statusLines: List<String>): String = buildString {
        appendLine("ClipTrail Android capture spike report")
        appendLine("${Build.MANUFACTURER} ${Build.MODEL} · Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}) · ${Build.DISPLAY}")
        appendLine()
        statusLines.forEach { appendLine(it) }
        appendLine()
        append(countersText())
        appendLine()
        appendLine("Event signatures (type | package | class | count | signal | last text):")
        store.signatures(80).forEach {
            appendLine("${it.type} | ${it.pkg} | ${it.cls} | ${it.count} | ${it.signal ?: "-"} | ${it.lastText ?: ""}")
        }
        appendLine()
        appendLine("Last attempts (no clip text):")
        store.attempts(50).forEach {
            appendLine("${fmtTime(it.at)} ${it.signal} ${it.verdict} ${it.pkg ?: "?"} ${it.kind ?: ""} ${it.latencyMs}ms age=${it.ageMs}ms ${it.note}")
        }
    }

    fun countersText(): String {
        val c = store.counters()
        fun n(k: String) = c[k] ?: 0L
        return buildString {
            val autoOk = n("v.auto.ACCEPTED")
            val miss = n("miss")
            val rate = if (autoOk + miss > 0) "%.0f%%".format(Locale.US, 100.0 * autoOk / (autoOk + miss)) else "n/a"
            appendLine("AUTOMATIC CAPTURE RATE: $rate  ($autoOk caught automatically, $miss missed and found later by a manual path)")
            val latCount = n("lat.count")
            if (latCount > 0) appendLine("Latency trigger→saved: avg ${n("lat.sum") / latCount} ms, max ${n("lat.max")} ms")
            val fc = n("overlay.focus_count")
            if (fc > 0) appendLine("Overlay focus grab: avg ${n("overlay.focus_ms_sum") / fc} ms, max ${n("overlay.focus_ms_max")} ms, failed ${n("overlay.fail")}x")
            appendLine()
            appendLine("Triggers seen:")
            Signal.entries.filter { it.automatic }.forEach {
                appendLine("  ${it.label}: ${n("trig.${it.name}")}  (won ${n("ok.${it.name}")} captures)")
            }
            appendLine("  merged into a running grab: ${n("trig.merged")} · ignored in cooldown: ${n("trig.cooldown")}")
            appendLine()
            appendLine("Results:")
            appendLine("  captured: ${n("v.ACCEPTED")} (auto ${n("v.auto.ACCEPTED")}, manual ${n("v.manual.ACCEPTED")})")
            Signal.entries.filter { !it.automatic }.forEach {
                val k = n("ok.${it.name}")
                if (k > 0) appendLine("    via ${it.label}: $k")
            }
            appendLine("  duplicates: same clip ${n("v.DUPLICATE_SAME_CLIP")}, same content ${n("v.DUPLICATE_CONTENT")}")
            appendLine("Failures / drops:")
            listOf(Verdict.NO_FOCUS, Verdict.STALE, Verdict.EMPTY, Verdict.SENSITIVE, Verdict.BLOCKED_APP).forEach {
                appendLine("  ${it.label}: ${n("v.${it.name}")}")
            }
            appendLine("  stale→retried: ${n("retry.stale")} · activity fallback: launched ${n("fallback.launch")}, ok ${n("fallback.ok")}, timed out ${n("fallback.timeout")}")
            appendLine()
            appendLine("Side effects: \"pasted from your clipboard\" toasts seen: ${n("side.access_toast")}")
            appendLine("Service connects: ${n("svc.connects")} · background test copies: ${n("test.background_copies")}")
        }
    }

    fun fmtTime(ms: Long): String = SimpleDateFormat("MM-dd HH:mm:ss", Locale.US).format(Date(ms))

    private fun newSalt(): String {
        val b = ByteArray(16)
        SecureRandom().nextBytes(b)
        return b.joinToString("") { "%02x".format(it) }
    }
}
