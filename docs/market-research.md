# Clipboard history across every device — market research and what to build

Date: 2026-11-01 · Companion to "Clipboard history across every device.md"

## Verdict on the idea

The gap in your doc is real and still open: **history tools don't sync, sync tools don't organize**. That holds up after reviewing what users actually say. Three things changed the picture since the doc was written, though:

1. **OS vendors ate the bottom of the market.** macOS Tahoe added clipboard history to Spotlight (up to 7 days retention, text + images). Microsoft shipped bidirectional clipboard sync through Phone Link / Link to Windows in Dec 2025 — Windows ↔ Android latest-clip, works with Gboard, no SwiftKey needed. "Sync my last clip" is becoming a solved, free, built-in problem. Your app cannot compete on "latest clip on my phone" — it has to win on **deep organized history**, which none of the OS features touch.
2. **Auto-categorization is less of a moat than the doc assumes.** Paste already detects types (it won't turn a 6-digit OTP into a color swatch). Deck and newer Tauri apps (Ortu, Beetroot) ship content-type detection and even auto-grouping. The moat is not categories alone — it's categories + time-travel dashboard + E2EE sync on every device, combined.
3. **The Android capture problem has a smarter answer than Accessibility.** Syncer (a real product now) sidesteps background clipboard reads entirely by shipping its own **keyboard** — the keyboard is the one app Android always lets read the clipboard. It also doubles as the paste-back surface on phones. This is worth considering alongside or instead of the Accessibility path.

Net: worth building, and the doc's architecture is sound. Scope is the main risk — see "What to build" below.

## What people actually say about existing products

| Product | What people praise | What people complain about / ask for |
| --- | --- | --- |
| **Ditto** (Windows) | "It just works, always"; deep history, groups, images; gold-standard Windows UX | Begging for Mac/cloud/mobile since 2020 (issue #4 — maintainer says no plans); dated UI; LAN-only sync is flaky and recently had a file-read security issue (#1170) |
| **Win+V** (built-in) | Zero setup | 25-item cap, unpinned items lost on restart, no search, can't resize, pinned items get buried |
| **Maccy** (macOS) | Best-in-class ignore filters ("single reason I stay"), lightweight, native | No iOS companion, no sync/E2EE (#1375); wants canned responses/snippets (#1125), better pinned layout (#1286), preview popover friction, Raycast extension |
| **Paste** (Apple) | Beloved UX; Pinboards; Paste Stack; iCloud sync; now Shared Pinboards + Paste for Teams | **Subscription backlash** (App Store ~2.9★); bulky bottom-bar UI; buggy updates; iOS can't auto-capture — needs share extension/keyboard |
| **CopyQ** (cross-platform) | Powerful, scriptable, tabs/tags | Steep learning curve, HDPI/Wayland bugs, flaky paste-back to window; wants sequential-paste hotkey (#3348), better pins, cross-tab search |
| **KDE Connect** | Device pairing, free | Android→PC clipboard is **manual** (Android 10+ block); users resort to ADB/Shizuku/Magisk hacks; sync silently dies after ~1-2 h; it's a bridge, not a history |
| **Apple Universal Clipboard** | "Magic" when it works | Notoriously flaky (needs unlocked device, same Apple ID); latest clip only, zero history |
| **SwiftKey cloud clipboard** | Was the official path | "Notoriously unreliable"; superseded by Phone Link sync |
| **Raycast clipboard** (macOS) | Good enough for many; snippets with `;triggers` | No rich previews, not resizable, modal; Pro-only sync; single-ecosystem |
| **Sync tools (ClipCascade, UniClipboard, SyncClipboard, rust-clip, Pastefrog, Syncer, ClipBridge)** | E2EE options, self-host, P2P, QR pairing — real products now | Thin or early-stage history (SyncClipboard warns history is "early stage"); little to no organization, no dashboard; setup friction (Docker, servers) |

## Feature asks that keep recurring (ranked by how often they come up)

1. **Cross-device sync** — the top ask by far, across every forum (Ditto #4, Maccy iOS companion requests, KDE Connect hacks, XDA "how I set up sync" guides). The worst-covered pairs: Windows↔Android, Mac↔Android, anything↔iPhone.
2. **Search that finds old clips** — full-text is table stakes; the frontier is **OCR on images** (PowerToys Advanced Paste, Deck, BTT wishlist) and semantic/fuzzy search ("that phone number from last week"). Deck exists solely for this.
3. **Paste as plain text / strip formatting** — recurring pain (Word has no Ctrl+Shift+V). PowerToys, Pure Paste, Notchy exist just for this. Cheap to build, loved by users. Also: strip tracking params from URLs on copy.
4. **Snippets / canned responses** — Maccy #1125, Ditto groups, Raycast Snippets, TextExpander users. People want a named, organized place for reusable text — clipboard history that graduates to a snippet library.
5. **Sequential paste / paste stack (FIFO queue)** — CopyQ #3348, Paste Stack, HN "godsend for Excel workflows": copy 5 things, paste them back in order with one hotkey.
6. **Sensitivity handling** — now effectively table stakes: honor concealed/transient flags, skip password managers by default, per-app blocklist, auto-expire (SnipTray documents the five rules users now expect). Your doc covers this — ship it with good defaults pre-populated.
7. **E2EE + self-host option** — UniClipboard, rust-clip, ClipCascade, SyncClipboard (WebDAV/S3) all lead with this. Users want a hosted option AND a bring-your-own-server option.
8. **Keyboard-first quick paste** — open picker at cursor, type to filter, Enter pastes and restores focus; numbered quick-paste hotkeys (Cmd+1..9). This is the daily-use flow; everyone who loves Ditto/Maccy loves this.
9. **Pinned items that stay usable** — always-on-top, compact layout (Maccy #1286, Win+V complaints).
10. **Lightweight / native feel** — "a background utility should not eat 300 MB" (Electron tax); Tauri+Rust is now the expected stack. Beetroot: 6 MB installer, ~30-50 MB idle.
11. **Merge/append clips** ("append to clipboard like >>"), **retention/export/portable DB**, **device-aware filters** — smaller but real asks.

## The complete picture — what we should be building

### The product in one line

**An encrypted, organized clipboard history that follows you across Windows, macOS, Linux and Android (read-only on iOS/web), with a dashboard you can scroll back through time.**

### Keep from the doc (validated by research)

- Rust core + Tauri desktop + UniFFI Android — the stack the whole category is converging on.
- E2EE sync with QR pairing — UniClipboard proves users accept this model.
- Local rules-based classifier first, corrections as training signal.
- Non-goal: team sharing, iOS auto-capture, cloud AI — all correct calls.

### Add — the features the market keeps asking for that the doc misses

| Priority | Feature | Why |
| --- | --- | --- |
| P0 | **Paste as plain text** (modifier on paste) | Universal pain, ~1 day of work, instantly loved |
| P0 | **Per-app exclusion + concealed-type honoring with defaults** | Trust feature; SnipTray's five rules are the checklist |
| P0 | **Quick-paste at cursor with numbered shortcuts + type-to-search** | This IS the product for daily users |
| P1 | **OCR on image clips** so screenshots are searchable | PowerToys and Deck normalized it; local OCR models are cheap |
| P1 | **Paste stack (FIFO sequential paste)** | Repeated explicit asks; differentiates from every history tool |
| P1 | **Named snippets / canned responses** on top of pinned clips | Maccy's most-wanted feature; natural upgrade path from "pin" |
| P1 | **Keyboard-based Android capture** (own keyboard app, à la Syncer) | The only fully reliable, Play-Store-friendly capture path; doubles as phone paste UI |
| P2 | **iOS share extension + shortcut + keyboard** (not just web dashboard) | Paste proves this is the only tolerable iOS UX; web-only will feel dead |
| P2 | **Merge/append clips, strip tracking params on copy** | Small, beloved power features |
| P2 | **Export/backup + self-hosted relay** | Self-host is a loud minority but a marketing wedge (rust-clip, SyncClipboard) |
| P3 | Semantic search (on-device embeddings) | Emerging; conflicts with "no AI" non-goal — keep optional and local |
| P3 | Shared pinboards / team mode | Paste went here in 2025; real revenue direction, later |

### Adjust

- **Android capture: keyboard first, Accessibility second.** The doc leads with Accessibility Service — it's fragile (Play review scrutiny, battery killers, broad-permission optics). A keyboard is OS-sanctioned clipboard access plus a paste surface on the phone. Support both; ship tile + share target regardless.
- **Reposition the pitch.** "Categories + dashboard" is no longer enough alone — the pitch is *"Ditto's history depth + Paste's UX, on every device, end-to-end encrypted."* Organization is a pillar, not the headline.
- **Don't fight the OS clipboard-sync features.** Phone Link and Universal Clipboard cover "last clip." Don't demo latest-clip sync as the wow moment; demo finding "that code snippet from last Tuesday, copied on my phone, on my laptop."
- **Pricing signal:** users revolt against subscriptions for a clipboard app (Paste's rating drop proves it) but do pay for sync. Open-source core + cheap hosted relay (or self-host free) is the sentiment-friendly model.

### Validation before building (in order)

1. Android keyboard-capture prototype — the whole cross-device story rests on it (biggest risk, as your doc says).
2. Hands-on with UniClipboard, Syncer, Pastefrog, Paste — feel where they annoy you; that annoyance is the spec.
3. Classifier accuracy on real exported history (your doc's plan is right).
4. Paste-back focus restore on Windows + macOS (Recopy reference still holds).

## Sources

- Ditto cross-platform plea: github.com/sabrogden/Ditto/issues/4 · Ditto issues (paste-as-keystrokes, pin panel, LAN sync bugs): github.com/sabrogden/Ditto/issues
- Maccy pro-user feedback (iOS/E2EE/snippets): github.com/p0deje/Maccy/issues/1375, /1125, /1286
- CopyQ sequential paste: github.com/hluk/CopyQ/issues/3348
- Paste reviews/subscription complaints: producthunt.com/products/paste/reviews · iOS limits: pasteapp.io/help
- KDE Connect Android clipboard threads: discuss.kde.org/t/kde-connect-clipboard-sync/6422
- Win+V wishes: makeuseof.com/microsoft-improve-windows-11-clipboard-history
- Microsoft Phone Link clipboard sync (Dec 2025 rollout): 9to5google.com/2025/09/03, windowsforum.com .394527
- macOS Tahoe Spotlight clipboard history: support.apple.com/guide/mac-help/mchl40d5b86b, macrumors 26.1 retention settings
- HN design-goals thread (stack/queue, append, plain text): news.ycombinator.com/item?id=29808487
- Sensitive-clip rules: sniptray.com/blog/how-clipboard-managers-handle-passwords
- New entrants: uniclipboard.app, pastefrog.app, syncerclip.com, clipbridge.me, github.com/Jeric-X/SyncClipboard, github.com/advenimus/rust-clip, deckclip.app, dev.to Beetroot/Ortu posts
