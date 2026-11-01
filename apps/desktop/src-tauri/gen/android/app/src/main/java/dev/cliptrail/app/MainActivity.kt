package dev.cliptrail.app

import android.app.Activity
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Color
import android.os.Bundle
import android.view.View
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

/** A `cliptrail://pair?...` link that opened the app, waiting for the UI to pick it up. */
object PairLinks {
    @Volatile
    private var pending: String? = null

    fun offer(intent: Intent?) {
        val data = intent?.data ?: return
        if (intent.action == Intent.ACTION_VIEW && data.scheme == "cliptrail" && data.host == "pair") {
            pending = data.toString()
        }
    }

    fun take(): String? {
        val p = pending
        pending = null
        return p
    }
}

class MainActivity : TauriActivity() {
    private var readOnFocus = false

    companion object {
        /** Matches --bg (or --panel) in src/index.css, so the bars blend into the screen under them. */
        private fun background(dark: Boolean, panel: Boolean) = when {
            panel && dark -> Color.rgb(0x15, 0x15, 0x17)
            panel -> Color.rgb(0xff, 0xff, 0xff)
            dark -> Color.rgb(0x0e, 0x0e, 0x10)
            else -> Color.rgb(0xf4, 0xf4, 0xf6)
        }

        fun applyBarStyle(activity: Activity, dark: Boolean, panel: Boolean = false) {
            val window = activity.window
            val color = background(dark, panel)
            window.decorView.setBackgroundColor(color)
            activity.findViewById<View>(android.R.id.content)?.setBackgroundColor(color)
            WindowCompat.getInsetsController(window, window.decorView).apply {
                isAppearanceLightStatusBars = !dark
                isAppearanceLightNavigationBars = !dark
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        // Open the clip store before Tauri starts the Rust side of the UI.
        CaptureEngine.init(this)
        PairLinks.offer(intent)
        super.onCreate(savedInstanceState)
        val night = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        applyBarStyle(this, night)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        PairLinks.offer(intent)
    }

    override fun onWebViewCreate(webView: WebView) {
        // Edge-to-edge: keep the page out from under the status bar, navigation bar, cutout and keyboard.
        val content = findViewById<View>(android.R.id.content) ?: return
        ViewCompat.setOnApplyWindowInsetsListener(content) { v, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            v.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, ime.bottom))
            WindowInsetsCompat.CONSUMED
        }
        ViewCompat.requestApplyInsets(content)
    }

    override fun onResume() {
        super.onResume()
        readOnFocus = true
        if (Prefs.autoCapture(this) && !KeepAliveService.running) {
            Prefs.putState(this, "fgs.from_ui", KeepAliveService.start(this))
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        // Manual path: we are the focused app now, so we may read the clipboard once.
        if (hasFocus && readOnFocus) {
            readOnFocus = false
            CaptureEngine.manualRead(Signal.APP_FOREGROUND, CaptureEngine.readClipboard(this), focused = true, done = null)
        }
    }
}
