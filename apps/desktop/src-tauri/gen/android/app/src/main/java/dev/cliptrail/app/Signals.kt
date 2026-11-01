package dev.cliptrail.app

import java.util.Locale

/**
 * Pure (no Android imports) signal matching, so it can be unit-tested on the JVM.
 *
 * A "signal" is an accessibility event that suggests the user just copied something.
 * Signals are only triggers: the clip itself is read afterwards (with focus) and checked
 * by [ClipGuard], so a false positive costs one focus flash, not a bad record.
 */
enum class Signal(val label: String, val automatic: Boolean) {
    CLICK_COPY("Tap on a Copy/Cut button", true),
    TOAST_COPIED("\"Copied\" toast", true),
    SYSUI_CLIPBOARD("SystemUI clipboard overlay", true),
    CLIPBOARD_UI("Clipboard panel window", true),
    APP_FOREGROUND("Opened ClipTrail", false),
    QS_TILE("Quick Settings tile", false),
    NOTIFICATION("Notification \"Save clipboard\"", false),
    SHARE("Share to ClipTrail", false),
    PROCESS_TEXT("Text menu \"ClipTrail\"", false),
    IN_APP("Save clipboard in the app", false),
    SELF_TEST("Copy-test button", false),
}

enum class EventKind { CLICK, WINDOW_STATE, NOTIFICATION, CONTENT_CHANGED, OTHER }

/** The few fields of an AccessibilityEvent we look at. */
data class A11yEvent(
    val kind: EventKind,
    val packageName: String,
    val className: String,
    val texts: List<String>,
    /** TYPE_NOTIFICATION_STATE_CHANGED whose parcelable is not a Notification, i.e. a toast. */
    val isToast: Boolean = false,
)

class SignalMatcher(
    /** Localized framework strings: android.R.string.copy, cut, copyUrl. */
    platformCopyLabels: Collection<String>,
    private val ownPackage: String,
    /** Our app label(s), to recognise the "<app> pasted from your clipboard" toast. */
    ownLabels: Collection<String> = emptyList(),
) {
    private val copyLabels: Set<String> =
        (platformCopyLabels.map(::normalize) + DEFAULT_COPY_LABELS).filter { it.isNotEmpty() }.toSet()
    private val ownLabels: List<String> = ownLabels.map(::normalize).filter { it.isNotEmpty() }

    fun match(e: A11yEvent): Signal? {
        if (e.packageName == ownPackage) return null
        return when (e.kind) {
            EventKind.CLICK -> if (e.texts.any(::isCopyLabel)) Signal.CLICK_COPY else null
            EventKind.NOTIFICATION ->
                if (e.isToast && e.texts.any(::isCopiedMessage)) Signal.TOAST_COPIED else null
            EventKind.WINDOW_STATE, EventKind.CONTENT_CHANGED -> when {
                e.packageName == SYSTEM_UI &&
                    (e.className.contains("clipboard", ignoreCase = true) || e.texts.any(::isCopiedMessage)) ->
                    Signal.SYSUI_CLIPBOARD
                e.kind == EventKind.WINDOW_STATE && isClipboardUiPackage(e.packageName) -> Signal.CLIPBOARD_UI
                else -> null
            }
            EventKind.OTHER -> null
        }
    }

    /** Android 12+ "<app> pasted from your clipboard" toast: a side effect we count, never a trigger. */
    fun isAccessToast(e: A11yEvent): Boolean {
        if (e.kind != EventKind.NOTIFICATION || !e.isToast) return false
        return e.texts.any { t ->
            val n = normalize(t)
            PASTE_WORDS.any { n.contains(it) } &&
                (n.contains("clipboard") || ownLabels.any { n.contains(it) })
        }
    }

    /** Window-state events from real apps tell us which app is in front (source of toast-triggered copies). */
    fun tracksForeground(e: A11yEvent): Boolean = e.kind == EventKind.WINDOW_STATE && isAppPackage(e.packageName)

    fun isAppPackage(pkg: String): Boolean =
        pkg.isNotEmpty() && pkg != ownPackage && pkg !in NON_APP_PACKAGES &&
            !isKeyboardPackage(pkg) && !isClipboardUiPackage(pkg)

    /** Events worth remembering in the on-device "event signatures" table (to learn what this phone emits). */
    fun isDiagnostic(e: A11yEvent): Boolean {
        if (e.packageName == ownPackage) return false
        return when (e.kind) {
            EventKind.NOTIFICATION -> true
            EventKind.WINDOW_STATE ->
                e.packageName in NON_APP_PACKAGES || isClipboardUiPackage(e.packageName) ||
                    isKeyboardPackage(e.packageName) || e.className.contains("clip", ignoreCase = true)
            EventKind.CONTENT_CHANGED ->
                e.className.contains("clip", ignoreCase = true) || isClipboardUiPackage(e.packageName)
            EventKind.CLICK -> e.texts.any { t ->
                val n = normalize(t)
                n.contains("cop") || n.contains("clip") || n == "cut"
            }
            EventKind.OTHER -> false
        }
    }

    fun isCopyLabel(text: String): Boolean {
        val n = normalize(text)
        if (n.isEmpty()) return false
        return n in copyLabels || (n.startsWith("copy ") && n.length <= MAX_COPY_LABEL_LEN)
    }

    companion object {
        const val SYSTEM_UI = "com.android.systemui"
        private const val MAX_COPY_LABEL_LEN = 40

        val NON_APP_PACKAGES = setOf(
            SYSTEM_UI,
            "android",
            "com.samsung.android.app.cocktailbarservice", // Edge panels
        )

        /** English extras on top of the platform strings; apps often use their own labels. */
        val DEFAULT_COPY_LABELS = setOf(
            "copy", "cut", "copy link", "copy url", "copy link address", "copy text",
            "copy to clipboard", "copy all", "copy code", "copy number", "copy email", "copy address",
        )

        private val COPIED_WORDS = listOf(
            "copied", "clipboard", "kopiert", "copiado", "copié", "copiato", "gekopieerd",
            "skopiowano", "скопирован", "コピーしました", "已复制", "已複製", "복사", "कॉपी",
        )
        private val PASTE_WORDS = listOf(
            "paste", "eingefügt", "pegado", "collé", "incollato", "вставлен", "貼り付け", "粘贴", "붙여넣",
        )

        /** Password managers and the like: never read the clipboard after a trigger from these. */
        private val BLOCKED_PACKAGES = setOf(
            "com.x8bit.bitwarden", "com.bitwarden.authenticator", "com.agilebits.onepassword",
            "com.onepassword.android", "keepass2android.keepass2android", "keepass2android.keepass2android_nonet",
            "com.kunzisoft.keepass.free", "com.kunzisoft.keepass.libre", "com.kunzisoft.keepass.pro",
            "com.lastpass.lpandroid", "com.dashlane", "proton.android.pass", "com.samsung.android.samsungpass",
        )

        fun isBlockedPackage(pkg: String): Boolean = pkg in BLOCKED_PACKAGES

        fun isKeyboardPackage(pkg: String): Boolean =
            pkg.contains("inputmethod") || pkg.contains("honeyboard") || pkg.contains("keyboard") ||
                pkg == "com.touchtype.swiftkey"

        fun isClipboardUiPackage(pkg: String): Boolean = pkg.contains("clipboard", ignoreCase = true)

        fun isCopiedMessage(text: String): Boolean {
            val n = normalize(text)
            if (n.isEmpty() || PASTE_WORDS.any { n.contains(it) }) return false
            return COPIED_WORDS.any { n.contains(it) }
        }

        private val WHITESPACE = Regex("\\s+")

        fun normalize(s: String): String =
            s.lowercase(Locale.ROOT)
                .replace("&", "")
                .replace(' ', ' ')
                .replace(WHITESPACE, " ")
                .trim()
                .trimEnd('.', '…', ':', '!')
                .trim()
    }
}
