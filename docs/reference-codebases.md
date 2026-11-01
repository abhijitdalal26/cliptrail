# Reference codebases

External open-source projects worth studying before writing our own code. Read them to understand approaches; **respect the license column** — only MIT/Apache code may be adapted, GPL/AGPL/PolyForm is read-for-ideas-only and must be reimplemented.

Legend: ✅ adaptable · 📖 ideas only (copyleft / non-commercial)

## Tier 1 — study first

| Repo | License | Stack | What it teaches us | Key files |
| --- | --- | --- | --- | --- |
| [windoze/clipper](https://github.com/windoze/clipper) | MIT ✅ | Rust workspace · Axum · Tauri 2 + React | Overall architecture: indexer crate (SurrealDB+RocksDB+BM25 FTS), REST+WS server with built-in web UI, bundled-vs-external server modes, bearer auth, clip sharing via short URLs, CLI. Closest model to our product. | `clipper-indexer/`, `clipper-server/src/{api,websocket,auth}.rs`, `clipper-client/` |
| [tipsxBase/clipboard](https://github.com/tipsxBase/clipboard) | MIT ✅ | Tauri + Rust | The rules engine to adapt for our classifier corrections: conditions on source_app/content_type/content-regex → actions ignore/mark_sensitive/pin/snippet/collection. Preset password-manager exclusions. AES-GCM DB encryption. | `src-tauri/src/rules.rs`, `src-tauri/src/crypto.rs` |
| [decentpaste](https://github.com/decentpaste/decentpaste) | Apache-2.0 ✅ | Tauri v2 · libp2p · RustCrypto | Tauri plugin pattern for mobile (share target in Kotlin, secure storage via Keychain/AndroidKeyStore). X25519 ECDH pairing → AES-256-GCM, Argon2id fallback. Full `ARCHITECTURE.md` on disk. | `ARCHITECTURE.md`, `decentpaste-app/tauri-plugin-decentshare/`, `tauri-plugin-decentsecret/` |
| [Jeric-X/SyncClipboard](https://github.com/Jeric-X/SyncClipboard) | MIT ✅ | .NET/WinUI3 (not Rust) | Concept: flexible relay model — built-in server, standalone Docker, or WebDAV/S3 as relay. Image transcoding on copy (heic/webp→jpg). History API shape. | `README_EN.md` (API section), `docs/` |

## Tier 2 — read for ideas only

| Repo | License | Stack | What it teaches us | Key files |
| --- | --- | --- | --- | --- |
| [UniClipboard/UniClipboard](https://github.com/UniClipboard/UniClipboard) | AGPL-3.0 📖 | Rust workspace · iroh P2P · daemon + quick-panel | Pairing UX (invite code + passphrase → "Space"), MasterKey model, daemon/GUI split, headless server node (ADR-007), mobile companions. | `CONTEXT.md` (domain glossary), `crates/`, `apps/`, `docs/architecture/` |
| [darkingtail/Recopy](https://github.com/darkingtail/Recopy) | PolyForm Noncommercial 📖 | Tauri v2 + React | Paste-back recipe: store previous foreground HWND → `SetForegroundWindow` → poll ≤10ms for settle → `SendInput` Ctrl+V. macOS: `NonactivatingPanel` + `resign_key_window`. | `src-tauri/src/platform/windows.rs`, `macos.rs`, `nspanel.rs` |
| [Sathvik-Rao/ClipCascade](https://github.com/Sathvik-Rao/ClipCascade) | GPL-3.0 📖 | Java server · RN mobile | Android capture hacks: `ClipboardFloatingActivity` + TYPE_APPLICATION_OVERLAY focus trick, logcat READ_LOGS fallback, BootReceiver + foreground service. | `ClipCascade_Mobile/.../ClipboardFloatingActivity.kt`, `ClipboardListenerModule.kt` |
| [hluk/CopyQ](https://github.com/hluk/CopyQ) | GPL-3.0 📖 | C++/Qt | Tabs/tags/scripting model; the feature-request backlog is our wishlist source (sequential paste, cross-tab search). | issues tracker |
| [sabrogden/Ditto](https://github.com/sabrogden/Ditto) | GPL-3.0 📖 | C++/SQLite | Groups UX (Ctrl+G hides groups from main list), history caps, SQLite schema — needed if we build a Ditto importer. | SQLite schema, `Groups` handling |
| [0PandaDEV/Qopy](https://github.com/0PandaDEV/Qopy) | AGPL-3.0 📖 | Tauri + Nuxt | Minimal clean capture/settings structure. | `src-tauri/` |
| [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard) | GPL-3.0 📖 | Tauri | WebDAV/MQTT sync, privacy masking of IDs/phones. | `src-tauri/` |

## Tier 3 — adjacent products worth knowing

| Product | Type | Why it matters |
| --- | --- | --- |
| [google/magika](https://github.com/google/magika) | Apache-2.0 library | Code-language detection for our classifier layer 3 — tested: reliable on multi-line code only, threshold it |
| [syncerclip.com](https://syncerclip.com) | Closed product | Proves the keyboard-IME capture path ships on Play Store; keyboard doubles as phone paste UI |
| [pastefrog.app](https://pastefrog.app) | Closed product | E2EE clipboard SaaS pricing model ($1.99/mo, 10 devices, 25GB) |
| [deckclip.app](https://deckclip.app) | Closed product | On-device semantic search + OCR — our P2/P3 reference |
| [pasteapp.io](https://pasteapp.io) | Closed product | The UX bar: pinboards, paste stack, iOS share-ext/keyboard pattern |
| [sniptray.com](https://sniptray.com) | Closed product | The 5 sensitive-clip rules users now expect as table stakes |
| KDE Connect / Phone Link / Universal Clipboard | OS-level | The "last clip" baseline we must beat with history + organization |

## Rule for the agent

- ✅ MIT/Apache code may be adapted with attribution.
- 📖 read for design and reimplement — do not copy code or structure verbatim; if you do, the whole project inherits their license (GPL/AGPL) or loses commercial rights (PolyForm).
- When unsure whether a pattern is "inspired" vs "copied", prefer rewriting from the design note + asking in the PR description.
