package dev.cliptrail.app

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

/** Quick Settings tile "Save clipboard": launches the invisible read activity and collapses the shade. */
class SaveClipTileService : TileService() {

    override fun onStartListening() {
        super.onStartListening()
        qsTile?.apply {
            state = Tile.STATE_INACTIVE
            label = getString(R.string.save_clipboard)
            updateTile()
        }
    }

    override fun onClick() {
        super.onClick()
        if (isLocked) unlockAndRun { launch() } else launch()
    }

    private fun launch() {
        val intent = ReadClipActivity.intent(this, Signal.QS_TILE)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startActivityAndCollapse(
                PendingIntent.getActivity(this, 2, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
            )
        } else {
            launchLegacy(intent)
        }
    }

    // Only reached below API 34, where the PendingIntent overload does not exist yet.
    @SuppressLint("StartActivityAndCollapseDeprecated")
    @Suppress("DEPRECATION")
    private fun launchLegacy(intent: Intent) = startActivityAndCollapse(intent)
}
