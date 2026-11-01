package dev.cliptrail.spike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SignalMatcherTest {
    private val own = "dev.cliptrail.spike"
    private val m = SignalMatcher(listOf("Copy", "Cut", "Copy URL"), own, listOf("ClipTrail Spike"))

    private fun click(pkg: String, vararg texts: String) =
        A11yEvent(EventKind.CLICK, pkg, "android.widget.Button", texts.toList())

    private fun toast(pkg: String, text: String) =
        A11yEvent(EventKind.NOTIFICATION, pkg, "android.widget.Toast\$TN", listOf(text), isToast = true)

    @Test fun clickOnCopyInSelectionToolbar() {
        assertEquals(Signal.CLICK_COPY, m.match(click("com.android.chrome", "Copy")))
        assertEquals(Signal.CLICK_COPY, m.match(click("com.whatsapp", "Cut")))
        assertEquals(Signal.CLICK_COPY, m.match(click("com.android.chrome", "Copy link address")))
        assertEquals(Signal.CLICK_COPY, m.match(click("com.google.android.gm", "  COPY  ")))
        assertEquals(Signal.CLICK_COPY, m.match(click("x", "Copy…")))
    }

    @Test fun localizedPlatformLabelMatches() {
        val de = SignalMatcher(listOf("Kopieren", "Ausschneiden"), own)
        assertEquals(Signal.CLICK_COPY, de.match(click("com.android.chrome", "Kopieren")))
    }

    @Test fun contentDescriptionAloneMatches() {
        assertEquals(Signal.CLICK_COPY, m.match(click("com.sec.android.app.sbrowser", "", "Copy")))
    }

    @Test fun lookalikesDoNotMatch() {
        assertNull(m.match(click("x", "Copyright")))
        assertNull(m.match(click("x", "Paste")))
        assertNull(m.match(click("x", "Shortcut")))
        assertNull(m.match(click("x", "Copy " + "a".repeat(60))))
        assertNull(m.match(click("x")))
    }

    @Test fun ownPackageIgnored() {
        assertNull(m.match(click(own, "Copy")))
        assertNull(m.match(toast(own, "Copied to clipboard")))
    }

    @Test fun samsungCopiedToast() {
        assertEquals(Signal.TOAST_COPIED, m.match(toast("com.android.systemui", "Copied to clipboard.")))
        assertEquals(Signal.TOAST_COPIED, m.match(toast("org.mozilla.firefox", "Copied")))
        assertEquals(Signal.TOAST_COPIED, m.match(toast("com.whatsapp", "Message copied")))
    }

    @Test fun realNotificationIsNotAToastSignal() {
        val n = A11yEvent(EventKind.NOTIFICATION, "com.whatsapp", "android.app.Notification", listOf("Copied!"), isToast = false)
        assertNull(m.match(n))
    }

    @Test fun pasteAccessToastNeverTriggersButIsCounted() {
        val t = toast("com.android.systemui", "ClipTrail Spike pasted from your clipboard")
        assertNull(m.match(t))
        assertTrue(m.isAccessToast(t))
        assertFalse(m.isAccessToast(toast("com.android.systemui", "Copied to clipboard")))
    }

    @Test fun systemUiClipboardOverlay() {
        val overlay = A11yEvent(
            EventKind.WINDOW_STATE, "com.android.systemui",
            "com.android.systemui.clipboardoverlay.ClipboardOverlayView", emptyList(),
        )
        assertEquals(Signal.SYSUI_CLIPBOARD, m.match(overlay))
        assertEquals(Signal.SYSUI_CLIPBOARD, m.match(overlay.copy(kind = EventKind.CONTENT_CHANGED)))
        val shade = A11yEvent(EventKind.WINDOW_STATE, "com.android.systemui", "android.widget.FrameLayout", listOf("Notification shade"))
        assertNull(m.match(shade))
    }

    @Test fun clipboardPackageWindow() {
        val e = A11yEvent(EventKind.WINDOW_STATE, "com.samsung.android.clipboarduiservice", "android.app.Dialog", emptyList())
        assertEquals(Signal.CLIPBOARD_UI, m.match(e))
        assertNull(m.match(e.copy(kind = EventKind.CONTENT_CHANGED)))
    }

    @Test fun foregroundTracking() {
        assertTrue(m.tracksForeground(A11yEvent(EventKind.WINDOW_STATE, "com.android.chrome", "Activity", emptyList())))
        assertFalse(m.tracksForeground(A11yEvent(EventKind.WINDOW_STATE, "com.android.systemui", "x", emptyList())))
        assertFalse(m.tracksForeground(A11yEvent(EventKind.WINDOW_STATE, "com.samsung.android.honeyboard", "x", emptyList())))
        assertFalse(m.tracksForeground(A11yEvent(EventKind.WINDOW_STATE, own, "x", emptyList())))
        assertFalse(m.tracksForeground(click("com.android.chrome", "Copy")))
    }

    @Test fun diagnosticsAreNarrow() {
        assertTrue(m.isDiagnostic(toast("com.android.systemui", "anything")))
        assertTrue(m.isDiagnostic(click("x", "Copy email")))
        assertFalse(m.isDiagnostic(click("x", "Send")))
        assertFalse(m.isDiagnostic(A11yEvent(EventKind.CONTENT_CHANGED, "com.android.chrome", "android.widget.TextView", emptyList())))
    }

    @Test fun blocklist() {
        assertTrue(SignalMatcher.isBlockedPackage("com.x8bit.bitwarden"))
        assertFalse(SignalMatcher.isBlockedPackage("com.android.chrome"))
    }
}
