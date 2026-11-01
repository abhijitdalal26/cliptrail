# ClipTrail Android capture spike (M1)

A tiny, throwaway, pure-Kotlin app (no Rust, no Tauri, no androidx) that answers one question on a real
phone: **can we reliably capture copies made in other apps without our own keyboard?** It measures
itself, so you can just use the phone for a while and then read the counters. Plan:
[`docs/android-plan.md`](../../docs/android-plan.md), milestone M1.

Nothing leaves the phone: the app has **no INTERNET permission**, backups are disabled, and clip previews
in the local log are AES-GCM encrypted with an Android Keystore key. Clip text never goes to logcat (only
kind/length).

## What it does

| Path | How | Kind |
|---|---|---|
| Accessibility trigger | `TYPE_VIEW_CLICKED` on a control labelled Copy / Cut / Copy URL (the phone's own localized framework strings, plus "Copy link", "Copy link address", …) | automatic |
| | "Copied" toast (`TYPE_NOTIFICATION_STATE_CHANGED` that is a toast, text contains "copied"/"clipboard", never "paste") | automatic |
| | SystemUI clipboard overlay (Android 13+; window/content event from `com.android.systemui` with a clipboard class) | automatic |
| | A window from a package with "clipboard" in its name (e.g. Samsung clipboard UI) | automatic |
| Focus grab | on a trigger: wait 120 ms, add a 1×1 `TYPE_ACCESSIBILITY_OVERLAY` window that takes input focus for one frame, read `primaryClip`, remove it. If that fails, fall back to an invisible activity | — |
| App opened | reads the clipboard once each time the spike's screen comes to the front | manual |
| Quick Settings tile | "Save clipboard" → invisible activity reads the clipboard | manual |
| Notification action | "Save clipboard" on the persistent "ClipTrail is capturing" notification | manual |
| Share target | Share → ClipTrail Spike (text, images; for images only the URI is logged in this spike) | manual |
| Text-selection menu | select text → ⋮ → "ClipTrail" (`ACTION_PROCESS_TEXT`, no clipboard involved) | manual |

Guards: an automatic trigger only counts if the clip's `ClipDescription` timestamp is under 5 s old
(a stale Copy click is retried once after 450 ms, for apps that copy asynchronously). Duplicates are
caught by timestamp and by a salted content hash. Clips flagged `EXTRA_IS_SENSITIVE` are dropped (and
counted). Triggers from password managers (Bitwarden, 1Password, KeePass*, LastPass, Dashlane, Proton
Pass, Samsung Pass) are never read. Non-click triggers right after a grab are ignored for 1.5 s, so our
own read (and its "pasted from your clipboard" toast) can't re-trigger us.

**Miss detection:** when a manual path (app opened, tile, notification) finds a *new* clip, the automatic
layer must have missed that copy. That's what the **automatic capture rate** on the main screen is based on:
`auto captures / (auto captures + misses)`.

## Build

From PowerShell (JDK = Android Studio's JBR, `ANDROID_HOME` set):

```powershell
cd apps\android-spike
.\gradlew.bat assembleDebug        # → app\build\outputs\apk\debug\app-debug.apk
.\gradlew.bat testDebugUnitTest    # signal matching + staleness/dedup guard
.\gradlew.bat lint
.\gradlew.bat --stop               # free the RAM afterwards
```

## Install

With USB debugging on and the phone plugged in:

```powershell
adb install -r app\build\outputs\apk\debug\app-debug.apk
```

Or copy `app-debug.apk` to the phone (USB, Quick Share, Drive…) and open it with My Files; allow
"Install unknown apps" for that file manager when asked. Note: a file-manager install is the case where
Android 13+ **restricted settings** blocks the accessibility toggle (step 2 below); `adb install` usually
avoids it.

## What to enable on the Samsung (S23 FE, One UI)

The main screen shows a live checklist with buttons for each of these.

1. **Accessibility service:** Settings → Accessibility → Installed apps → **ClipTrail Spike** → On → Allow.
2. **Restricted settings** (only if the switch is greyed out or you get a "Restricted setting" dialog):
   try to enable the service once, then Settings → Apps → ClipTrail Spike (or the **Open App info** button)
   → **⋮** (top right) → **Allow restricted settings** → confirm with your PIN/fingerprint → go back to step 1.
3. **Notifications:** allow when the app asks (or tap **Allow notifications**). You should see a quiet
   "ClipTrail is capturing" notification with a **Save clipboard** button.
4. **Battery:**
   - tap **Allow unrestricted** (system dialog "Let app always run in background?" → Allow);
   - Settings → Battery → **Background usage limits** → **Never sleeping apps** → **+** → ClipTrail Spike;
   - in the same screen, turn off **Put unused apps to sleep**.
5. **Quick Settings tile:** swipe down twice → ✎ (edit) → drag **Save clipboard** into the active tiles.
6. *(Optional, only if counters show many "focus not gained")* Settings → Apps → ClipTrail Spike →
   **Appear on top** → On. This only helps the invisible-activity fallback.

Then open the app once: the checklist should be all ✅ (the "Appear on top" and restricted-settings rows are
informational).

## Test protocol (go / no-go)

1. Open the spike, tap **Reset all**, then **Copy test now**: within ~2 s the log shows a `SELF_TEST`
   attempt marked ✅ (or ↺ if an automatic signal got it first). That proves reading works.
2. Tap **Copy test in 5 s** and immediately switch to another app. When you come back, look for an
   automatic attempt (`TOAST_COPIED` / `SYSUI_CLIPBOARD`). If there's none, programmatic copies on this phone
   emit no signal (expected on One UI); that's fine, just note it.
3. Make **30 deliberate, different copies** (any mix, about 5 per app), each by long-press → Copy, or
   the app's own Copy button/menu:
   - **Chrome** (page text, "Copy link address" from a long-pressed link, URL bar → copy)
   - **Samsung Internet** (same three)
   - **WhatsApp** (long-press a message → copy icon)
   - **Gmail** (text in a mail, copy an address)
   - **Samsung Notes** (select text → Copy)
   - **Settings** (e.g. About phone → long-press a value → Copy; or search-field text)
   After each copy, glance at the screen: note any of the side effects below.
4. Open the spike. It does one "app opened" read (that's how a missed *last* copy gets counted). For
   misses in the *middle* of the run, tap the **Save clipboard** tile or notification button right after any
   copy where you suspect a miss (or just do it after every copy, to be strict).
5. Read **3. Counters**:
   - **AUTOMATIC CAPTURE RATE**, "won N captures" per trigger type (shows which signals actually work on
     this phone), failures by reason, latency.
   - **4. Last 50 attempts**: correct text and source app per copy.
   - **5. Event signatures**: which toast/overlay/window classes this phone emits (that's the
     "written list of event signatures" M1 asks for).
6. Tap **Share report** and send it to yourself (it has counters, event signatures and attempt metadata but
   **no clip text**) so it can go into `docs/android-plan.md`.

**Go** = at least **27/30** copies captured automatically (≥ 90%), with correct text and package, saved within
about 1 s, and side effects you can live with. **No-go** = under about 21/30 (70%): the plan then falls back to a
UX built around the tile, notification, share target and text menu first (plus the optional READ_LOGS mode).
In between: look at which apps/signals missed and decide.

Optional, with USB connected: `adb logcat -s ClipTrailSpike ClipboardService` shows triggers, verdicts and the
system's own clipboard-denial lines (no clip text from us).

## Side effects to watch for

Write these down during the 30-copy run; they're half of the go/no-go:

- **Keyboard dismissal:** copying from a text field (WhatsApp compose box, Notes) and the keyboard drops
  or flickers when the focus grab happens. (We set `FLAG_ALT_FOCUSABLE_IM` + `SOFT_INPUT_STATE_UNCHANGED`
  to try to prevent it.)
- **Lost selection:** selection handles / the floating Copy-Paste toolbar disappear right after Copy,
  sooner than usual, or the text cursor jumps.
- **"pasted from your clipboard" toast (Android 12+):** a system toast like "ClipTrail Spike pasted from
  your clipboard" on every capture. The counters show how many the service saw. Check if One UI has a toggle
  (Settings → Security and privacy → More privacy settings → **Clipboard access alerts** / "Show clipboard
  access") and whether turning it off hides it.
- **Video/audio hiccups:** a playing video pausing or stuttering when you copy (e.g. copy a YouTube comment).
- **Brief flash / touch swallowed:** only expected when the invisible-activity fallback runs (counter
  "activity fallback: launched N").
- **Battery / heat:** anything noticeable over a day with the service on.
- **Service silently off:** after a day or a reboot, is the checklist still ✅ and does capture still work?

## Not covered by this spike

Rust core, encryption-at-rest of the real store, sync, image bytes (only URIs are logged), boot receiver. All
of that is M2+ in the plan. This project is meant to be thrown away once the numbers are in.
