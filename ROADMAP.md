# Roadmap

## M1 — MVP (the 3-day build)
- [x] `clipd-core`: clip model, SQLite + FTS5, capture pipeline, classifier, type-aware transforms
- [x] `clipd-capture`: Windows watcher + paste-back; Linux X11/Wayland (CI-checked)
- [x] `clipd` CLI: watch, list, search, show, stats, pin, delete
- [x] Desktop (Tauri 2): tray, hotkey picker with paste-back, timeline dashboard + category lanes + search, settings
- [x] Encrypted sync: desktop hosts on the LAN, QR pairing, echo suppression, phone reconnects after the laptop's address changes
- [x] Local database encryption at rest (SQLite3 Multiple Ciphers, ADR 0002; images encrypted too)
- [x] Release installers (shipping since v0.1.0; latest v0.1.8)
- [ ] Read-only web dashboard

## M2 — Mobile capture
- [x] Android: tap-to-capture (no accessibility service, no custom keyboard): notification button, Quick Settings tile, share target, selection menu; sideloaded APK, tested on Galaxy S23 FE
- [ ] iOS: share extension, Siri Shortcut, widget, keyboard
- [ ] Import from Ditto / CopyQ / Win+V history

## Also shipped
- [x] Collections (your own folders, synced on desktop and Android)
- [x] AI assistants: MCP server built into the desktop app (v0.1.7)

## M3 — Power features
- [ ] OCR on image clips (searchable screenshots)
- [ ] Paste stack (sequential FIFO paste)
- [ ] Named snippets / canned responses
- [x] Paste-as-plain-text, tracking-param stripping, color/JSON/code transforms
- [ ] Merge/append clips

## M4 — Hosted service
- [ ] Hosted relay + accounts (identity only — keys stay on device)
- [ ] Device management, free/paid tiers
- [ ] Web dashboard v2

## Later / exploring
- [ ] Semantic search (on-device embeddings, opt-in)
- [ ] Short-URL clip sharing
- [ ] Shared pinboards / team spaces
- [ ] CLI + Raycast extension
- [ ] Linux Wayland polish
- [ ] Windows code signing via SignPath (deferred on purpose until the project has a following; releases ship unsigned and SmartScreen may warn. See docs/code-signing.md)
