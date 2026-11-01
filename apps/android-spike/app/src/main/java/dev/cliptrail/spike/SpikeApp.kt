package dev.cliptrail.spike

import android.app.Application

class SpikeApp : Application() {
    override fun onCreate() {
        super.onCreate()
        CaptureEngine.init(this)
    }
}
