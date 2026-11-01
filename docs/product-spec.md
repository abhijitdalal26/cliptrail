# Clipboard history across every device

Nov 1, 2026 · @Abhjit Dalal

Build a Rust clipboard history that captures everything you copy on every device, sorts it into a few major categories automatically, and lets you browse back through time and paste from any section.

## Goals and non-goals

The problem: you copy dozens of things a day, Win+V forgets them, and nothing lets you find last Tuesday's code snippet from your phone. The fix is a history you never have to manage.

Goals for v1:

- Never lose a copy. Everything is captured and kept until you choose to expire it, with images expiring on their own schedule.
- Sort without effort. Each clip lands in a major category (link, code, note, image, file, sensitive) with no tagging by hand.
- Go back as far as you want. Every clip carries a timestamp, and the dashboard scrolls and jumps through time.
- Same history on laptop and phone, end-to-end encrypted so the server never reads it.
- Paste from a section in two keystrokes: open the picker, choose Links or Code, choose the clip.
- Screenshots store a path when a file exists, and a thumbnail, so images are not duplicated.

Non-goals for v1:

- Team sharing or accounts for other people.
- Automatic phone-to-laptop capture on iOS, which the OS does not allow.
- AI summaries or cloud models. Classification stays local and cheap.
- Editing or rewriting clips beyond pin, move and delete.

## What exists today

From what I found, tools are either good at history on one machine or good at sync, and none leads with automatic categories plus a dashboard you can browse through time. That is the gap.

| Player | Platforms | Sync model | Worth studying for | What it lacks |
| --- | --- | --- | --- | --- |
| Ditto | Windows | LAN sharing between PCs | Deep history, groups, hotkeys, SQLite store | No phone, dated UI |
| Win+V | Windows | Microsoft account, text and links only | Zero setup | About 25 items, unpinned items lost on restart |
| ClipCascade | Windows, macOS, Linux, Android | Server or P2P, end-to-end encrypted, self-hostable | Closest working cross-platform sync, Android client | Sync-first, little organization |
| UniClipboard | Desktop plus iOS and Android app | P2P with encrypted relay fallback | Rust and Tauri, no central server, encrypted history | Sync-first, no category dashboard |
| DecentPaste | Desktop and Android | libp2p on the local network | Rust, RustCrypto, mobile client | Local network only |
| ORNAS | Linux, macOS, Windows | None, offline only | Capture pipeline with a categorize stage, collections, tags | No sync, no mobile |
| Octoclip | iOS, Android, Windows, macOS | Nearby, cloud, or your own WebDAV or S3 | Aliases, type filters, Android authorization paths | Closed source |
| KDE Connect | All major platforms | Local network | Device pairing | A bridge, keeps no history |
| Paste and Maccy | Apple only | iCloud for Paste | Best-regarded history UIs | Apple only |

These facts come from search results and project pages, not from running each app. Test the top three on your own devices before you design around them.

## How it works

A shared Rust core does the work on every device, and each client is a thin shell around it. Every clip moves through one pipeline: normalize, hash, dedup, classify, extract metadata, persist, notify. ORNAS uses a similar seven-stage flow, which is worth reading before you write your own.

Components:

1. core, a Rust library crate with the clip model, SQLite storage, encryption, the classifier and the sync protocol.
2. desktop, a Tauri 2 app for Windows, macOS and Linux that runs capture, the tray, the hotkey picker and the dashboard.
3. server, an axum and tokio WebSocket relay that stores and forwards ciphertext only.
4. android, a Kotlin shell that handles capture permissions, the tile and the share target, and calls core through UniFFI.
5. web, a read-only dashboard for any browser, including iOS.

Data model, one row per clip:

| Field | Purpose |
| --- | --- |
| id | ULID, sortable by time, safe to create offline on any device |
| captured\_at | UTC timestamp, the axis the dashboard scrolls along |
| device\_id, source\_app | Where it was copied, used for filters |
| kind, subkind | Major category and detail, such as code and rust, or link and github.com |
| content\_hash | Dedup key, so the same copy on two devices is one clip |
| text, thumb | The body, or a thumbnail for images |
| file\_path | Original screenshot or file path when one exists, instead of a duplicate copy |
| section\_id, pinned | User organization on top of the automatic category |
| sensitive, expires\_at | Skip or expire passwords, codes and old images |

Sync is an append-only log of encrypted events, so a device that was offline just replays what it missed. Text is encrypted on the client with a key that never reaches the server. Pair a new device by scanning a QR code from one you already trust.

## Classifier

Aim for right about nine times in ten at the top level, not perfect. A wrong guess costs one drag to another section, so cheap and fast beats clever. Run it locally on every clip in a few milliseconds.

Major categories for v1:

| Category | How it is detected | Subkind examples |
| --- | --- | --- |
| Link | URL pattern, a single token starting with a scheme or www | Domain, such as github.com |
| Code | Multi-line, high symbol density, indentation, braces, keywords, then a language guess | rust, python, sql, json, shell |
| Sensitive | OTP-shaped short digits, API key and token patterns, private key headers, or a flag from the source app | Skipped or expired by default |
| Contact and data | Email, phone, address, hex color, number with unit | email, phone, color |
| File and image | Image data on the clipboard, or a file path | screenshot, path |
| Note | Everything else: prose, short text, lists | Length bucket |

Layers, in order, stopping at the first confident answer:

1. Deterministic rules with regular expressions for links, emails, phone numbers, colors, paths and secret patterns. These are fast and nearly always right.
2. A structure heuristic for code: line count, brace and semicolon density, indentation, keyword hits. This separates code from prose well enough for the category.
3. Language naming for code with Google's Magika, a Rust library that detects over 200 content types with a small model run through ONNX. It was built for files, so test it on short snippets before trusting it, and fall back to "code, unknown language" when its confidence is low.
4. Fallback to Note.

User corrections are the real training signal. When someone moves a clip, store a rule from it, such as source app or domain maps to a section, and apply it before the other layers.

How to measure it: export your own Ditto history, hand-label a few hundred clips, and track accuracy on the six top-level categories. Do this before building the dashboard.

## Dashboard and paste flow

The dashboard is where this app should beat everything else, so design it before the sync layer. It has to look good, scroll back as far as you like, and make a clip findable in a few seconds.

Views:

- Timeline. Clips grouped by day, then week, then month as you scroll back, with a date scrubber and a calendar heatmap to jump to any day.
- Category lanes. Tabs or columns for Links, Code, Notes, Images and Files, each with cards suited to the type: syntax-highlighted and collapsible code, links with favicon and page title, image thumbnails with the source path.
- Sections. Your own folders on top of the automatic categories, for example Links with a Notes subsection. Drag a clip to move it, and the move teaches the classifier.
- Search. One box with filters for category, date range, device and source app. Index it on the client so it works over encrypted data.
- Day and week summaries. Counts per category, the most copied domains, and a pinned strip for things you reuse.

Paste flow, the daily-use part:

1. A global hotkey opens a small picker at the cursor.
2. Pick a section, or type to search across all of them.
3. Press Enter. The app writes the clip to the clipboard, returns focus to the previous window and sends the paste keystroke. Recopy implements this flow on Windows and macOS and is a good reference.

Technical notes: virtualize the list so a million rows scroll smoothly, keep thumbnails small, load full images on demand, and keep light and dark themes from the start.

## Android capture

Since Android 10, a background app cannot read the clipboard unless it is the active keyboard or the app in focus. Users should not have to switch keyboards, so use these paths in this order:

1. Accessibility Service as the main path. The app notices copy events and briefly takes focus to read the clipboard. It needs one broad permission, so say plainly what is read and never send it unencrypted.
2. Optional ADB-granted READ\_LOGS permission for people who want to avoid Accessibility. Tasker and Join use this approach, and Tasker's developer says it does not work in every case.
3. A Quick Settings tile and a share target as the fallback, so saving always works with one tap.
4. A foreground service with a persistent notification to keep capture alive.

Expect trouble on phones with aggressive battery management. Onboarding has to walk the user through the autostart and battery exclusions for their brand, and that screen will matter as much as any feature. Skip clips that Android flags as sensitive, and add a per-app blocklist for banking apps.

Google Play reviews Accessibility Service use closely, so plan to ship first as an APK from GitHub or F-Droid. Laptop to phone is easy everywhere. iOS can only offer a share extension, a widget and the web dashboard, with no automatic capture.

## Codebases to study

Clone these into a scratch folder, outside your project, and have your coding agent read each one and write a short summary answering the question beside it. Treat them as references, not as code to paste. Check each license before reusing anything, because GPL code pulls your whole app under GPL and Recopy's license forbids commercial use.

| Repo | Stack | Question for your agent |
| --- | --- | --- |
| [Sathvik-Rao/ClipCascade](https://github.com/Sathvik-Rao/ClipCascade) | Java server, Windows, macOS, Linux and Android clients | How does the Android client capture the clipboard in the background, and how do server mode and P2P mode share one protocol? Licensing is reported inconsistently (GPL-3.0 in one listing, MIT in another), so read the LICENSE file. |
| [UniClipboard/UniClipboard](https://github.com/UniClipboard/UniClipboard) | Rust, Tauri 2, iroh P2P | How are devices paired, how does the encrypted relay fallback work, and how is history encrypted at rest with a key in the system keyring? Its mobile app lives in UniClipboard/UniClip. |
| [decentpaste/decentpaste](https://github.com/decentpaste/decentpaste) | Rust, Tauri v2, libp2p, RustCrypto, Apache-2.0 | How does a Tauri app ship an Android build, and how is the crypto layered? |
| [sanromarth/ornas](https://github.com/sanromarth/ornas) | Rust 2024, Tauri v2, SQLite | How is the seven-stage pipeline (normalize, hash, dedup, categorize, metadata, persist, notify) structured, and how are collections and tags modeled? |
| [windoze/clipper](https://github.com/windoze/clipper) | Rust workspace: indexer, REST server, Tauri 2 desktop, CLI | How is full-text search split into its own crate, and how does the server expose sync? |
| [tipsxBase/clipboard](https://github.com/tipsxBase/clipboard) | Rust, Tauri v2, Vue 3 | How does it ignore password managers, keep sensitive items in memory only, and encrypt the database with AES-GCM? |
| [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard) | Tauri 2, Rust | How do tags, WebDAV and MQTT sync, and privacy masking of IDs and phone numbers work? |
| [darkingtail/Recopy](https://github.com/darkingtail/Recopy) | Rust, Tauri v2, React, SQLx | How does paste-back work: write to the clipboard, restore the previous window, send the paste keystroke? Read for ideas only, since the license is PolyForm Noncommercial. |
| [0PandaDEV/Qopy](https://github.com/0PandaDEV/Qopy) | Rust, Tauri, GPL-3.0 | What is a minimal, clean capture and settings structure in a Tauri clipboard app? |
| [sabrogden/Ditto](https://github.com/sabrogden/Ditto) | C++, SQLite, GPLv3 | What is in its SQLite schema, how are groups stored, and can your sidecar read the database read-only? |
| [hluk/CopyQ](https://github.com/hluk/CopyQ) | C++, Qt | How do tabs, tags and scripting organize clips, and what would you copy for sections? |
| [google/magika](https://github.com/google/magika) | Rust CLI and library, ONNX model | Can the Rust crate label short code snippets accurately, and how are its per-type confidence thresholds used? |

Libraries to evaluate for the core, checking current versions and maintenance first: arboard or clipboard-rs for clipboard access and change watching, rusqlite or sqlx for storage, chacha20poly1305 or aes-gcm with argon2 for encryption, axum with tokio-tungstenite for the relay, and uniffi for Android bindings.

## Build order

Build what you will use daily first, and add platforms only when you need them. These estimates assume one person working part-time with a coding agent, and are guesses to adjust after phase 1.

1. Weeks 1 to 2: install Ditto today, then build core with the clip model, SQLite and tests, plus a Windows capture daemon with a command-line search. Run the classifier on your own exported history and measure it.
2. Weeks 3 to 4: the Tauri dashboard with timeline, category lanes, search and the paste picker. This is the part that has to feel good.
3. Weeks 5 to 6: encryption, the axum relay, device pairing by QR code, and sync between two machines. Add the web dashboard so your phone can read history from day one.
4. Weeks 7 to 10: the Android app with Accessibility capture, the tile and the share target, then the battery-optimization onboarding.
5. Later: macOS and Linux builds from the same Tauri app, with X11 first on Linux. iOS last, and only if you need it, since it offers the least for the most work.

## Risks and open questions

The biggest risk is Android background capture, which can go quiet on phones with strict battery management. Build it early and test on your own phone before committing to the rest.

Other risks:

- Privacy. A clipboard history stores passwords and tokens, so default to skipping sensitive clips, encrypt on the client, and offer a per-app blocklist.
- Scope. Sync, classification, dashboard and three platforms is a lot. Ship the Windows dashboard first and let it prove itself.
- Licensing. If you reuse code from a GPL project, your app becomes GPL. Decide early whether you want that.
- Classifier accuracy on short snippets, especially language detection. Measure it on real history before building UI around it.

Open questions:

- [ ] Hosted relay, self-hosted relay, or P2P first?
- [ ] Which license for the project?
- [ ] Does ClipCascade already cover enough of sync that you only build the organization layer?
- [ ] Name and distribution: GitHub APK and F-Droid first, Play Store later?

## Sources

Facts come from search results and project pages, not hands-on testing. Verify licenses and activity before relying on any of them.

- [ClipCascade](https://github.com/Sathvik-Rao/ClipCascade)
- [UniClipboard](https://github.com/uniclipboard/uniclipboard)
- [DecentPaste](https://github.com/decentpaste/decentpaste)
- [ORNAS](https://github.com/sanromarth/ornas)
- [Clipper (windoze)](https://gh.r-mj.com/windoze/clipper)
- [tipsxBase clipboard](https://github.com/tipsxbase/clipboard)
- [TieZ](https://github.com/jimuzhe/tiez-clipboard/wiki)
- [Recopy](https://github.com/darkingtail/Recopy)
- [Qopy](https://awesome.ecosyste.ms/projects/github.com%2F0PandaDEV%2FQopy)
- [Ditto on SourceForge](https://sourceforge.net/p/ditto-cp/)
- [Magika 1.0 coverage](https://www.infoq.com/news/2025/12/magika-rust-file-type-detector)
- [Octoclip on Android clipboard limits](https://octoclip.app/learn/android-clipboard-not-saving/)
- [Join and READ\_LOGS workaround](https://joaoapps.com/AutoApps/Help/Info/com.joaomgcd.join/android_10_read_logs.html)
