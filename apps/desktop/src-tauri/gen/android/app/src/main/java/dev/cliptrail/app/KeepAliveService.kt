package dev.cliptrail.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.os.Build
import android.os.IBinder

/**
 * Foreground service (type specialUse) with a quiet persistent notification and a "Save clipboard"
 * action. It keeps the process alive against OEM background killing, which keeps automatic capture and
 * the LAN sync connection to the paired computer running. Runs while automatic capture is on.
 */
class KeepAliveService : Service() {

    companion object {
        private const val CHANNEL = "capture"
        private const val NOTIFICATION_ID = 1

        @Volatile
        var running = false
            private set

        /** Returns a short result; ForegroundServiceStartNotAllowedException shows up here. */
        fun start(ctx: Context): String = try {
            ctx.startForegroundService(Intent(ctx, KeepAliveService::class.java))
            "start requested"
        } catch (e: Exception) {
            "denied: ${e.javaClass.simpleName}"
        }

        fun stop(ctx: Context) {
            ctx.stopService(Intent(ctx, KeepAliveService::class.java))
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        CaptureEngine.init(this)
        val nm = getSystemService(NotificationManager::class.java)
        nm?.createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.channel_name), NotificationManager.IMPORTANCE_LOW).apply {
                setShowBadge(false)
            },
        )
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                startForeground(NOTIFICATION_ID, buildNotification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
            } else {
                startForeground(NOTIFICATION_ID, buildNotification())
            }
            running = true
            Prefs.putState(this, "fgs", "running")
        } catch (e: Exception) {
            Prefs.putState(this, "fgs", "startForeground failed: ${e.javaClass.simpleName}")
            stopSelf()
        }
        return START_STICKY
    }

    override fun onDestroy() {
        running = false
        super.onDestroy()
    }

    private fun buildNotification(): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val save = PendingIntent.getActivity(
            this, 1, ReadClipActivity.intent(this, Signal.NOTIFICATION),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val icon = Icon.createWithResource(this, R.drawable.ic_clip)
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_clip)
            .setContentTitle(getString(R.string.notif_title))
            .setContentText(getString(R.string.notif_text))
            .setOngoing(true)
            .setShowWhen(false)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setContentIntent(open)
            .addAction(Notification.Action.Builder(icon, getString(R.string.save_clipboard), save).build())
            .build()
    }
}
