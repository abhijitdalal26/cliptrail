package dev.cliptrail.app

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.widget.RemoteViews

/** Home screen widget "Save clipboard": a tap launches the invisible read activity. */
class SaveClipWidget : AppWidgetProvider() {

    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        val tap = PendingIntent.getActivity(
            context, 3, ReadClipActivity.intent(context, Signal.WIDGET),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val views = RemoteViews(context.packageName, R.layout.widget_save).apply {
            setOnClickPendingIntent(android.R.id.background, tap)
        }
        manager.updateAppWidget(ids, views)
    }
}
