package dev.cliptrail.app

import android.content.Context
import android.content.SharedPreferences
import java.security.SecureRandom

/**
 * Small Android-side settings and capture-health counters. Never holds clip content:
 * only a timestamp guard, counters and which signal last captured something.
 */
object Prefs {
    private fun sp(ctx: Context): SharedPreferences =
        ctx.applicationContext.getSharedPreferences("cliptrail", Context.MODE_PRIVATE)

    /** "Automatic capture (beta)": the accessibility trigger. On by default. */
    fun autoCapture(ctx: Context): Boolean = sp(ctx).getBoolean("autoCapture", true)

    fun setAutoCapture(ctx: Context, on: Boolean) {
        sp(ctx).edit().putBoolean("autoCapture", on).apply()
    }

    /** Salt for the in-memory content hashes of the dedup guard. */
    fun salt(ctx: Context): String {
        sp(ctx).getString("salt", null)?.let { return it }
        val b = ByteArray(16)
        SecureRandom().nextBytes(b)
        val s = b.joinToString("") { "%02x".format(it) }
        sp(ctx).edit().putString("salt", s).apply()
        return s
    }

    /** ClipDescription timestamp of the last clip we decided on (survives restarts). */
    fun lastClipTs(ctx: Context): Long = sp(ctx).getLong("guard.lastTs", 0)

    fun setLastClipTs(ctx: Context, ts: Long) {
        if (ts > 0) sp(ctx).edit().putLong("guard.lastTs", ts).apply()
    }

    fun recordCapture(ctx: Context, signal: Signal) {
        val p = sp(ctx)
        p.edit()
            .putLong("last.at", System.currentTimeMillis())
            .putString("last.signal", signal.name)
            .putLong(if (signal.automatic) "count.auto" else "count.manual", p.getLong(if (signal.automatic) "count.auto" else "count.manual", 0) + 1)
            .apply()
    }

    /** A manual path found a new clip while automatic capture was on: the trigger missed it. */
    fun recordMiss(ctx: Context) {
        val p = sp(ctx)
        p.edit().putLong("count.miss", p.getLong("count.miss", 0) + 1).apply()
    }

    fun lastCaptureAt(ctx: Context): Long = sp(ctx).getLong("last.at", 0)
    fun lastSignal(ctx: Context): String? = sp(ctx).getString("last.signal", null)
    fun count(ctx: Context, name: String): Long = sp(ctx).getLong("count.$name", 0)

    fun putState(ctx: Context, key: String, value: String) {
        sp(ctx).edit().putString("state.$key", value).apply()
    }

    fun state(ctx: Context, key: String): String? = sp(ctx).getString("state.$key", null)
}
