# ClipTrail for Android

The Android app is the **same Tauri project** as the desktop app (`apps/desktop`), built for
`aarch64-linux-android`. One Rust library (`cliptrail_desktop_lib`) holds the clip store, classifier,
transforms and the sync client; the React UI switches to a phone layout; a thin Kotlin layer
(`src-tauri/gen/android/app/src/main/java/dev/cliptrail/app/`) does what only Android can: capture.

Package: `dev.cliptrail.app` (set in `src-tauri/tauri.android.conf.json`). Sideloaded APK; not on Play.

## How it fits together

```
 Kotlin (Android)                                     Rust (same .so)
 ─────────────────                                    ───────────────
 ClipTrailApp.onCreate ── NativeCore.nativeInit ───▶  mobile::init → process-wide Core
 CaptureAccessibilityService ┐                         (pipeline + settings), FileKeyStore,
 SaveClipTileService ────────┤  ReadClipActivity      sync client started if paired
 KeepAliveService (action) ──┘  (focus → read clip)
 IntentCaptureActivity (Share / text menu)
 MainActivity (read on focus) ── NativeCore.ingest* ─▶ mobile::ingest → Pipeline::ingest
                                                       → sync notify_local_change
                                                       → emit clips-changed (if UI alive)
 MainActivity (TauriActivity) ── Tauri ────────────▶  mobile::run: UI commands
 ClipTrailPlugin (Tauri plugin) ◀── bridge::call ──── android_* commands, copy_clip, …
```

- **Capture without the UI.** The Kotlin services call JNI functions exported by the Rust library
  (`src-tauri/src/mobile/jni.rs`): `nativeInit(dataDir, keysDir, deviceName)`, `ingestText`,
  `ingestImage(png)`, `isPaused`. They share one process-wide `Core` with the Tauri commands, so there
  is one SQLite store, one dedup and one sync client, whether or not the window is open.
- **Sync** runs with the process, not the window: it starts when the core is initialized by whichever
  component Android started first. The phone is always a client of the desktop hub. Images sync as
  thumbnails; the full image downloads when you open the clip.
- **Data**: `filesDir` (clips.db, images/, thumbs/, settings.json, sync.db). Sync keys: a `0600` file
  under `noBackupFilesDir/keys/` (TODO: wrap with an Android Keystore key). Backups are disabled.
- **Privacy**: clip text never goes to logcat (tag `ClipTrail`: signal, verdict, kind, length only).
  `EXTRA_IS_SENSITIVE` clips are dropped, triggers from password managers never read the clipboard.
  `INTERNET` is used only for the LAN connection to your own paired computer. No telemetry.

## Build

Prerequisites (once): Android Studio's JBR, SDK platform 36, NDK, `rustup target add aarch64-linux-android`.

```powershell
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:NDK_HOME     = "$env:ANDROID_HOME\ndk\29.0.14206865"
$env:JAVA_HOME    = "C:\Program Files\Android\Android Studio\jbr"
cd apps\desktop
# On this machine host build scripts need MSVC: wrap with scratch\msvc.cmd
& ..\..\scratch\msvc.cmd pnpm tauri android build --debug --apk --target aarch64
# → src-tauri\gen\android\app\build\outputs\apk\universal\debug\app-universal-debug.apk
cd src-tauri\gen\android; .\gradlew.bat --stop   # free the RAM afterwards
```

Notes:
- 8 GB RAM: `gen/android/gradle.properties` keeps Gradle at 2 GB, no parallel workers, Kotlin in-process.
  Set `CARGO_BUILD_JOBS=2` if the machine thrashes. Don't run Gradle and a big cargo build at once.
- `targetSdk` stays **36**: Android 17 (targetSdk 37) blocks local-network access without the
  `ACCESS_LOCAL_NETWORK` runtime permission (declared already, not yet requested).
- Building for Android rewrites `src-tauri/gen/schemas/acl-manifests.json` with the mobile plugin set.
  The next desktop build rewrites it back; don't commit the mobile version.
- Browser preview of the phone UI: `pnpm dev`, open `http://localhost:1420/?platform=android` (or any
  window narrower than 640 px).

## Install

```powershell
adb install -r src-tauri\gen\android\app\build\outputs\apk\universal\debug\app-universal-debug.apk
```

`adb install` is a session install, which usually avoids Android's "restricted settings" block. Copying
the APK to the phone and opening it from My Files works too, but then step 2 below is needed.

## Set up on the Samsung (Galaxy S23 FE, One UI)

Open ClipTrail › Settings (gear, top right). The **Capture** section has a live checklist with buttons:

1. **Automatic capture (beta)** — on by default. Tap **Open**: Accessibility › Installed apps ›
   **ClipTrail automatic capture** › On › Allow.
2. **Switch greyed out / "Restricted setting"** (file-manager installs): try step 1 once, then tap
   **App info** › **⋮** (top right) › **Allow restricted settings** › confirm, and do step 1 again.
3. **Notifications** › Allow. You get a quiet "ClipTrail is saving your copies" notification with a
   **Save clipboard** button.
4. **Keep ClipTrail running**: **Allow** (unrestricted battery), then **Battery settings** ›
   Background usage limits › **Never sleeping apps** › + ClipTrail, and turn off **Put unused apps to
   sleep**.
5. Optional: Quick Settings › ✎ › drag **Save clipboard** into the active tiles.

Ways a copy gets saved: automatic (accessibility trigger + one-frame focus grab), the tile, the
notification button, Share › **Save to ClipTrail** (text and images), select text › ⋮ › **Save to
ClipTrail**, opening the app, and the in-app save button (bottom right). Android 12+ shows a "ClipTrail
pasted from your clipboard" toast when ClipTrail reads the clipboard; that's the OS, not an error.

## Pair with the laptop

1. Laptop: ClipTrail › Settings › Devices › **Sync with my devices** on › **Pair a phone**. A QR code
   and a `cliptrail://pair?...` link appear (valid 10 minutes, single use).
2. Phone: ClipTrail › Settings › **Your computer** › **Scan QR code** (or **Paste link**). Scanning the
   QR with the phone's own camera app also works: the link opens ClipTrail and pairs.
3. Compare the 4-digit **check code** on both screens. If they differ, unpair.
4. Both on the same Wi-Fi. If nothing connects: the Windows network must be **Private**, and Windows
   Firewall must allow ClipTrail on private networks (it asks the first time the hub starts). Guest
   Wi-Fi with client isolation blocks it.

Copies then flow both ways within a second or two while both are online; offline changes queue and
catch up on reconnect. **Unpair** (phone) tells the laptop and forgets the keys; clips stay.

Dev trick without Wi-Fi: `adb reverse tcp:47821 tcp:47821`, run the hub on the laptop with
`CLIPTRAIL_SYNC_BIND=127.0.0.1`, and pair with the link (it advertises 127.0.0.1).

## Known gaps

- Everything Android-specific was compiled and smoke-tested on one phone; the accessibility trigger's
  hit rate depends on the app and One UI version (see `apps/android-spike` for the measurement tool).
- Clipboard URIs that aren't images (files) aren't saved yet.
- Sync keys are a private file, not yet wrapped by the Android Keystore; the clip DB is plaintext
  (same as desktop during the prototype).
- The phone never hosts sync; with the laptop off, phone copies queue until it's back.
- Excluded-apps editing and per-app filters are desktop-only in the UI for now (password managers are
  still blocked on the phone).
- No release signing yet (debug APK only).
