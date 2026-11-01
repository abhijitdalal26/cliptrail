package dev.cliptrail.spike

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast

// Share target (ACTION_SEND with text/plain or any image type) and ACTION_PROCESS_TEXT
// ("ClipTrail" in the text-selection menu).
class IntentCaptureActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        CaptureEngine.init(this)
        val verdict = handle(intent)
        val msg = verdict?.let { getString(R.string.toast_result, it.label) } ?: getString(R.string.toast_nothing)
        Toast.makeText(applicationContext, msg, Toast.LENGTH_SHORT).show()
        finish()
    }

    private fun handle(i: Intent): Verdict? {
        val from = referrer?.host
        return when (i.action) {
            Intent.ACTION_PROCESS_TEXT -> {
                val text = i.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString()?.takeIf { it.isNotEmpty() }
                    ?: return null
                CaptureEngine.recordShared(Signal.PROCESS_TEXT, "text", text, Preview.of(text), "text/plain", from)
            }
            Intent.ACTION_SEND -> {
                val mime = i.type ?: "?"
                val text = i.getStringExtra(Intent.EXTRA_TEXT)?.takeIf { it.isNotEmpty() }
                val uri = streamUri(i)
                when {
                    mime.startsWith("image/") && uri != null ->
                        // Spike: record that an image arrived; bytes are not copied (M5 does that).
                        CaptureEngine.recordShared(Signal.SHARE, "image", uri.toString(), "[image $mime]", mime, from)
                    text != null -> CaptureEngine.recordShared(Signal.SHARE, "text", text, Preview.of(text), mime, from)
                    else -> null
                }
            }
            else -> null
        }
    }

    private fun streamUri(i: Intent): Uri? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            i.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION")
            i.getParcelableExtra(Intent.EXTRA_STREAM)
        }
}
