package dev.cliptrail.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class ClipGuardTest {
    private val now = 1_800_000_000_000L
    private fun snap(ts: Long, hash: String?, sensitive: Boolean = false) = ClipSnapshot(ts, hash, sensitive)

    @Test fun freshNewClipIsAccepted() {
        val g = ClipGuard()
        assertEquals(Verdict.ACCEPTED, g.evaluate(snap(now - 300, "a"), now, requireFresh = true))
        assertEquals(now - 300, g.lastSeenTimestamp)
    }

    @Test fun staleClipRejectedForAutomaticTriggersOnly() {
        val g = ClipGuard(freshWindowMs = 5_000)
        assertEquals(Verdict.STALE, g.evaluate(snap(now - 6_000, "a"), now, requireFresh = true))
        // A stale verdict must not poison state: a manual read of the same clip still works.
        assertEquals(Verdict.ACCEPTED, g.evaluate(snap(now - 6_000, "a"), now, requireFresh = false))
    }

    @Test fun sameClipTwiceIsDuplicateSameClip() {
        val g = ClipGuard()
        g.evaluate(snap(now - 100, "a"), now, true)
        assertEquals(Verdict.DUPLICATE_SAME_CLIP, g.evaluate(snap(now - 100, "a"), now + 50, true))
    }

    @Test fun recopiedContentIsDuplicateContent() {
        val g = ClipGuard()
        g.evaluate(snap(now - 100, "a"), now, true)
        g.evaluate(snap(now, "b"), now, true)
        assertEquals(Verdict.DUPLICATE_CONTENT, g.evaluate(snap(now + 1_000, "a"), now + 1_000, true))
    }

    @Test fun sensitiveIsDroppedAndNotHashed() {
        val g = ClipGuard()
        assertEquals(Verdict.SENSITIVE, g.evaluate(snap(now, null, sensitive = true), now, true))
        assertEquals(Verdict.DUPLICATE_SAME_CLIP, g.evaluate(snap(now, null, sensitive = true), now, true))
    }

    @Test fun emptyAndNull() {
        val g = ClipGuard()
        assertEquals(Verdict.EMPTY, g.evaluate(null, now, true))
        assertEquals(Verdict.EMPTY, g.evaluate(snap(now, null), now, true))
    }

    @Test fun unknownTimestampCountsAsFreshAndSkipsSameClipCheck() {
        val g = ClipGuard()
        assertEquals(Verdict.ACCEPTED, g.evaluate(snap(0, "shared"), now, true))
        assertEquals(Verdict.DUPLICATE_CONTENT, g.evaluate(snap(0, "shared"), now, true))
    }

    @Test fun futureTimestampIsFresh() {
        val g = ClipGuard()
        assertEquals(Verdict.ACCEPTED, g.evaluate(snap(now + 2_000, "a"), now, true))
    }

    @Test fun capacityEvictsOldest() {
        val g = ClipGuard(capacity = 2)
        g.evaluate(snap(1, "a"), 1, false)
        g.evaluate(snap(2, "b"), 2, false)
        g.evaluate(snap(3, "c"), 3, false)
        assertEquals(Verdict.ACCEPTED, g.evaluate(snap(4, "a"), 4, false))
        assertEquals(Verdict.DUPLICATE_CONTENT, g.evaluate(snap(5, "c"), 5, false))
    }

    @Test fun restoreAfterRestart() {
        val g = ClipGuard()
        g.restore(listOf("a", "b"), lastTimestamp = now - 10)
        assertEquals(Verdict.DUPLICATE_SAME_CLIP, g.evaluate(snap(now - 10, "b"), now, true))
        assertEquals(Verdict.DUPLICATE_CONTENT, g.evaluate(snap(now, "a"), now, true))
    }

    @Test fun clipTimeHandlesBothTimeBases() {
        val wall = now
        val elapsed = 90_000_000L // phone up ~25 h
        assertEquals(wall - 500, ClipTime.toWallClock(wall - 500, wall, elapsed))
        assertEquals(wall - 500, ClipTime.toWallClock(elapsed - 500, wall, elapsed))
        assertEquals(0, ClipTime.toWallClock(0, wall, elapsed))
    }

    @Test fun hashIsSaltedAndKindAware() {
        assertEquals(ContentHash.of("s", "text", "hi"), ContentHash.of("s", "text", "hi"))
        assertNotEquals(ContentHash.of("s", "text", "hi"), ContentHash.of("t", "text", "hi"))
        assertNotEquals(ContentHash.of("s", "text", "hi"), ContentHash.of("s", "uri", "hi"))
    }

    @Test fun previewFlattensAndTruncates() {
        assertEquals("a b c", Preview.of("  a\n b\t\tc "))
        assertEquals("x".repeat(60) + "…", Preview.of("x".repeat(100)))
    }
}
