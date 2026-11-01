package dev.cliptrail.app

import android.content.ClipboardManager
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.SystemClock
import android.text.Html
import android.util.Log
import java.io.ByteArrayOutputStream

enum class ContentKind { TEXT, IMAGE, OTHER, EMPTY }

/** One clipboard read. Text never goes to logcat; only kind and length do. */
data class ClipContent(
    val snapshot: ClipSnapshot,
    val kind: ContentKind,
    val text: String?,
    val html: String?,
    val uri: Uri?,
    val mime: String,
    val length: Int,
)

object ClipReader {
    private const val TAG = "ClipTrail"
    /** ClipDescription.EXTRA_IS_SENSITIVE (API 33); honored on every version that sets it. */
    const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
    /** Images are downscaled so neither side exceeds this. */
    private const val MAX_SIDE = 4096

    /** Only succeeds while our app (an activity or the overlay window) has input focus. */
    fun read(ctx: Context, salt: String): ClipContent? {
        val cm = ctx.getSystemService(ClipboardManager::class.java) ?: return null
        val clip = try {
            cm.primaryClip
        } catch (_: Exception) {
            null
        } ?: return null
        val desc = clip.description
        val ts = ClipTime.toWallClock(desc.timestamp, System.currentTimeMillis(), SystemClock.elapsedRealtime())
        val sensitive = desc.extras?.getBoolean(EXTRA_IS_SENSITIVE, false) == true
        val mime = if (desc.mimeTypeCount > 0) desc.getMimeType(0) else "?"
        if (clip.itemCount == 0) return ClipContent(ClipSnapshot(ts, null, sensitive), ContentKind.EMPTY, null, null, null, mime, 0)
        val item = clip.getItemAt(0)
        val html = item.htmlText?.takeIf { it.isNotEmpty() }
        val text = item.text?.toString()?.takeIf { it.isNotEmpty() }
            ?: html?.let { Html.fromHtml(it, Html.FROM_HTML_MODE_COMPACT).toString().trim().takeIf { t -> t.isNotEmpty() } }
        val uri = item.uri
        return when {
            text != null -> ClipContent(
                ClipSnapshot(ts, if (sensitive) null else ContentHash.of(salt, "text", text), sensitive),
                ContentKind.TEXT, if (sensitive) null else text, if (sensitive) null else html, null, mime, text.length,
            )
            uri != null && mime.startsWith("image/") -> ClipContent(
                ClipSnapshot(ts, if (sensitive) null else ContentHash.of(salt, "image", uri.toString()), sensitive),
                ContentKind.IMAGE, null, null, if (sensitive) null else uri, mime, 0,
            )
            uri != null -> ClipContent(
                ClipSnapshot(ts, if (sensitive) null else ContentHash.of(salt, "uri", uri.toString()), sensitive),
                ContentKind.OTHER, null, null, null, mime, 0,
            )
            else -> ClipContent(ClipSnapshot(ts, null, sensitive), ContentKind.EMPTY, null, null, null, mime, 0)
        }
    }

    /**
     * Decode an image URI (clipboard or share), downscale to [MAX_SIDE] and re-encode as PNG for the
     * core (whose image codec is PNG-only). Run on a worker thread, right away: clipboard URI grants
     * only last while the clip is current.
     */
    fun imagePng(ctx: Context, uri: Uri): ByteArray? = try {
        val cr = ctx.contentResolver
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        cr.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) {
            null
        } else {
            var sample = 1
            while (bounds.outWidth / sample > MAX_SIDE || bounds.outHeight / sample > MAX_SIDE) sample *= 2
            val opts = BitmapFactory.Options().apply { inSampleSize = sample }
            val bmp = cr.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, opts) }
            bmp?.let { b ->
                val out = ByteArrayOutputStream()
                b.compress(Bitmap.CompressFormat.PNG, 100, out)
                b.recycle()
                out.toByteArray()
            }
        }
    } catch (e: Exception) {
        Log.w(TAG, "image read failed: ${e.javaClass.simpleName}")
        null
    } catch (_: OutOfMemoryError) {
        Log.w(TAG, "image too large to read")
        null
    }
}
