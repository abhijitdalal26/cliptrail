package dev.cliptrail.app

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.PersistableBundle
import android.os.PowerManager
import android.os.SystemClock
import android.provider.Settings
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanIntentResult
import com.journeyapps.barcodescanner.ScanOptions
import java.io.File

@InvokeArg
class EnabledArgs {
    var enabled: Boolean = false
}

@InvokeArg
class OpenArgs {
    var target: String = ""
}

@InvokeArg
class DarkArgs {
    var dark: Boolean = false
    var panel: Boolean = false
}

@InvokeArg
class WriteArgs {
    var text: String? = null
    var html: String? = null
    var imagePath: String? = null
    var sensitive: Boolean = false
}

/**
 * Android APIs for the UI while it's open. Registered from Rust (src-tauri/src/mobile/bridge.rs) and
 * only called through our Rust commands, never directly from JavaScript.
 */
@TauriPlugin
class ClipTrailPlugin(private val activity: Activity) : Plugin(activity) {

    override fun load(webView: WebView) {
        CaptureEngine.init(activity)
    }

    // ---- status / onboarding ---------------------------------------------------------------------

    private fun a11yComponent() = ComponentName(activity, CaptureAccessibilityService::class.java)

    private fun a11yEnabled(): Boolean {
        val enabled = Settings.Secure.getString(activity.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES)
            ?: return false
        return enabled.split(':').any { ComponentName.unflattenFromString(it) == a11yComponent() }
    }

    private fun notificationsAllowed(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    private fun batteryUnrestricted(): Boolean =
        activity.getSystemService(PowerManager::class.java)?.isIgnoringBatteryOptimizations(activity.packageName) == true

    private fun statusObject(): JSObject {
        val ctx = activity
        return JSObject().apply {
            put("autoCapture", Prefs.autoCapture(ctx))
            put("serviceEnabled", a11yEnabled())
            put("serviceConnected", CaptureEngine.serviceConnected)
            put("notificationsAllowed", notificationsAllowed())
            put("batteryUnrestricted", batteryUnrestricted())
            put("keepAliveRunning", KeepAliveService.running)
            put("overlayAllowed", Settings.canDrawOverlays(ctx))
            put("restrictedSettings", Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU)
            put("samsung", Build.MANUFACTURER.equals("samsung", ignoreCase = true))
            put("sdk", Build.VERSION.SDK_INT)
            put("device", NativeCore.deviceName(ctx))
            put("lastCaptureAt", Prefs.lastCaptureAt(ctx))
            put("lastSignal", Prefs.lastSignal(ctx))
            put("autoCaptures", Prefs.count(ctx, "auto"))
            put("manualCaptures", Prefs.count(ctx, "manual"))
            put("misses", Prefs.count(ctx, "miss"))
        }
    }

    @Command
    fun status(invoke: Invoke) {
        invoke.resolve(statusObject())
    }

    @Command
    fun setAutoCapture(invoke: Invoke) {
        val args = invoke.parseArgs(EnabledArgs::class.java)
        Prefs.setAutoCapture(activity, args.enabled)
        if (args.enabled) KeepAliveService.start(activity) else KeepAliveService.stop(activity)
        invoke.resolve(statusObject())
    }

    @Command
    fun open(invoke: Invoke) {
        val target = invoke.parseArgs(OpenArgs::class.java).target
        val pkg = activity.packageName
        val ok = when (target) {
            // The per-service page needs a privileged permission, so this normally lands on the
            // Accessibility list; the fragment key highlights our entry where Settings supports it.
            "accessibility" -> tryStart(
                Intent("android.settings.ACCESSIBILITY_DETAILS_SETTINGS")
                    .putExtra(Intent.EXTRA_COMPONENT_NAME, a11yComponent().flattenToString()),
                Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)
                    .putExtra(":settings:fragment_args_key", a11yComponent().flattenToString()),
            )
            "appInfo" -> tryStart(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", pkg, null)))
            "batteryOptimization" -> tryStart(
                Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.fromParts("package", pkg, null)),
                Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
            )
            // Samsung Device Care battery screen ("Background usage limits" lives here); stock fallbacks.
            "battery" -> tryStart(
                Intent().setComponent(ComponentName("com.samsung.android.lool", "com.samsung.android.sm.battery.ui.BatteryActivity")),
                Intent(Intent.ACTION_POWER_USAGE_SUMMARY),
                Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
            )
            "notifications" -> tryStart(
                Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, pkg),
            )
            "overlay" -> tryStart(Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.fromParts("package", pkg, null)))
            else -> false
        }
        if (ok) invoke.resolve(JSObject().put("opened", true)) else invoke.reject("Couldn't open that screen on this phone")
    }

    private fun tryStart(vararg intents: Intent): Boolean {
        for (i in intents) {
            try {
                activity.startActivity(i)
                return true
            } catch (_: Exception) {
                // try the next one
            }
        }
        return false
    }

    @Command
    fun requestNotifications(invoke: Invoke) {
        if (notificationsAllowed()) {
            invoke.resolve(JSObject().put("granted", true))
            return
        }
        val wry = activity as? WryActivity
        if (wry == null) {
            tryStart(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName))
            invoke.resolve(JSObject().put("granted", false))
            return
        }
        val asked = SystemClock.uptimeMillis()
        wry.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS)) { granted ->
            if (granted == true && Prefs.autoCapture(activity)) KeepAliveService.start(activity)
            // Denied before: Android answers at once without showing the dialog, so open the setting
            // instead. A slower "no" is the user's own answer to the dialog; respect it.
            if (granted != true && SystemClock.uptimeMillis() - asked < 400) {
                tryStart(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName))
            }
            invoke.resolve(JSObject().put("granted", granted == true))
        }
    }

    // ---- clipboard ---------------------------------------------------------------------------------

    @Command
    fun writeClipboard(invoke: Invoke) {
        val args = invoke.parseArgs(WriteArgs::class.java)
        val cm = activity.getSystemService(ClipboardManager::class.java)
        if (cm == null) {
            invoke.reject("Clipboard unavailable")
            return
        }
        try {
            val path = args.imagePath
            val clip = when {
                path != null -> {
                    val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", File(path))
                    ClipData.newUri(activity.contentResolver, "Image", uri)
                }
                args.html != null -> ClipData.newHtmlText("ClipTrail", args.text ?: "", args.html)
                else -> ClipData.newPlainText("ClipTrail", args.text ?: "")
            }
            if (args.sensitive) {
                clip.description.extras = PersistableBundle().apply { putBoolean(ClipReader.EXTRA_IS_SENSITIVE, true) }
            }
            cm.setPrimaryClip(clip)
            CaptureEngine.noteOwnClip(activity)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Couldn't copy: ${e.javaClass.simpleName}")
        }
    }

    /**
     * Plain text on the clipboard, for the "Paste" button next to the pairing-link box: the WebView
     * denies navigator.clipboard.readText. The app is focused, so the read is allowed. Never logged.
     */
    @Command
    fun readClipboardText(invoke: Invoke) {
        val cm = activity.getSystemService(ClipboardManager::class.java)
        val text = try {
            cm?.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(activity)?.toString()
        } catch (_: Exception) {
            null
        }
        invoke.resolve(JSObject().put("text", text ?: ""))
    }

    /** The in-app "Save clipboard" button (the app is focused, so the read is allowed). */
    @Command
    fun saveClipboard(invoke: Invoke) {
        val read = CaptureEngine.readClipboard(activity)
        CaptureEngine.manualRead(Signal.IN_APP, read, focused = true) { msg ->
            invoke.resolve(JSObject().put("message", msg ?: ""))
        }
    }

    // ---- pairing -------------------------------------------------------------------------------------

    @Command
    fun scanQr(invoke: Invoke) {
        val options = ScanOptions()
            .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
            .setPrompt("Scan the code shown on your computer")
            .setBeepEnabled(false)
            .setOrientationLocked(false)
        val intent = ScanContract().createIntent(activity, options)
        startActivityForResult(invoke, intent, "onScanResult")
    }

    @ActivityCallback
    private fun onScanResult(invoke: Invoke, result: ActivityResult) {
        val r: ScanIntentResult = ScanIntentResult.parseActivityResult(result.resultCode, result.data)
        val text = r.contents
        if (text != null) invoke.resolve(JSObject().put("text", text)) else invoke.resolve(JSObject().put("cancelled", true))
    }

    /** A cliptrail://pair link that opened the app (camera app, browser, adb). Returned once. */
    @Command
    fun takePairLink(invoke: Invoke) {
        val link = PairLinks.take()
        invoke.resolve(JSObject().put("link", link))
    }

    override fun onNewIntent(intent: Intent) {
        PairLinks.offer(intent)
    }

    // ---- window ----------------------------------------------------------------------------------

    @Command
    fun setBarStyle(invoke: Invoke) {
        val args = invoke.parseArgs(DarkArgs::class.java)
        activity.runOnUiThread {
            MainActivity.applyBarStyle(activity, args.dark, args.panel)
        }
        invoke.resolve()
    }

    @Command
    fun minimize(invoke: Invoke) {
        activity.runOnUiThread { activity.moveTaskToBack(true) }
        invoke.resolve()
    }
}
