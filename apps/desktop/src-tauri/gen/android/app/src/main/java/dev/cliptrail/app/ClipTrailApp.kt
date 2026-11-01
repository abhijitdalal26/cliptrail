package dev.cliptrail.app

import android.app.Application

/** Opens the Rust clip store before any component runs (activity, service, tile, share target). */
class ClipTrailApp : Application() {
    override fun onCreate() {
        super.onCreate()
        CaptureEngine.init(this)
    }
}
