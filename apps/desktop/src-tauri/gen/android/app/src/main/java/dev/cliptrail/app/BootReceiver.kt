package dev.cliptrail.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** After a reboot or an app update: bring the keep-alive service back (process start also restarts sync). */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED -> {
                CaptureEngine.init(context)
                if (Prefs.autoCapture(context)) Prefs.putState(context, "fgs.from_boot", KeepAliveService.start(context))
            }
        }
    }
}
