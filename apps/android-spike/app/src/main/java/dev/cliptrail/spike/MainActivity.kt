package dev.cliptrail.spike

import android.Manifest
import android.app.Activity
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast

/** One screen: status checklist, Samsung onboarding, counters, copy tests, last attempts, event signatures. */
class MainActivity : Activity() {

    private lateinit var statusBox: LinearLayout
    private lateinit var countersView: TextView
    private lateinit var attemptsView: TextView
    private lateinit var signaturesView: TextView
    private var readOnNextFocus = false
    private val refreshRunnable = Runnable { refresh() }
    private val listener: () -> Unit = {
        CaptureEngine.main.removeCallbacks(refreshRunnable)
        CaptureEngine.main.postDelayed(refreshRunnable, 250)
    }

    private val a11yComponent by lazy { ComponentName(this, CaptureAccessibilityService::class.java) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        CaptureEngine.init(this)
        setContentView(buildUi())
    }

    override fun onResume() {
        super.onResume()
        CaptureEngine.addListener(listener)
        readOnNextFocus = true
        if (!KeepAliveService.running) {
            CaptureEngine.store.putText("fgs.from_ui", "${KeepAliveService.start(this)} at ${CaptureEngine.fmtTime(System.currentTimeMillis())}")
        }
        refresh()
    }

    override fun onPause() {
        CaptureEngine.removeListener(listener)
        CaptureEngine.main.removeCallbacks(refreshRunnable)
        super.onPause()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        // Manual path 5: read once when our app comes to the front (we are focused, so this is allowed).
        if (hasFocus && readOnNextFocus) {
            readOnNextFocus = false
            CaptureEngine.manualRead(Signal.APP_FOREGROUND, CaptureEngine.readClipboard(this), focused = true)
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        refresh()
    }

    // ---- status -----------------------------------------------------------------------------------

    private data class Check(val ok: Boolean?, val title: String, val detail: String, val actions: List<Pair<String, () -> Unit>>)

    private fun a11yEnabled(): Boolean {
        val enabled = Settings.Secure.getString(contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES) ?: return false
        return enabled.split(':').any { ComponentName.unflattenFromString(it) == a11yComponent }
    }

    private fun notificationsAllowed(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    private fun batteryUnrestricted(): Boolean =
        getSystemService(PowerManager::class.java)?.isIgnoringBatteryOptimizations(packageName) == true

    private fun checks(): List<Check> {
        val enabled = a11yEnabled()
        val connected = CaptureEngine.serviceConnected
        val store = CaptureEngine.store
        val list = mutableListOf(
            Check(
                enabled && connected,
                "Accessibility service: " + when {
                    enabled && connected -> "on and connected"
                    enabled -> "enabled but not connected (toggle it off and on)"
                    else -> "OFF"
                },
                "Settings → Accessibility → Installed apps → ClipTrail Spike → On.",
                listOf("Open accessibility settings" to ::openAccessibility),
            ),
        )
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            list += Check(
                if (enabled) true else null,
                "Restricted settings (Android 13+ sideload)",
                "If the switch is greyed out or you see \"Restricted setting\": first try to enable the service once, " +
                    "then App info → ⋮ (top right) → Allow restricted settings → confirm, and enable the service again.",
                listOf("Open App info" to ::openAppInfo),
            )
        }
        list += Check(
            notificationsAllowed(),
            "Notification permission",
            "Needed to see the \"ClipTrail is capturing\" notification and its Save clipboard button.",
            listOf("Allow notifications" to ::askNotifications),
        )
        list += Check(
            batteryUnrestricted(),
            "Battery: not optimized",
            "Also on Samsung: Settings → Battery → Background usage limits → Never sleeping apps → + → ClipTrail Spike. " +
                "And turn off \"Put unused apps to sleep\".",
            listOf("Allow unrestricted" to ::askBattery, "Battery settings" to ::openBatterySettings),
        )
        list += Check(
            KeepAliveService.running,
            "Foreground service: " + if (KeepAliveService.running) "running" else "not running",
            "state: ${store.text("fgs.state") ?: "-"}\nstart from accessibility: ${store.text("fgs.from_a11y") ?: "-"}\n" +
                "start from app: ${store.text("fgs.from_ui") ?: "-"}",
            listOf("Start service" to {
                store.putText("fgs.from_ui", "${KeepAliveService.start(this)} at ${CaptureEngine.fmtTime(System.currentTimeMillis())}")
                refresh()
            }),
        )
        list += Check(
            if (Settings.canDrawOverlays(this)) true else null,
            "Optional: Appear on top",
            "Only used by the fallback path (invisible activity) when the overlay focus grab fails. Leave off at first; " +
                "turn on only if counters show many \"focus not gained\".",
            listOf("Open setting" to ::openOverlaySetting),
        )
        return list
    }

    private fun statusLines(): List<String> = checks().map { "[${mark(it.ok)}] ${it.title}" }

    private fun mark(ok: Boolean?) = when (ok) { true -> "OK"; false -> "!!"; null -> "--" }

    // ---- actions ----------------------------------------------------------------------------------

    private fun tryStart(vararg intents: Intent) {
        for (i in intents) {
            try {
                startActivity(i)
                return
            } catch (_: Exception) {
                // try the next one
            }
        }
        Toast.makeText(this, "Could not open that screen on this phone", Toast.LENGTH_SHORT).show()
    }

    private fun openAccessibility() = tryStart(
        Intent("android.settings.ACCESSIBILITY_DETAILS_SETTINGS")
            .putExtra(Intent.EXTRA_COMPONENT_NAME, a11yComponent.flattenToString()),
        Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS),
    )

    private fun openAppInfo() = tryStart(
        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", packageName, null)),
    )

    private fun askNotifications() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && !notificationsAllowed()) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        } else {
            tryStart(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, packageName))
        }
    }

    // Sideloaded spike whose whole point is staying alive: the exemption dialog is the right tool here.
    @android.annotation.SuppressLint("BatteryLife")
    private fun askBattery() = tryStart(
        Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.fromParts("package", packageName, null)),
        Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
    )

    private fun openBatterySettings() = tryStart(
        // Samsung Device Care battery screen ("Background usage limits" lives here); falls back to stock screens.
        Intent().setComponent(ComponentName("com.samsung.android.lool", "com.samsung.android.sm.battery.ui.BatteryActivity")),
        Intent(Intent.ACTION_POWER_USAGE_SUMMARY),
        Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
    )

    private fun openOverlaySetting() = tryStart(
        Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.fromParts("package", packageName, null)),
    )

    private fun shareReport() {
        val send = Intent(Intent.ACTION_SEND)
            .setType("text/plain")
            .putExtra(Intent.EXTRA_SUBJECT, "ClipTrail spike report")
            .putExtra(Intent.EXTRA_TEXT, CaptureEngine.report(statusLines()))
        val chooser = Intent.createChooser(send, "Share report (no clip text included)")
            .putExtra(Intent.EXTRA_EXCLUDE_COMPONENTS, arrayOf(ComponentName(this, IntentCaptureActivity::class.java)))
        tryStart(chooser)
    }

    // ---- rendering --------------------------------------------------------------------------------

    private fun refresh() {
        statusBox.removeAllViews()
        for (c in checks()) {
            statusBox.addView(text("${icon(c.ok)}  ${c.title}", 16f, bold = true))
            statusBox.addView(text(c.detail, 13f).apply { alpha = 0.8f })
            val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
            c.actions.forEach { (label, action) -> row.addView(button(label) { action() }) }
            statusBox.addView(row)
            statusBox.addView(spacer(8))
        }
        countersView.text = CaptureEngine.countersText()
        attemptsView.text = CaptureEngine.store.attempts(50).joinToString("\n\n") { a ->
            val ok = a.verdict == Verdict.ACCEPTED.name
            buildString {
                append(if (ok) "✅ " else if (a.verdict.startsWith("DUPLICATE")) "↺ " else "✖ ")
                append(CaptureEngine.fmtTime(a.at)).append(" · ").append(a.signal).append(" · ").append(a.verdict)
                append("\n   ").append(a.pkg ?: "source ?")
                if (a.latencyMs > 0) append(" · ").append(a.latencyMs).append(" ms")
                if (a.ageMs >= 0) append(" · clip age ").append(a.ageMs).append(" ms")
                a.kind?.let { append(" · ").append(it) }
                a.preview?.let { append("\n   “").append(it).append("”") }
                if (a.note.isNotEmpty()) append("\n   ").append(a.note)
            }
        }.ifEmpty { "Nothing yet. Copy something in another app." }
        signaturesView.text = CaptureEngine.store.signatures(60).joinToString("\n") { s ->
            "${s.signal?.let { "[$it] " } ?: ""}${s.type} ${s.pkg} / ${s.cls.substringAfterLast('.')} ×${s.count}" +
                (s.lastText?.let { "  “$it”" } ?: "")
        }.ifEmpty { "No candidate events yet." }
    }

    private fun icon(ok: Boolean?) = when (ok) { true -> "✅"; false -> "⚠️"; null -> "ℹ️" }

    private fun buildUi(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(16), dp(16), dp(32))
        }
        root.addView(text(getString(R.string.app_name), 24f, bold = true))
        root.addView(
            text(
                "${Build.MANUFACTURER} ${Build.MODEL} · Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})\n" +
                    "Measures whether copies in other apps can be captured without a custom keyboard. " +
                    "Everything stays on this phone (no internet permission).",
                13f,
            ).apply { alpha = 0.8f },
        )

        root.addView(header("1. Setup"))
        statusBox = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        root.addView(statusBox)
        root.addView(
            text(
                "Also add the Quick Settings tile: pull down twice → ✎ (edit) → drag \"Save clipboard\" into the panel. " +
                    "In any app, select text → ⋮ in the menu → \"ClipTrail\" saves the selection.",
                13f,
            ),
        )

        root.addView(header("2. Sanity check"))
        val tests = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        tests.addView(button("Copy test now") { CaptureEngine.copyTestNow(this) })
        tests.addView(button("Copy test in 5 s") {
            CaptureEngine.copyTestDelayed()
            Toast.makeText(this, "Switch to another app now", Toast.LENGTH_SHORT).show()
        })
        root.addView(tests)
        root.addView(
            text(
                "\"Copy test in 5 s\": switch to another app; the copy happens there. If a toast/overlay signal exists " +
                    "on this phone, the attempt shows up in the log within a second.",
                13f,
            ).apply { alpha = 0.8f },
        )

        root.addView(header("3. Counters"))
        countersView = text("", 13f).apply { typeface = Typeface.MONOSPACE; setTextIsSelectable(true) }
        root.addView(countersView)
        val actions = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        actions.addView(button("Share report") { shareReport() })
        actions.addView(button("Reset all") {
            CaptureEngine.reset()
            Toast.makeText(this, "Counters and log cleared", Toast.LENGTH_SHORT).show()
        })
        root.addView(actions)

        root.addView(header("4. Last 50 attempts"))
        attemptsView = text("", 13f)
        root.addView(attemptsView)

        root.addView(header("5. Event signatures seen on this phone"))
        root.addView(text("Toasts, SystemUI/clipboard/keyboard windows and Copy-like clicks. [SIGNAL] = used as a trigger.", 12f).apply { alpha = 0.8f })
        signaturesView = text("", 12f).apply { typeface = Typeface.MONOSPACE }
        root.addView(signaturesView)

        val scroll = ScrollView(this).apply {
            addView(root)
            isFillViewport = true
        }
        // targetSdk 35+ is edge-to-edge on Android 15+: pad for the system bars ourselves.
        scroll.setOnApplyWindowInsetsListener { v, insets ->
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            } else {
                @Suppress("DEPRECATION")
                v.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop, insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            }
            insets
        }
        return scroll
    }

    private fun header(s: String) = text(s, 19f, bold = true).apply { setPadding(0, dp(20), 0, dp(6)) }

    private fun text(s: String, sizeSp: Float, bold: Boolean = false) = TextView(this).apply {
        text = s
        textSize = sizeSp
        if (bold) setTypeface(typeface, Typeface.BOLD)
        layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
    }

    private fun button(label: String, onClick: () -> Unit) = Button(this).apply {
        text = label
        isAllCaps = false
        setOnClickListener { onClick() }
        layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginEnd = dp(6) }
    }

    private fun spacer(h: Int) = View(this).apply {
        layoutParams = LinearLayout.LayoutParams(1, dp(h))
    }

    private fun dp(v: Int) = (v * resources.displayMetrics.density).toInt()
}
