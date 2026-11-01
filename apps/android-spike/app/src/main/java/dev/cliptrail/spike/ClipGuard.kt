package dev.cliptrail.spike

import java.security.MessageDigest
import kotlin.math.abs

/** Pure (no Android imports) staleness + dedup guard, unit-tested on the JVM. */
enum class Outcome { OK, DUPLICATE, DROPPED, FAILED }

enum class Verdict(val label: String, val outcome: Outcome) {
    ACCEPTED("captured", Outcome.OK),
    DUPLICATE_SAME_CLIP("same clip again", Outcome.DUPLICATE),
    DUPLICATE_CONTENT("same content re-copied", Outcome.DUPLICATE),
    STALE("stale timestamp", Outcome.FAILED),
    EMPTY("empty clip / read denied", Outcome.FAILED),
    NO_FOCUS("focus not gained", Outcome.FAILED),
    SENSITIVE("sensitive (dropped)", Outcome.DROPPED),
    BLOCKED_APP("blocked app (not read)", Outcome.DROPPED),
}

/**
 * What we learned from one clipboard read.
 * @param timestampMs ClipDescription timestamp in wall-clock ms, 0 when unknown (shared content has none).
 * @param hash salted content hash, null when there is nothing usable (or the clip is sensitive).
 */
data class ClipSnapshot(val timestampMs: Long, val hash: String?, val sensitive: Boolean)

class ClipGuard(val freshWindowMs: Long = 5_000, private val capacity: Int = 500) {
    private val order = ArrayDeque<String>()
    private val known = HashSet<String>()

    /** Timestamp of the last clip we took a decision on (accepted, dup or sensitive). */
    var lastSeenTimestamp: Long = 0
        private set

    /**
     * @param requireFresh true for accessibility triggers: the clip must be younger than [freshWindowMs],
     *   otherwise the trigger was a false positive (or the app has not written the clip yet).
     *   Manual paths (tile, notification, app opened) pass false: the user asked for "whatever is there".
     */
    fun evaluate(snap: ClipSnapshot?, nowMs: Long, requireFresh: Boolean): Verdict {
        if (snap == null) return Verdict.EMPTY
        val ts = snap.timestampMs
        if (ts > 0 && ts == lastSeenTimestamp) return Verdict.DUPLICATE_SAME_CLIP
        if (snap.sensitive) {
            markSeen(ts)
            return Verdict.SENSITIVE
        }
        val hash = snap.hash ?: return Verdict.EMPTY
        if (requireFresh && isStale(ts, nowMs)) return Verdict.STALE
        markSeen(ts)
        if (hash in known) {
            order.remove(hash)
            order.addLast(hash)
            return Verdict.DUPLICATE_CONTENT
        }
        remember(hash)
        return Verdict.ACCEPTED
    }

    /** Unknown timestamps and clock skew into the future count as fresh. */
    fun isStale(timestampMs: Long, nowMs: Long): Boolean = timestampMs > 0 && nowMs - timestampMs > freshWindowMs

    fun restore(hashesOldestFirst: List<String>, lastTimestamp: Long) {
        clear()
        hashesOldestFirst.forEach(::remember)
        lastSeenTimestamp = lastTimestamp
    }

    fun clear() {
        order.clear()
        known.clear()
        lastSeenTimestamp = 0
    }

    private fun markSeen(ts: Long) {
        if (ts > 0) lastSeenTimestamp = ts
    }

    private fun remember(hash: String) {
        if (!known.add(hash)) return
        order.addLast(hash)
        while (order.size > capacity) known.remove(order.removeFirst())
    }
}

object ClipTime {
    /**
     * ClipDescription.getTimestamp() is documented as wall-clock based, but be robust against a ROM that
     * uses elapsedRealtime: pick whichever time base the value is closer to and convert to wall clock.
     */
    fun toWallClock(ts: Long, nowWall: Long, nowElapsed: Long): Long {
        if (ts <= 0) return 0
        return if (abs(ts - nowWall) <= abs(ts - nowElapsed)) ts else nowWall - (nowElapsed - ts)
    }
}

object ContentHash {
    fun of(salt: String, kind: String, content: String): String {
        val md = MessageDigest.getInstance("SHA-256")
        md.update(salt.toByteArray())
        md.update(0)
        md.update(kind.toByteArray())
        md.update(0)
        md.update(content.toByteArray())
        return md.digest().joinToString("") { "%02x".format(it) }
    }
}

object Preview {
    private val WHITESPACE = Regex("\\s+")
    fun of(text: String, max: Int = 60): String {
        val flat = text.replace(WHITESPACE, " ").trim()
        return if (flat.length <= max) flat else flat.take(max) + "…"
    }
}
