package dev.cliptrail.app

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.widget.Toast

/**
 * Invisible, no-history activity that exists only to become the focused app for a moment and read the
 * clipboard. Used by the Quick Settings tile, the notification action, and as the background fallback
 * when the accessibility overlay could not get focus.
 */
class ReadClipActivity : Activity() {

    companion object {
        private const val EXTRA_SIGNAL = "signal"
        private const val EXTRA_REQUEST = "request"
        private const val TIMEOUT_MS = 1_500L

        fun intent(ctx: Context, signal: Signal, requestId: Long = 0): Intent =
            Intent(ctx, ReadClipActivity::class.java)
                .putExtra(EXTRA_SIGNAL, signal.name)
                .putExtra(EXTRA_REQUEST, requestId)
                .addFlags(
                    Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_ANIMATION or
                        Intent.FLAG_ACTIVITY_EXCLUDE_FROM_RECENTS or Intent.FLAG_ACTIVITY_MULTIPLE_TASK,
                )
    }

    private var done = false
    private lateinit var signal: Signal
    private var requestId = 0L
    private val timeout = Runnable { complete(null, focused = false) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        CaptureEngine.init(this)
        signal = intent.getStringExtra(EXTRA_SIGNAL)?.let { runCatching { Signal.valueOf(it) }.getOrNull() }
            ?: Signal.QS_TILE
        requestId = intent.getLongExtra(EXTRA_REQUEST, 0)
        CaptureEngine.main.postDelayed(timeout, TIMEOUT_MS)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && !done) complete(CaptureEngine.readClipboard(this), focused = true)
    }

    private fun complete(read: ClipContent?, focused: Boolean) {
        if (done) return
        done = true
        CaptureEngine.main.removeCallbacks(timeout)
        val app = applicationContext
        CaptureEngine.onActivityRead(signal, requestId, read, focused) { msg ->
            if (msg != null) Toast.makeText(app, msg, Toast.LENGTH_SHORT).show()
        }
        finish()
    }

    override fun onDestroy() {
        CaptureEngine.main.removeCallbacks(timeout)
        super.onDestroy()
    }
}
