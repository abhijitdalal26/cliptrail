package dev.cliptrail.app

/**
 * How a clip reached ClipTrail on this phone. Every path is one the user starts: Android 10+ lets only
 * the focused app read the clipboard, and we don't use an accessibility service to watch for copies
 * (sideloaded apps that declare one are blocked at install by Play Protect in some regions).
 */
enum class Signal(val label: String) {
    APP_FOREGROUND("Opened ClipTrail"),
    QS_TILE("Quick Settings tile"),
    NOTIFICATION("Notification \"Save clipboard\""),
    SHARE("Share to ClipTrail"),
    PROCESS_TEXT("Text menu \"ClipTrail\""),
    IN_APP("Save clipboard in the app"),
    SHORTCUT("App icon shortcut \"Save clipboard\""),
    WIDGET("Home screen widget"),
}
