# Roadmap

## M1 — MVP (the 3-day build)
- [x] `clipd-core`: clip model, SQLite + FTS5, capture pipeline, classifier, type-aware transforms
- [x] `clipd-capture`: Windows watcher + paste-back; Linux X11/Wayland (CI-checked)
- [x] `clipd` CLI: watch, list, search, show, stats, pin, delete
- [x] Desktop (Tauri 2): tray, hotkey picker with paste-back, timeline dashboard + category lanes + search, settings
- [ ] Encrypted sync: desktop hosts on the LAN, QR pairing, echo suppression (in progress)
- [ ] Local database encryption at rest
- [ ] Release installers (workflow ready; first tag at v0.1.0)
- [ ] Read-only web dashboard

## M2 — Mobile capture
- [ ] Android: accessibility-triggered capture (no custom keyboard), Quick Settings tile, share target, battery onboarding — capture spike built, awaiting on-device test
- [ ] iOS: share extension, Siri Shortcut, widget, keyboard
- [ ] Import from Ditto / CopyQ / Win+V history

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
