package dev.cliptrail.app

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast

/**
 * Share target (ACTION_SEND / SEND_MULTIPLE with text or images) and ACTION_PROCESS_TEXT
 * ("Save to ClipTrail" in the text-selection menu). Content arrives in the intent; no clipboard access.
 * Stays open (invisibly) until shared images are read, because the read grant ends with the activity.
 */
class IntentCaptureActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        CaptureEngine.init(this)
        val from = referrer?.host?.takeIf { it != packageName }
        if (!handle(intent, from)) finishWith(getString(R.string.msg_nothing))
    }

    private fun finishWith(msg: String) {
        Toast.makeText(applicationContext, msg, Toast.LENGTH_SHORT).show()
        finish()
    }

    /** True when a save was started (it finishes the activity when done). */
    private fun handle(i: Intent, from: String?): Boolean {
        when (i.action) {
            Intent.ACTION_PROCESS_TEXT -> {
                val text = i.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString()?.takeIf { it.isNotBlank() }
                    ?: return false
                CaptureEngine.saveSharedText(text, from, Signal.PROCESS_TEXT, ::finishWith)
                return true
            }
            Intent.ACTION_SEND -> {
                val mime = i.type ?: ""
                val uri = streamUri(i)
                if (mime.startsWith("image/") && uri != null) {
                    CaptureEngine.saveSharedImages(listOf(uri), from, ::finishWith)
                    return true
                }
                val text = i.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()?.takeIf { it.isNotBlank() }
                    ?: return false
                CaptureEngine.saveSharedText(text, from, Signal.SHARE, ::finishWith)
                return true
            }
            Intent.ACTION_SEND_MULTIPLE -> {
                if (!(i.type ?: "").startsWith("image/")) return false
                val uris = streamUris(i)
                if (uris.isEmpty()) return false
                CaptureEngine.saveSharedImages(uris, from, ::finishWith)
                return true
            }
        }
        return false
    }

    private fun streamUri(i: Intent): Uri? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            i.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION")
            i.getParcelableExtra(Intent.EXTRA_STREAM)
        }

    private fun streamUris(i: Intent): List<Uri> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            i.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java) ?: emptyList()
        } else {
            @Suppress("DEPRECATION")
            i.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM) ?: emptyList()
        }
}
