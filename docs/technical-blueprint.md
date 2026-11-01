# Clipboard app — full technical blueprint

Companion to "Clipboard history across every device.md" and `clipboard_market_research.md`.
Codebases below were actually cloned and read on 2026-11-01 (in `~/scratch/clipboard-refs`), not just searched.

---

## 1. Reference codebase guide — what each repo teaches us

License matters first. Sorted by how reusable they are:

| Repo | License | Stack | What to learn from it |
| --- | --- | --- | --- |
| **windoze/clipper** ⭐ closest to our product | MIT | Rust workspace, Axum REST+WS server w/ built-in web UI, Tauri 2 + React desktop, CLI | The overall shape of our app: `clipper-indexer` (SurrealDB + RocksDB + BM25 FTS) as a standalone crate, `clipper-server` (axum, bearer-token auth, optional LAN access, clip sharing via short URLs), `clipper-client` lib, shared React UI package. Study how sync is just REST+WebSocket to an embedded or external server — that IS the self-host model we want. |
| **tipsxBase/clipboard** | MIT | Tauri + Rust | `src-tauri/src/rules.rs` is literally our classifier-correction system: `Rule { conditions (source_app/content_type/content+regex) → action (ignore/mark_sensitive/pin/snippet/add_to_collection) }` with preset rules for 1Password/Bitwarden/KeePassXC. `crypto.rs` = AES-GCM DB encryption. Read and adapt freely. |
| **decentpaste** | Apache-2.0 | Tauri v2, libp2p, RustCrypto | Has a real `ARCHITECTURE.md`. Tauri plugins `tauri-plugin-decentshare` (Android share target in Kotlin) and `tauri-plugin-decentsecret` (Keychain/AndroidKeyStore/Secure Enclave secure storage) — copy the plugin pattern for our Android shell. Crypto: X25519 ECDH pairing → AES-256-GCM, Argon2id PIN fallback. |
| **Jeric-X/SyncClipboard** | MIT | .NET/WinUI3 (not Rust) | Concept only: the flexible server model — client-built-in server, standalone Docker server, **WebDAV or S3 as the relay**. Proves users accept "bring your own storage" and it's how a solo dev ships sync without running infra. Also does image-format transcoding on copy (heic/webp→jpg/gif) — nice detail. |
| **UniClipboard** | AGPL-3.0 ⚠️ read-only | Rust workspace, iroh P2P + encrypted relay, daemon + quick-panel apps, iOS/Android companions | Concept only (AGPL contaminates). `CONTEXT.md` is a great domain glossary: "Space" = trust group with passphrase → MasterKey (32B) encrypts history + transport; QR/invite-code pairing; daemon process + separate GUI; headless server node (ADR-007). Model for our pairing UX. |
| **darkingtail/Recopy** | PolyForm Noncommercial ⚠️ read-only | Tauri v2 + React | Paste-back implementation, the one hard UI thing: store `PREV_FOREGROUND` HWND → `SetForegroundWindow` → poll ≤10ms for focus to settle → `SendInput` Ctrl+V. macOS uses `NonactivatingPanel` so the picker never steals focus + `resign_key_window` before pasting. Ideas only. |
| **Sathvik-Rao/ClipCascade** | GPL-3.0 ⚠️ read-only | Java server, React Native mobile | Android capture trick: `ClipboardFloatingActivity` + `TYPE_APPLICATION_OVERLAY` window that takes focus briefly to read the clipboard, plus a logcat-reader thread (READ_LOGS fallback) and `BootReceiver`/foreground service. Confirms the overlay trick works but is hacky — strengthens the keyboard-first argument. |
| **hluk/CopyQ** | GPL-3.0 ⚠️ | C++/Qt | Concept: tabs/tags/scripting. Feature-request source (sequential paste, cross-tab search). |
| **sabrogden/Ditto** | GPL-3.0 ⚠️ | C++/SQLite | Concept: groups (Ctrl+G hides them from main list — good UX idea), 25/250/2500 history cap settings, LAN peer sync. Read schema if we want a Ditto importer. |
| **Qopy** | AGPL ⚠️ | Tauri+Nuxt | Minimal clean capture/settings structure. |
| **jimuzhe/tiez-clipboard** | GPL-3.0 ⚠️ | Tauri | WebDAV/MQTT sync, privacy masking of IDs/phones. |
| **sanromarth/ornas** | — | — | **Repo is gone (404).** The 7-stage pipeline from the doc survives as a design pattern only — implement normalize→hash→dedup→classify→metadata→persist→notify ourselves. |
| **google/magika** | Apache-2.0 | Rust/Python, ONNX | Tested locally on short snippets — results in §6. |

**Rule of thumb for the agent:** MIT/Apache code can be adapted; GPL/AGPL/PolyForm = read for ideas, reimplement. If we vendor nothing GPL, we stay free to choose MIT or dual-license.

## 2. Tech stack recommendation

Everything converges on the same answer — keep the doc's stack, it matches what the best repos use:

| Layer | Choice | Why / evidence |
| --- | --- | --- |
| Core lib | Rust workspace, one `core` crate | clipper/decentpaste/UniClipboard all do this |
| Storage | **SQLite + rusqlite + FTS5** | Simplest that works: encrypted-at-rest via SQLCipher or our own AES-GCM on blobs (tipsxBase does AES-GCM). FTS5 gives ranked full-text search for free. clipper's SurrealDB is heavier than we need. |
| Desktop | Tauri 2 | Every modern entrant uses it; ~6MB installers, ~30-50MB idle RAM. One codebase → Win/Mac/Linux. |
| Picker/HUD | Tauri webview, non-activating panel (macOS) / foreground-restore trick (Windows, per Recopy) | |
| Sync protocol | **Append-only encrypted event log over WebSocket** + REST catch-up | axum + tokio-tungstenite. clipper proves REST+WS is enough; UniClipboard proves encrypted-relay + QR pairing UX. |
| Crypto | XChaCha20-Poly1305 per clip, X25519 ECDH for pairing, Argon2id for passphrase KEK | decentpaste + rust-clip + UniClipboard all picked this family |
| Relay server | axum, single binary + `docker-compose.yml`, stores ciphertext only | clipper-server is the template (standalone OR bundled-in-app mode!) |
| Android | Kotlin shell + **UniFFI** into core; capture via custom **keyboard IME first**, Accessibility/overlay second, QS tile + share target as guaranteed fallback | |
| iOS | Later: share extension + Siri Shortcut + custom keyboard + read-only web dashboard | Paste's exact formula |
| Web dashboard | The Tauri frontend compiled for web, read-only, talks to relay | clipper-server already ships a built-in web UI this way |

**What I'd change from the doc:** use SQLite instead of inventing storage, and steal clipper's "bundled vs external server" trick — the desktop app can embed the relay for LAN-only users, while self-hosters run the standalone binary.

## 3. Two-way distribution (your "hackers vs non-developers" split)

This maps cleanly onto what clipper + SyncClipboard + Pastefrog already proved works:

**Track A — Self-host (developers):**
- `docker run clipd/relay` or a single binary → you get the relay, pairing, and web dashboard.
- Docs: `docker-compose.yml`, plus WebDAV/S3-as-relay later (SyncClipboard's model) for zero-infra self-hosting.
- Server stores ciphertext only; open source; "your clipboard never touches our servers" is the marketing line.
- Free forever. This is the trust engine.

**Track B — Hosted (non-developers):**
- **Yes, this needs login/accounts** — you were right that people ask for it. Account = email + password (or "sign in with Google/GitHub") that exists ONLY to host your identity; the encryption keys never leave devices. Pairing a new device = sign in OR scan QR from an existing device.
- Hosted relay, web dashboard at app.ourname.com, device management page (list/revoke devices).
- Suggested model: free tier (2-3 devices, 30-day history) → paid tier (~$2-3/mo or one-time) for unlimited devices + history. Research shows users tolerate paying for sync but hate subscriptions for a clipboard app — price it like Pastefrog or offer a lifetime option.
- **Critical trust detail:** the account stores email + device list, never plaintext clips. Publish a SECURITY.md like UniClipboard's explaining exactly what the server can see (metadata: timestamps, device IDs, sizes — not content).

## 4. Important features, complete list (prioritized)

P0 = v1 must-have, P1 = fast-follow, P2 = later.

| P | Feature | Notes |
| --- | --- | --- |
| P0 | Capture everything (text, link, image, file ref), dedup by content hash | Core pipeline |
| P0 | Quick picker: global hotkey → panel at cursor → type-to-search → Enter pastes & restores focus | Recopy technique; numbered hotkeys (Cmd/Ctrl+1-9) |
| P0 | Auto-categories (§5) + pin + sections | The differentiator |
| P0 | Search: FTS5 full-text + filters (category, date, device, source app) | |
| P0 | Paste-as-plain-text modifier | Universal ask, trivial to add |
| P0 | Sensitive handling: honor concealed flags, regex-detect OTP/keys/cards, **preset app-exclusion list** for password managers, per-app blocklist UI | tipsxBase's rules.rs is the blueprint |
| P0 | Retention/expiry rules (images expire on schedule; never-expire for pinned) | |
| P0 | E2EE sync + QR pairing + hosted relay + self-host binary | Both tracks |
| P1 | Timeline dashboard + calendar heatmap jump (§7) | The "browse back through time" promise |
| P1 | OCR on image clips → searchable screenshots | PowerToys/Deck normalized this; local OCR (Windows.Media.OCR / Vision framework / Tesseract) |
| P1 | Paste stack (FIFO sequential paste) | CopyQ #3348, Paste Stack, HN "godsend" |
| P1 | Named snippets / canned responses (graduates pinned clips) | Maccy's most-requested feature |
| P1 | Android: keyboard-based capture + tile + share + battery-onboarding | §8 |
| P1 | iOS: share ext + shortcut + keyboard + read-only web | |
| P1 | Merge/append clips; strip tracking params on copy | Small, beloved |
| P1 | Import from Ditto/CopyQ/Win+V db | Killer onboarding wedge — no other tool does migration |
| P2 | Semantic search (on-device embeddings, opt-in) | Deck proves demand; keep optional |
| P2 | Short-URL clip sharing (share a single clip via link) | clipper already has it |
| P2 | Shared pinboards / team spaces | Paste for Teams direction |
| P2 | CLI + Raycast extension | Power users live there (Maccy #1375 asks) |
| P2 | Linux Wayland + X11 capture, GNOME/KDE quirks | Budget real time; CopyQ's issue list is the cautionary tale |

## 5. Classifier — the higher-level separation you're asking about

You're right that this is easy-ish: rules catch ~90% before any ML. Design (validated by testing Magika today):

**Layer 1 — deterministic regex (sub-ms, ~always right):**
URL/domain, email, phone, hex color, file path, OTP shape (4-8 digits alone), card numbers, JWT/API-key/PEM patterns → Link, Contact/Data, Sensitive.

**Layer 2 — code structure heuristic:** multi-line + indentation + brace/semicolon/keyword density → Code vs Note. This alone separates code from prose reliably.

**Layer 3 — language naming, only if layer 2 said "code" AND length > ~80 chars.** I ran Magika on real short snippets today:

```
multi-line rust    → rust  (0.95)   ✓
multi-line python  → python (1.00)  ✓
sql                → sql    (1.00)  ✓
json               → jsonl  (0.77)  ✓ (map jsonl→json)
"let x = 5;"       → javascript (0.54) ✗ wrong + low conf
shell one-liner    → batch  (0.65)  ~ right family
URL/email/prose    → txt           ✓ (never reaches layer 3)
```

**Verdict:** Magika is reliable on multi-line code, unreliable on one-liners. Use it only above a length threshold, label everything else "code · unknown". Total classifier latency stays <5ms/clip, all local.

**Layer 4 — learned user rules.** When a user drags a clip between sections, write a `Rule` (source_app/domain/content-pattern → section) evaluated *before* layers 1-3 — tipsxBase/rules.rs is the exact shape. This is the moat: the app personalizes without a cloud model.

**Categories to ship (top level):** `link` (subkind=domain), `code` (subkind=lang), `note` (prose/lists), `contact` (email/phone/color), `file`/`image` (subkind=screenshot/path), `sensitive` (auto-skipped or auto-expiring). "Post"-type content (long social text) lands in `note` — don't over-fragment v1.

## 6. Search across time + calendar

Your instinct is right — this is cheap with the right data model:

- Every clip has `captured_at` (UTC). Grouping day→week→month is a SQL `GROUP BY strftime`, not a feature.
- **Timeline view:** infinite scroll, date headers, virtualized list (react-window pattern, ~20 DOM nodes at 10k items — Beetroot measured this).
- **Calendar heatmap:** one SQL query `SELECT date, count(*) GROUP BY date` → GitHub-style contribution grid; click a day → jump scroll position. Months of history render instantly.
- **Date scrubber:** ruler on the right edge, drag to teleport through time (Paste/iOS Photos pattern).
- FTS5 index covers text + OCR text + URL titles; filters: category, device, source app, date range.

## 7. How the app should look

Steal the proven mental models:

- **Picker (daily use, 95% of opens):** small floating panel at cursor/text caret. Left rail = section chips (All, Pinned, Links, Code, Notes, Images, Files, + your custom). Search box on top. Cards: 1-3 lines each — code = syntax-highlighted mono, links = favicon+title+domain, images = thumbnail, notes = first lines. Cmd+1..9 to quick-paste. Esc or click-away closes; Enter pastes and restores focus (Recopy's flow).
- **Dashboard (the wow surface):** three-pane — sections sidebar, timeline center with date scrubber + calendar popover, preview pane (full code render, full image, link metadata). Day/week summary strip on top (counts per category, top domains, pinned strip).
- **Card contents per type:** code → collapsed 3-line preview + expand + "copy again" + lang badge; link → favicon + page title + domain; image → thumb + dimensions + source path; sensitive → masked by default (`••••`) with reveal-on-hover.
- Themes light/dark from day one; density setting (comfortable/compact) — Maccy #1286 shows pinned-clutter is a real complaint.
- **Android:** keyboard shows a clipboard strip above keys (Syncer's trick — the keyboard IS the phone UI), plus QS tile + share sheet.
- **iOS/web:** read-only timeline + categories + search; share-sheet "Save to clips".

## 8. Technical landmines to know

1. **Android 10+ blocks background clipboard reads.** Only the focused app or the IME can read. Paths in order of reliability: (a) **own keyboard** — always allowed, doubles as paste UI (Syncer proves it ships on Play Store); (b) Accessibility Service + brief focus grab (ClipCascade's `ClipboardFloatingActivity` overlay trick — works but Play-review scrutiny + OEM battery killers); (c) ADB `READ_LOGS`+`SYSTEM_ALERT_WINDOW` (KDE Connect's community hack — dies after ~1-2h on some ROMs); (d) tile/share-target manual save (always works). Ship (d)+(a), offer (b) as APK/F-Droid extra.
2. **iOS will never auto-capture.** Design around it: share extension, Shortcut (SyncClipboard ships one), keyboard, widget, and "capture on app open". On iPad, Slide Over captures live.
3. **macOS paste-back** needs Accessibility permission + non-activating panel; **Windows** needs foreground-restore + SendInput with the settle-poll (Recopy code, §1). Wayland needs per-compositor work — X11 first.
4. **Dedup is subtle:** same content copied on two devices must merge (content_hash as dedup key) AND syncing a clip must not re-trigger the sender's watcher (echo suppression: last-write hash + quiet window — decentpaste's `set_last_hash`).
5. **Encrypt per-clip, not just at-rest:** the relay and the search problem both get easier if the searchable index (category, timestamp, hash, first-N-chars preview) stays locally derived while the body is ciphertext in transit. Decide what metadata is plaintext-on-relay: timestamp+device+size is defensible; content/type should be ciphertext.
6. **License decision gates distribution:** MIT core + MIT relay = anyone can self-host and fork (max goodwill, no moat). BSL/ELv2 = protects hosted business, hurts adoption. AGPL = SyncClipboard/UniClipboard's choice; fine for a free product, blocks a hosted-biz moat later. Recommendation: **MIT or Apache-2.0 for client+relay, keep the hosted control-plane (accounts/billing) closed** — same split as GitLab/Plausible.
7. **Play Store distribution:** Accessibility-based capture will get flagged; keyboard-based is a legit category. Plan F-Droid/GitHub APK first regardless (your doc is right).
8. **Migrations/importers** (Ditto SQLite, CopyQ, Win+V) are a cheap wedge nobody else offers — Ditto's schema is well-understood, read it read-only.

## 9. Ways this helps people (the "why" list for the pitch)

1. Never lose a copy — "what did I copy before that?" disappears forever.
2. Find the snippet/link/phone number from last Tuesday in seconds — code from phone usable on laptop.
3. Stop retyping canned text — snippets/templates with hotkeys replace TextExpander-lite.
4. Form-filling speedrun — copy 5 fields, paste-stack them in order (Excel/admin work).
5. Clean pastes — strip formatting/tracking junk without a detour through a text editor.
6. Screenshots become searchable text (OCR) — "the error dialog I screenshotted last week".
7. Password-manager safety — history that provably never records secrets (auditable, encrypted).
8. Developers: one self-hosted binary = encrypted clipboard fabric across laptop/desktop/phone/homelab, data never leaves your infra.
9. Cross-ecosystem households — Windows PC + iPhone + Android tablet finally share a clipboard.
10. Solo users AND teams — shared pinboards for snippets/prompts/links (Paste for Teams proved demand).

## 10. Recommended build order (revised)

1. **Weeks 1-2:** core crate (clip model, SQLite+FTS5, AES-GCM, rules engine à la tipsxBase), Windows capture daemon + CLI search; classifier measured on real history.
2. **Weeks 3-4:** Tauri picker + dashboard (timeline, lanes, search, paste-back). This is the make-or-break UX.
3. **Weeks 5-6:** crypto + axum relay (bundled AND standalone modes, clipper's pattern) + QR pairing + web dashboard. Ship self-host first — it's also the easiest to demo.
4. **Weeks 7-10:** Android keyboard + tile + share + battery onboarding.
5. **Then:** hosted control plane (accounts, device mgmt, billing) → iOS extensions → Linux/macOS polish → OCR/paste-stack/snippets.

The research report (`clipboard_market_research.md`) has the competitive table and source links for every claim above.
