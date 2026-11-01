package dev.cliptrail.spike

import android.accessibilityservice.AccessibilityService
import android.app.Notification
import android.content.Intent
import android.graphics.PixelFormat
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.view.accessibility.AccessibilityEvent

/**
 * Trigger layer. An accessibility service cannot read the clipboard itself (Android 10+ allows only the
 * focused app and the default IME), so on a copy signal it adds a 1x1 TYPE_ACCESSIBILITY_OVERLAY window,
 * lets it take input focus for one frame, reads the clip, and removes the window.
 * canRetrieveWindowContent=false: we never read the screen, only the event's own fields.
 */
class CaptureAccessibilityService : AccessibilityService() {

    override fun onServiceConnected() {
        super.onServiceConnected()
        CaptureEngine.init(this)
        CaptureEngine.onServiceConnected(this)
        // Is an FGS start allowed from here (background)? Record the answer; the UI starts it too.
        val result = KeepAliveService.start(this)
        CaptureEngine.store.putText("fgs.from_a11y", "$result at ${CaptureEngine.fmtTime(System.currentTimeMillis())}")
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        event ?: return
        val kind = when (event.eventType) {
            AccessibilityEvent.TYPE_VIEW_CLICKED -> EventKind.CLICK
            AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED -> EventKind.WINDOW_STATE
            AccessibilityEvent.TYPE_NOTIFICATION_STATE_CHANGED -> EventKind.NOTIFICATION
            AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED -> EventKind.CONTENT_CHANGED
            else -> EventKind.OTHER
        }
        if (kind == EventKind.OTHER) return
        val pkg = event.packageName?.toString().orEmpty()
        val cls = event.className?.toString().orEmpty()
        // Content-changed events are very frequent; drop the uninteresting ones before allocating anything.
        if (kind == EventKind.CONTENT_CHANGED && pkg != SignalMatcher.SYSTEM_UI &&
            !cls.contains("clip", ignoreCase = true) && !SignalMatcher.isClipboardUiPackage(pkg)
        ) return
        val texts = buildList {
            event.text?.forEach { if (it != null) add(it.toString()) }
            event.contentDescription?.let { add(it.toString()) }
        }
        @Suppress("DEPRECATION") // getParcelableData() is the documented way to tell toasts from notifications
        val isToast = kind == EventKind.NOTIFICATION && event.parcelableData !is Notification
        CaptureEngine.onEvent(A11yEvent(kind, pkg, cls, texts, isToast))
    }

    override fun onInterrupt() = Unit

    override fun onUnbind(intent: Intent?): Boolean {
        CaptureEngine.onServiceDisconnected(this)
        return super.onUnbind(intent)
    }

    override fun onDestroy() {
        CaptureEngine.onServiceDisconnected(this)
        super.onDestroy()
    }

    /**
     * One-frame focus grab. [done] gets (gotWindowFocus, read, error). Runs on the main thread.
     * FLAG_ALT_FOCUSABLE_IM + SOFT_INPUT_STATE_UNCHANGED try to keep the keyboard attached to the app below.
     */
    fun grabFocusAndRead(timeoutMs: Long, done: (Boolean, ClipRead?, String?) -> Unit) {
        val wm = getSystemService(WindowManager::class.java)
        if (wm == null) {
            done(false, null, "no WindowManager")
            return
        }
        var finished = false
        val view = object : View(this) {
            override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
                super.onWindowFocusChanged(hasWindowFocus)
                if (!hasWindowFocus || finished) return
                finished = true
                val read = CaptureEngine.readClipboard(this@CaptureAccessibilityService)
                removeQuietly(wm, this)
                done(true, read, null)
            }
        }
        val lp = WindowManager.LayoutParams(
            1, 1,
            WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
                WindowManager.LayoutParams.FLAG_ALT_FOCUSABLE_IM or
                WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            softInputMode = WindowManager.LayoutParams.SOFT_INPUT_STATE_UNCHANGED
            title = "ClipTrailFocusGrab"
        }
        val started = SystemClock.uptimeMillis()
        try {
            wm.addView(view, lp)
        } catch (e: Exception) {
            done(false, null, "addView failed: ${e.javaClass.simpleName}")
            return
        }
        CaptureEngine.main.postDelayed({
            if (finished) return@postDelayed
            finished = true
            // No focus callback in time; a read may still work if focus arrived without the callback.
            val read = CaptureEngine.readClipboard(this)
            removeQuietly(wm, view)
            done(false, read, "no window focus after ${SystemClock.uptimeMillis() - started} ms")
        }, timeoutMs)
    }

    private fun removeQuietly(wm: WindowManager, view: View) {
        try {
            wm.removeViewImmediate(view)
        } catch (_: Exception) {
            // already gone
        }
    }
}
