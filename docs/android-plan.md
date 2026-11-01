# ClipTrail Android v1 — research and plan

Status: plan only, nothing built. Written 2026-11-01. Target: the owner's own phone + Windows laptop, sideloaded APK, no custom keyboard. Read `CLAUDE.md` "Current decisions" first; this doc follows them.

Legend: **[V]** verified against a cited source or a file in this repo/machine; **[R]** reported by third-party projects or from memory of AOSP, must be confirmed on the real phone in the spike (M1); **[?]** could not verify.

## 0. Open questions for the owner

1. Phone model, Android version, OEM skin (Samsung One UI / Xiaomi HyperOS / Pixel ...). This decides which capture signals exist (section 2.3) and which battery killers we fight. No phone is attached to this laptop right now (`adb devices` empty). **[V]**
2. Is the phone arm64? (Almost certainly. Plan assumes `aarch64-linux-android` only.)

## 1. Architecture decision

### 1.1 What the references actually do

- **decentpaste** (Apache-2.0, `refs/decentpaste--decentpaste`) is a Tauri 2 app that ships on Android. Its Android-specific code is small: `gen/android` (generated project, checked in; `MainActivity : TauriActivity`), plus a Kotlin Tauri plugin `tauri-plugin-decentshare` that registers an `<activity-alias>` with a SEND intent filter, stores the shared text in a `@Volatile` field, and exposes `getPendingShare` as a `@Command` the JS side polls on startup (cold-start intent arrives before the WebView is ready, so it must be stored and pulled). **[V]** It has **no** background capture, no foreground service, no Accessibility service. Its own ARCHITECTURE.md says sync only works while the app is foregrounded. It proves: Tauri 2 Android builds in CI, Rust core + React-style UI works, Kotlin plugins are straightforward. It does not prove anything about our background-capture needs.
- **Tauri 2 mobile plugins**: a Kotlin class extending `app.tauri.plugin.Plugin`, constructed with the `Activity`; `@Command` methods callable from Rust/JS; `trigger(event, payload)` pushes events to JS; commands run on the main thread, so push blocking work to `Dispatchers.IO`. **[V]** Plugins live and die with the Activity, so a plugin cannot be the entry point for an Accessibility Service or Quick Settings tile (the OS starts those, not the WebView).
- **Tauri docs PR on JNI** shows the escape hatch we need: Kotlin does `System.loadLibrary("<your cdylib>")` in an `init` block, declares `external fun`, and Rust exports `Java_<pkg>_<Class>_<method>` (`#[no_mangle] pub extern "system" fn`, `jni` crate). The docs explicitly say this lets native code run "even when the application WebView is suspended". **[V]** (tauri-docs PR #3575.)

### 1.2 Options

| | (a) Tauri 2 Android + shared React UI + native Kotlin capture layer | (b) Native Kotlin + Compose + UniFFI | (c) Hybrid: Compose shell hosting React in a WebView + UniFFI |
|---|---|---|---|
| UI reuse | Desktop React/Tailwind reused (responsive pass needed) | Rewrite whole UI in Compose; two UIs to keep "beautiful" | Reuse React but hand-roll the JS/Rust bridge Tauri already gives |
| Capture layer | Kotlin service/tile/activities + JNI into the same `.so` | Same Kotlin; calls Rust via UniFFI | Same |
| Rust build | Tauri CLI drives NDK + Gradle | `cargo-ndk` + UniFFI bindgen + Gradle by hand | same as (b) |
| New infra to learn | Tauri Android, `jni` crate | UniFFI, Compose, Hilt/nav etc. | all of the above |
| Native feel | WebView (Chromium); fine for a list UI | Best | WebView |
| Risk | Tauri-on-Android rough edges (insets, IME, dev loop on Windows) | Doubled UI work; the desktop design system doesn't carry over | Maintaining a custom bridge forever |

### 1.3 Recommendation: (a), with the capture layer written so it does not depend on Tauri

Reasons:
1. Biggest cost for a one-person project is a second UI. The user wants a beautiful UI; (a) gets the desktop design system for free, (b) forces a parallel one.
2. All hard Android work (Accessibility, overlay focus trick, tile, share target, FGS) is plain Kotlin in every option, so (b) buys nothing there.
3. Sync client, crypto and store are Rust in every option; in (a) they are already in the process.
4. Escape hatch: because capture talks to Rust through a tiny JNI surface (below), we can swap the UI for Compose later without touching capture or the core.

**How captured content reaches the Rust store when the WebView activity is closed** (the key question). Choose the first; others are rejected:

- **Chosen: JNI straight into the same cdylib, no Tauri needed.** The Android process is the same one regardless of which component the OS started. The Accessibility Service (or tile/share activity) calls `System.loadLibrary("<tauri lib name>")` (a no-op if the activity already loaded it) and then `external fun ingestText(...)` etc. On the Rust side a process-wide `static CORE: OnceLock<Pipeline>` (opened lazily from `filesDir`, passed in by Kotlin as `initCore(dataDir)`) is shared by the JNI functions and by the Tauri commands, so there is exactly one SQLite connection pool and one dedup state. After an ingest, Rust emits a Tauri event `clips-changed` **only if** a global `AppHandle` has been registered (UI alive); otherwise the UI just re-queries on next open. The Tauri runtime itself is started by the Activity's `mobile_entry_point`; our JNI functions never touch it. **[R]** the loading pattern is documented (above); that a service-only process runs fine without the Tauri runtime is standard JNI behavior but must be proven in M2.
- Rejected: Kotlin-side queue (SharedPreferences/file) drained by Rust on launch. Simpler, but history would be invisible to sync until the UI opens, and it forks dedup/classify logic. Keep it only as an emergency fallback if JNI misbehaves.
- Rejected: separate "headless Rust process". Two processes sharing SQLite = lock and cache problems for no gain.

Required core changes (small, additive, to be done deliberately because `model.rs` is the shared contract):
- Data dir comes from Kotlin (`dirs` does not work on Android). Keep `CLIPTRAIL_DATA_DIR`-style override.
- Images: Kotlin decodes the clipboard image URI and re-encodes to PNG (`Bitmap.compress`), hands bytes to Rust. Add `RawContent::ImageBytes { png: Vec<u8> }` (or accept encoded bytes in `Image`) so the `image` crate stays PNG-only. `source_app` = Android package name (Kotlin maps to labels for display).
- SQLCipher on Android needs OpenSSL for NDK targets; this is a known build pain. Decide early (M2) whether Android uses SQLCipher or the "XChaCha20 on blobs" fallback. **[?]** not tried.

**Risks of (a)**: Tauri Android dev loop on Windows (symlink/Developer Mode requirements are commonly reported but the Tauri prerequisites page does not mention them **[?]**); WebView-specific UI bugs (keyboard insets, edge-to-edge; decentpaste calls `enableEdgeToEdge()` in `MainActivity`); two FFI-ish layers (Tauri IPC for UI, JNI for capture) in one `.so`; a Tauri 2 Android port elsewhere reported a launch crash from linking `c++_shared`, so avoid C++ deps in the Android build **[R]** (git.manko.yoga postmortem, one team's experience).

## 2. Capture on Android 10 to 16

### 2.1 The rule we are working around
Since Android 10 the clipboard is readable only by the default IME or the app that currently has input focus. **[V]** (developer.android.com Android 10 privacy changes.) Background reads return nothing and the system logs a denial. Android 12+ shows a toast whenever an app reads clip data it did not write **[V]** (secure-clipboard-handling page). A privileged `READ_CLIPBOARD_IN_BACKGROUND` permission exists but is not for third-party apps **[V]** (AOSP CTS). Android 16's behavior-change page has no clipboard changes. **[V]** I found no Android 16/17 clipboard change at all, but Android 17 (targetSdk 37) adds mandatory local-network permission, see 2.9 and 4.8.

### 2.2 Can an AccessibilityService read the clipboard itself?
No, not as a general rule. An accessibility service is not the focused app and not the IME, and docs list only those two exceptions. **[V]** Practitioners (Clipboard Actions, Seam, the acopy client) all use accessibility only as a **trigger** and then obtain focus to read. **[R]** Background activity-launch exemptions also do not list accessibility services **[V]** (background-starts doc), so launching a transparent Activity from the service needs `SYSTEM_ALERT_WINDOW` or another exemption; an accessibility overlay window avoids this (2.4).

### 2.3 Detection signals (trigger layer)
Service config: listen to `typeViewClicked | typeWindowStateChanged | typeNotificationStateChanged`, `canRetrieveWindowContent=false` (we do not need to read the screen; smaller privacy footprint), no package filter. Event `packageName` gives a free **source_app**.

| Signal | How | Reliability |
|---|---|---|
| Click on a control labelled "Copy" / "Cut" / "Copy link" etc. | `TYPE_VIEW_CLICKED`, compare `event.text`/`contentDescription` to the platform's own localized strings (`android.R.string.copy`, `cut`, plus a small list for "Copy link"/"Copy URL") so it works in any language | Good for the text-selection toolbar, share sheet, many menus. Misses apps that copy via custom UI or keyboard shortcut |
| System toast "Copied." (Samsung) | `TYPE_NOTIFICATION_STATE_CHANGED`, package `com.android.systemui`, class `Toast` | Seam's main Samsung signal; catches Firefox's Copy which sends no click. Samsung-specific **[R]** |
| Android 13+ clipboard preview overlay | window event from SystemUI with a clipboard-related class name | Seam matches it by class name but its author **did not verify on a Pixel** **[R]**; we must read the exact class on the owner's phone |
| `TYPE_VIEW_TEXT_SELECTION_CHANGED` | selection moved | **Weak.** Means "user selected text", not "copied". Use only as an optional "armed" hint. I found no source confirming it as a copy signal **[?]** |
| Hardware Ctrl+C | `onKeyEvent` with key filtering | Skip in v1 |

Guard against false positives and replays: after any trigger, read the clip and accept it only if `ClipDescription.getTimestamp()` is within about 5 s and differs from the last accepted timestamp (Seam uses a 5 s window for the same reason **[R]**). Heuristic misses are unavoidable, hence the manual paths in section 2.6.

### 2.4 Getting focus to read: two variants
1. **Accessibility overlay (preferred).** From the service, add a tiny view to `WindowManager` with `TYPE_ACCESSIBILITY_OVERLAY`, initially not focusable, then clear `FLAG_NOT_FOCUSABLE`, wait for window focus (or one layout pass), read `primaryClip`, remove the view. No `SYSTEM_ALERT_WINDOW` needed for accessibility overlay windows; no activity launch, so no background-launch rules. Seam's PR describes exactly this "one-frame focus grab" and reports it working on a Galaxy S25 Ultra only. **[R]** Seam is AGPL: idea only, none of its code was read or is to be copied; this paragraph is my own description.
2. **Transparent activity fallback.** Launch an invisible, no-history, excluded-from-recents activity that reads the clip in `onWindowFocusChanged(true)` and finishes. From the background it needs `SYSTEM_ALERT_WINDOW` (a listed exemption **[V]**); the same activity is what the Quick Settings tile and notification action launch (those are user-initiated, so allowed).

Side effects to measure in M1 (all **[?]** until tested): the app underneath loses window focus for ~50-150 ms, which may dismiss the soft keyboard, collapse the text-selection handles/toolbar, or briefly pause video; mitigation is `FLAG_ALT_FOCUSABLE_IM` so the IME stays attached to the underlying window. Plus the Android 12+ "ClipTrail pasted from your clipboard" toast on every capture; check whether Settings has a "Show clipboard access" toggle on the phone **[?]**.

### 2.5 What ClipCascade actually does (GPL-3.0; described in my own words, no code reused)
Files read: `ClipCascade_Mobile/src/android/app/src/main/{AndroidManifest.xml, java/com/clipcascade/ClipboardListenerModule.kt, ClipboardFloatingActivity.kt, BootReceiver.kt}`. **[V]**
- **It does not use an AccessibilityService at all** (no service in the manifest, no accessibility code in the Android tree). The task brief assumed it did.
- It registers a normal `OnPrimaryClipChangedListener` (fires reliably while the app is foreground) and, if `READ_LOGS` was granted via `adb shell pm grant`, starts a background thread running `logcat` filtered to `ClipboardService:E`. Any line mentioning its own application id means the OS just refused it a background read, i.e. "the clipboard changed". That line is the trigger. I believe this is the system's "denying clipboard access ... not in focus" error, emitted when the service tries to notify a listener that lacks access (AOSP memory **[R]**; verify in M1 with `adb logcat -s ClipboardService`).
- On trigger it launches a transparent activity (new task, clear task, excluded from recents). That activity adds a 1-pixel `TYPE_APPLICATION_OVERLAY` view (needs "draw over other apps", granted by `adb shell appops set ... SYSTEM_ALERT_WINDOW allow`), flips the view to focusable so the app owns window focus, reads `primaryClip` on the next layout pass, flips it back, removes it, `finish()`es. A 1 s debounce prevents storms.
- Content types: text if mime `text/*`; image if `image/*` with a URI (the URI string is passed on, not bytes); any other URI as "files". Only item 0 is read.
- Keep-alive: foreground service via the notifee library (type `remoteMessaging`), `BootReceiver` for `BOOT_COMPLETED`, optional 15-minute periodic health check, `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`. README says the READ_LOGS consent popup can be dismissed by an auto-start after reboot, which breaks monitoring, so it tells ADB users to disable start-on-boot.
- Takeaway for us: the focus-grab recipe is the same; the **trigger** differs. It needs a USB/ADB step and a log-consent popup; accessibility needs a Settings toggle and no computer.

### 2.6 Guaranteed manual paths (always built)
1. **Quick Settings tile** "Save clipboard": `TileService` click launches the transparent read activity (Android 14+: use the `PendingIntent` overload of `startActivityAndCollapse` **[R]**). Clipboard Actions calls this "the most universal option". **[R]**
2. **Persistent notification action** "Save clipboard" (a notification-tap PendingIntent is a listed background-launch exemption **[V]**). Free second trigger from the FGS notification.
3. **Share target**: `ACTION_SEND` `text/plain` and `image/*` (decentpaste's activity-alias pattern, but with a real Activity so we get the intent on cold start).
4. **`ACTION_PROCESS_TEXT`** ("Save to ClipTrail" in the text-selection toolbar overflow of most apps): captures the selection without touching the clipboard at all. ClipCascade also registers it.
5. **On app foreground**: when our UI comes to the front, read the clipboard once (we are focused, so this is allowed) and dedupe by hash.

### 2.7 Sensitive content and exclusions (must-do)
- Honor `ClipDescription.EXTRA_IS_SENSITIVE` (string key `android.content.extra.IS_SENSITIVE` on older SDKs) in `description.extras`. **[V]** (secure-clipboard-handling page; added API 33, the page does not state the level **[?]**.) Default behavior: **drop the clip, do not store**, matching "password managers excluded by default". Offer a setting to store as masked `Kind::Sensitive`.
- Package blocklist from the event `packageName` (Bitwarden, 1Password, KeePass variants, banking apps, ClipTrail itself) — editable.
- Never log clip text; log sizes/kinds only (AGENTS.md rule).

### 2.8 Foreground service type (Android 14+)
Every FGS must declare a type and its `FOREGROUND_SERVICE_*` permission. **[V]** Options:
- `dataSync`: apps targeting Android 15+ get 6 h per 24 h, reset when the app is foregrounded, and cannot start it from `BOOT_COMPLETED`. **[V]** Wrong fit for an always-on service.
- `specialUse`: no prerequisite, no timeout listed; requires a `<property android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE">` free-text explanation; Play reviews it, but we are sideloading. **[V]**
- `connectedDevice`: no timeout, but needs one of a list of prerequisite permissions (e.g. `CHANGE_NETWORK_STATE`); semantically "talks to a paired device" which fits, but stretches the definition. **[V]** prerequisites only.
- Choice: **`specialUse`** (subtype text: "keeps a local-network connection to the user's paired desktop for clipboard history sync"); fall back to `connectedDevice` if a ROM misbehaves. Start it from the UI, from `BOOT_COMPLETED`, and from the accessibility service's `onServiceConnected` (background-start rules for FGS are **[?]**, test in M6).
- Note: an enabled AccessibilityService is itself kept alive/bound by the system, so capture does not strictly need the FGS; the FGS exists for the sync socket, the notification action, and OEM-killer resistance.

### 2.9 Sideload and OEM gotchas
- **Restricted settings (Android 13+)**: a sideloaded APK's accessibility toggle is greyed out until the user opens App info, overflow menu, "Allow restricted settings" (after first trying to enable the service). **[V]** (9to5Google, Android Authority, F-Droid issue). Installing through a session-based installer avoids it; `adb install` is a session install, which is convenient for dev. **[R]** (F-Droid issue.) Onboarding screen must walk through this.
- **Battery killers**: dontkillmyapp ranks Huawei, Xiaomi, OnePlus, Samsung worst. **[V]** Xiaomi: lock app in recents, Autostart on, Battery saver "No restrictions". Samsung: Battery > "Don't optimize", turn off "Put unused apps to sleep", add to "Never sleeping apps", disable Adaptive battery. **[V]** Onboarding must deep-link to these (`ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`, app-details intent) and re-check after OS updates.
- **Play Store**: accessibility use for clipboard capture is reported to be rejected (Seam keeps it out of its Play build). **[R]** Irrelevant for v1; keep the service in a separate `full` build flavor so a later Play variant can drop it.
- **Optional extra (power users): READ_LOGS over ADB** as the ClipCascade-style exact trigger. Treat as an advanced toggle, off by default; not needed if accessibility signals prove good enough in M1.

## 3. Images and files

- Clipboard images arrive as `content://` URIs in `ClipData.Item.uri` (screenshot preview "Copy", Chrome "Copy image", Gallery). **[R]** (ClipCascade reads `image/*` + `item.uri`.) The read grants us temporary access to that URI while that clip is current, so open the stream and copy the bytes **immediately** on a worker thread, before the next copy replaces the clip. Kotlin: `ContentResolver.openInputStream`, decode with `BitmapFactory`, re-encode PNG, hand to Rust; Rust writes `images/` and `thumbs/` per CLAUDE.md. Cap size (e.g. 20 MB) and downscale huge bitmaps.
- Text: take `item.text` and `item.htmlText` (HTML flavor maps to `RawContent::Text.html`); item 0 only in v1, multi-item later.
- Other URIs (files): v1 stores a `Kind::File` row with display name and size via `OpenableColumns`, no bytes. Moving file content is a v2 feature.
- Share target for images: `ACTION_SEND` and `ACTION_SEND_MULTIPLE` with `image/*`, reading `EXTRA_STREAM` URIs (the sender grants read permission with the intent). Works with no focus tricks, so it is the reliable path for images.

## 4. Sync v1 (desktop-hosted LAN hub)

Goal: history flows both ways, E2EE, no cloud. The desktop is the hub; the phone is a client. Same event/crypto model as `docs/system-design.md` sections 5-6 so the hosted relay later is a drop-in (the relay just replaces the hub).

### 4.1 Pairing
1. Desktop (Settings > Pair phone) starts the hub if needed, creates a one-time `pair_secret` (16 random bytes, 3-minute expiry, single use) and shows a QR containing
   `cliptrail://pair?v=1&hosts=192.168.1.23,10.0.0.5&port=47821&space=<space_id>&dpub=<desktop X25519 pubkey, b64url>&secret=<b64url>`.
   `hosts` lists every private IPv4 of the PC (VPN/virtual adapters make "the" IP ambiguous); phone tries each.
2. Phone scans (Tauri's official barcode-scanner plugin is the likely choice **[?]** not verified; CameraX + ML Kit is the fallback), generates an X25519 keypair, then `POST /v1/pair` with `{device_id, name, phone_pub, proof}` where `proof = HMAC-SHA256(pair_secret, phone_pub || device_id)`.
3. Desktop checks proof, computes `shared = X25519(desktop_priv, phone_pub)`, `k = HKDF-SHA256(shared, salt=pair_secret, info="cliptrail-pair-v1")`, and returns `MasterKey` sealed with XChaCha20-Poly1305 under `k` (AAD = space_id || device_id) plus a random 32-byte `device_token` (desktop stores only its hash) and the current `head_seq`. Pair secret is burned.
4. Why this is safe on a LAN: the QR is an out-of-band channel carrying the desktop's public key (an attacker cannot swap it) and the secret (an attacker cannot forge `proof`). The MasterKey never crosses the wire unwrapped. The desktop UI shows the phone's name and a 4-digit check code derived from `k` for confirmation.
5. Phone stores MasterKey in Android Keystore-wrapped storage; token in app-private storage.

### 4.2 Transport and auth
HTTP + WebSocket served by axum inside the desktop app (bundled-relay idea from clipper, `docs/technical-blueprint.md` section 2). Plain HTTP is acceptable in v1 because every payload is sealed; but a bearer token in a cleartext header can be sniffed, so requests carry `Authorization: CT1 device=<id>, ts=<unix ms>, mac=<HMAC-SHA256(token, method|path|ts|sha256(body))>` instead of the raw token, with a ±2 min clock window. The Rust sync client (not OkHttp/WebView) makes the connections, so Android's cleartext-traffic policy does not get in the way **[R]** (the policy is advisory for raw sockets; confirm in M8). Upgrade path: self-signed TLS with the fingerprint added to the QR.

### 4.3 Endpoints (v1)
| Method | Path | Body / notes |
|---|---|---|
| GET | `/v1/hello` | `{proto:1, space_id, hub_device_id, head_seq}`; no secrets |
| POST | `/v1/pair` | as 4.1; unauthenticated but proof-gated, rate-limited |
| POST | `/v1/events` | `[Envelope]`; idempotent by `event_id`; returns `{accepted:[event_id], head_seq}` |
| GET | `/v1/events?since=<seq>&limit=200` | `{events:[{seq, ...Envelope}], head_seq}` catch-up |
| WS | `/v1/stream?since=<seq>` | server pushes `{"t":"event","seq":N,...}`; client sends `{"t":"push","env":{...}}` and `{"t":"ack","seq":N}`; ping every 20 s |
| PUT/GET | `/v1/blobs/{blob_id}` | sealed image bytes; PUT before the referencing event is posted |
| DELETE | `/v1/devices/me` | unpair; desktop UI can also revoke (kills token) |

### 4.4 Event format
Envelope (what the hub stores/relays): `{event_id (ULID), device_id, ts, nonce (24 B), ct}`; the hub adds `seq` (monotonic, hub-assigned). `ct = XChaCha20-Poly1305(MasterKey, nonce, plaintext_json, AAD = event_id || device_id || space_id)`.
Plaintext events:
- `ClipAdded {hash, kind, subkind, text?, html?, source_app?, captured_at, copy_count, image?: {blob_id, w, h, thumb_b64 (<=20 KB)}}`
- `ClipUpdated {hash, pinned?}`
- `ClipDeleted {hash, ts}` (tombstone; receiver wipes content, keeps hash+ts)
Identity across devices is the **content hash** already UNIQUE in the store, so there is no id-mapping table. Conflict rule: last-writer-wins by `ts` for `pinned`/delete; a later `ClipAdded` newer than a tombstone resurrects (copying it again is intent). Sensitive/masked clips are never emitted.

### 4.5 Dedup, echo, offline
- **Dedup**: on `ClipAdded`, if the hash exists: bump `last_seen_at`/`copy_count` (max of the two), do not duplicate. Same path as local `IngestOutcome::Bumped`.
- **Echo suppression**: applying a remote event writes to the store directly, never through the capture pipeline, and v1 does **not** write remote clips to the phone's clipboard (user taps a clip to copy it). That removes the loop and the Android 12+ toast problem. If auto-copy is added later, record `last_written_hash` and ignore a capture with that hash for 5 s (decentpaste `set_last_hash` pattern). Devices also skip events whose `device_id` is their own when replaying.
- **Laptop off**: the phone keeps an `outbox` (events with no `accepted` ack) and its `last_seq` cursor. It retries with backoff (1 s ... 60 s) and on Wi-Fi-available `NetworkCallback`, then POSTs the outbox and does `GET /v1/events?since=last_seq`. Nothing blocks local capture. Mirror image for the laptop being on while the phone sleeps: the hub log is the queue; the phone catches up on next connect. Doze may cut the socket on an idle phone; accepted, because catch-up is cheap.
- **Images**: event carries a thumbnail inline so lists render at once; full image fetched lazily by `blob_id` when opened (or in background for the newest N on Wi-Fi). Blobs are sealed with the same key; nonce prefix in the blob.
- **Discovery after IP change**: try QR hosts, then (optional, v1.1) mDNS `_cliptrail._tcp` via Android `NsdManager`; desktop re-shows its current IP in the paired-devices list.

### 4.6 Windows side
- First time the hub binds a port Windows Defender Firewall shows "has blocked some features of this app" with Private/Public checkboxes; inbound is default-deny and a Private-only allowance does not apply on a Public-profile network. **[V]** (geekrewind guide, Microsoft training module.) Home Wi-Fi is often mislabeled Public: the pairing screen should detect "no inbound connection within 20 s" and show the fix (set network to Private, or Allow an app through firewall). The NSIS installer can add an inbound rule for the exe, `profile=private`, `netsh advfirewall firewall add rule ...` (needs admin at install time).
- Bind `0.0.0.0` only while sync is enabled; reject non-private source IPs. Prefer fixed port 47821, fall back to random and put it in the QR.
- Failure causes to document: AP/client isolation on guest Wi-Fi, phone full-tunnel VPN, corporate Wi-Fi.

### 4.7 Where the code lives
New shared crate `clipd-sync` (envelope, crypto, pairing math, hub handlers, client with outbox) used by the desktop app (hub mode) and the Android lib (client mode). Hub axum code only compiled for desktop; client + crypto for both. Later `clipd-relay` reuses the same handlers.

### 4.8 Android local-network permission
Android 17: apps targeting API 37 get local-network access blocked by default and need the `ACCESS_LOCAL_NETWORK` runtime permission; apps targeting lower SDKs with `INTERNET` get it implicitly; blocked TCP connections just time out. **[V]** (developer.android.com/privacy-and-security/local-network-permission.) Plan: target 36 for now, declare `ACCESS_LOCAL_NETWORK` and request it during pairing before ever bumping to 37; the symptom of getting this wrong is a silent connect timeout.

## 5. Toolchain on this machine (checked 2026-11-01)

| Item | State |
|---|---|
| `ANDROID_HOME` | set (user env) to `C:\Users\abhijit\AppData\Local\Android\Sdk` **[V]** |
| Android Studio | installed (AI-261, "2026.1.3"), bundled JDK at `C:\Program Files\Android\Android Studio\jbr` **[V]** |
| `JAVA_HOME` | **unset**; `java` on PATH is Oracle 17.0.12 **[V]** |
| SDK platforms | android-34, 35, 36, 36.1, 37.0 **[V]** |
| build-tools | 34.0.0, 35.0.0, 35.0.1, 36.0.0 **[V]** |
| platform-tools / adb | 37.0.1 present; **no device attached** **[V]** |
| **NDK** | **missing** (`Sdk\ndk` does not exist) |
| **cmdline-tools (sdkmanager)** | **missing** |
| Rust | 1.96.0; only `x86_64-pc-windows-msvc` installed; no `cargo-ndk`, no `tauri-cli` **[V]** |
| Node / npm | 22.17.1 / 11.11.0 **[V]** |
| Emulator + `system-images/android-36/google` | present (emulator is heavy on this machine, see below) |
| RAM / disk | 8 GB RAM (tight for Studio + Gradle + rustc), 145 GB free **[V]** |

**To install / set (nothing installed by me):**
1. Android Studio > Settings > Languages & Frameworks > Android SDK > SDK Tools: tick **NDK (Side by side)** (latest stable; I could not confirm an exact version string, pick what the list offers, r27 or newer) and **Android SDK Command-line Tools (latest)**. About 2 GB.
2. User env vars (PowerShell, then restart terminals):
   ```powershell
   [Environment]::SetEnvironmentVariable("JAVA_HOME","C:\Program Files\Android\Android Studio\jbr","User")
   $ndk = (Get-ChildItem "$env:ANDROID_HOME\ndk" | Sort-Object Name | Select-Object -Last 1).FullName
   [Environment]::SetEnvironmentVariable("NDK_HOME",$ndk,"User")
   ```
   (Per the Tauri prerequisites page: JAVA_HOME = Studio `jbr`, NDK_HOME = a version folder under `ndk`.) **[V]**
3. Rust targets (phone is arm64; add the rest only when needed):
   ```powershell
   rustup target add aarch64-linux-android
   rustup target add x86_64-linux-android   # only if using the emulator
   ```
   Tauri's page lists all four (`armv7`, `i686` too). **[V]**
4. Tauri CLI: `cargo install tauri-cli --version "^2" --locked` (or `npm i -D @tauri-apps/cli@^2` in the app). Then `cargo tauri android init` from the desktop app crate generates `gen/android` (commit it, as decentpaste does).
5. Windows Developer Mode ON (symlink creation during `tauri android dev`; commonly reported **[?]**, harmless to enable).
6. Phone: Settings > About > tap Build number 7x > Developer options > USB debugging; plug in, accept the RSA prompt, confirm `adb devices` shows the phone. (Wireless debugging works on Android 11+.)
7. Build through `scratch\msvc.cmd` for the desktop/Windows parts as usual; Android Rust builds use NDK clang and do not need `link.exe`, but run the Tauri CLI from PowerShell, not Git Bash.
8. Not needed for option (a): `cargo-ndk`, `uniffi-bindgen`. (Needed only if we later fall back to option b: `cargo install cargo-ndk`.)
9. 8 GB RAM tips: close Android Studio during CLI builds, set `org.gradle.jvmargs=-Xmx2g` in `gen/android/gradle.properties`, avoid the emulator; use the real phone.
10. Optional: `adb shell pm grant <pkg> android.permission.READ_LOGS` for the logcat trigger experiment (section 2.9).

## 6. Milestones (smallest first)

Each milestone ends with something you can see on the phone.

| # | Milestone | Done when |
|---|---|---|
| **M0** | **Phone ready.** Developer options, USB debugging, `adb devices` lists the phone. | `adb shell getprop ro.build.version.release` returns the version; note model/OEM here. |
| **M1** | **Capture spike, pure Kotlin, no Rust, no Tauri** (throwaway Android Studio project; the one thing that can sink the plan). AccessibilityService logging every candidate event (package/class/text, nothing else) to an on-screen list + logcat; overlay focus-grab reader; clip timestamp guard; sensitive-flag check. Test matrix: Chrome, Firefox, Samsung Internet/Chrome, WhatsApp, Gmail, Termux, a PDF viewer, Gboard "copy" in suggestions, screenshot "copy", share-sheet "Copy", long-press "Copy link". | On the owner's phone: at least 90% of 30 deliberate copies captured within 1 s, with correct text and package; false-positive clips rejected by the timestamp guard; written list of which event signatures this phone emits (toast/overlay class names); measured side effects (keyboard dismissal, selection loss, video pause, paste-toast). Also check `adb logcat -s ClipboardService` for the denial line. **Go/no-go:** if under ~70%, plan falls back to tile/notification/share-target-first UX and the optional READ_LOGS mode. |
| **M2** | **Rust under the service.** Toolchain installed; `cargo tauri android init`; Tauri app skeleton with `clipd-core` linked; `initCore(dataDir)` and `ingestText` JNI functions; the M1 service (ported) calls them. Decide SQLCipher vs blob-encryption here. | Copy text in Chrome with ClipTrail's UI **closed**; later `adb shell run-as <pkg> sqlite3 files/...` shows the row, classified correctly; no clip text in logcat. |
| **M3** | **Mobile UI.** Shared React UI in a mobile layout (bottom nav, big touch targets, safe-area insets, dark mode): list, search, kind filters, detail sheet, copy-to-clipboard, pin, delete; `clips-changed` event live-updates. | Capture a clip with the app open in the background, bring app up: it is at the top within 300 ms; search and filter work on 1k seeded clips at 60 fps. |
| **M4** | **Manual paths.** Quick Settings tile, notification action, share target (text), `PROCESS_TEXT`, read-on-foreground. | Each of the five paths adds a clip (or dedups) on the phone; shared text works from a cold start. |
| **M5** | **Images.** Clipboard image URI to PNG + thumb; share target for `image/*`; image cards. | Screenshot "Copy" and Chrome "Copy image" each produce an image card with thumbnail; 20 MB cap respected. |
| **M6** | **Staying alive + onboarding.** `specialUse` FGS with notification, boot receiver, onboarding for: enable accessibility (with restricted-settings walkthrough), battery exemption, OEM-specific page; a "capture health" indicator (service connected? last capture time). | After reboot and 8 h idle with screen off, a copy is still captured; health screen reports truthfully. |
| **M7** | **Desktop hub.** `clipd-sync` crate + axum hub in the desktop app; CLI test client; event log table `sync_events`. | `curl` + a test client pair, post events, fetch since seq; two local processes converge. Windows firewall rule documented/installed. |
| **M8** | **Pairing.** QR on desktop, scan on phone, X25519 pairing, token auth. | Phone scans QR and shows "Paired with <laptop>"; wrong/expired QR and replayed proof are rejected. |
| **M9** | **Two-way sync + offline.** WS stream, outbox, catch-up, dedup, tombstones, pin/delete propagate. | Copy on phone appears on laptop in under 2 s and vice versa; airplane-mode the phone, copy 5 clips, reconnect: all 5 arrive once; laptop off then on: catch-up works. |
| **M10** | **Image sync, hardening, release.** Lazy blobs, signing key + release APK, local-network permission, error states. | A 5 MB screenshot copied on the phone is viewable on the laptop; signed APK installs via `adb install`; an update installs over the previous one preserving data. |

Suggested order of attention: M0-M1 this week (no Rust at all), then M2-M4 (capture visible end to end), M5-M6, then sync M7-M10 in parallel with desktop polish.

## 7. Biggest risks
1. **Accessibility heuristics miss copies** on the owner's phone (OEM-specific toasts, apps with custom copy UI). Mitigated by M1 go/no-go and the five manual paths.
2. **Focus-grab side effects** (keyboard flicker, selection loss, toast per capture) make the app annoying. Measured in M1; users may need to accept the toast.
3. **OEM background killing** (Xiaomi/Samsung/OnePlus) silently stops capture or sync; needs onboarding + health indicator.
4. **Restricted settings** hide the accessibility toggle on a sideloaded APK (Android 13+); onboarding must explain, or install via session installer.
5. **Tauri Android friction on this 8 GB Windows box** and C++/`c++_shared` linking surprises; SQLCipher on Android NDK.
6. **Windows firewall / Public network profile / client isolation** make LAN sync look broken; need explicit diagnostics.
7. **Android 17 local-network permission** if targetSdk is bumped to 37 without declaring it.

## 8. Not verified (explicit)
Exact event class names for the Android 13+ clipboard overlay and Samsung toast on the owner's phone; that an accessibility overlay window with focus satisfies the OS "app in focus" test on every OEM (Seam reports one Samsung device); the exact AOSP log line and that the listener triggers it for background apps; whether FGS may be started from `onServiceConnected`; Tauri barcode-scanner plugin suitability; Windows Developer Mode requirement; exact NDK version to install; API level of `EXTRA_IS_SENSITIVE` per the docs page (I believe 33).

## Sources
- Android 10 privacy changes (clipboard limited to default IME or focused app): https://developer.android.com/about/versions/10/privacy/changes
- Secure clipboard handling (EXTRA_IS_SENSITIVE, Android 12 toast, Android 13 auto-clear): https://developer.android.com/privacy-and-security/risks/secure-clipboard-handling
- Foreground service types: https://developer.android.com/develop/background-work/services/fgs/service-types
- Foreground service timeouts (dataSync 6 h / 24 h): https://developer.android.com/develop/background-work/services/fgs/timeout
- Background activity launch exemptions: https://developer.android.com/guide/components/activities/background-starts
- Android 16 behavior changes (no clipboard changes): https://developer.android.com/about/versions/16/behavior-changes-all
- Local network permission (Android 17): https://developer.android.com/privacy-and-security/local-network-permission
- Tauri prerequisites (JAVA_HOME, NDK_HOME, targets): https://v2.tauri.app/start/prerequisites/
- Tauri mobile plugin development: https://v2.tauri.app/develop/plugins/develop-mobile/
- Tauri docs PR on Kotlin-to-Rust JNI: https://github.com/tauri-apps/tauri-docs/pull/3575/files
- Seam PR #26, accessibility clipboard capture (AGPL, ideas only): https://github.com/aveterosystems/seam/pull/26
- acopy client release note (accessibility + transparent activity): https://github.com/acopy-org/client/releases/tag/v1.0.48
- Clipboard Actions, Android 10 workarounds: https://github.com/joecks/clipboard-actions-community/blob/master/help/anndroid10.md
- Restricted settings for sideloaded accessibility: https://9to5google.com/2022/05/06/android-13-sideloaded-accessibility/ , https://www.androidauthority.com/android-13-sideloading-apps-restrictions-3161162 , https://gitlab.com/fdroid/fdroidclient/-/issues/2680
- OEM battery killers: https://dontkillmyapp.com/ , https://dontkillmyapp.com/xiaomi , https://dontkillmyapp.com/samsung
- Windows firewall first-run prompt and profiles: https://geekrewind.com/what-to-do-when-windows-firewall-blocks-apps-you-need/ , https://learn.microsoft.com/en-us/training/modules/manage-defender-windows-client/6-explore-windows-defender-firewall-advanced-security
- Tauri 2 Android port postmortem (c++_shared crash): https://git.manko.yoga/manawenuz/wz-phone/src/branch/opus-DRED/docs/incident-tauri-android-init-tcb.md
- Local references: `refs/decentpaste--decentpaste/decentpaste-app/{tauri-plugin-decentshare,src-tauri/gen/android}`, `refs/ClipCascade--Sathvik-Rao/ClipCascade_Mobile/src/android/app/src/main/` (GPL-3.0, read for ideas only)
