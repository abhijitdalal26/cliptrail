# Roadmap

## M1 — MVP (the 3-day build)
- [ ] `clipd-core`: clip model, SQLite + FTS5, AES-GCM, capture pipeline, classifier, rules engine
- [ ] Desktop (Tauri 2): tray, hotkey picker with paste-back, timeline dashboard + category lanes + search
- [ ] `clipd-relay`: axum relay (ciphertext only), QR pairing, echo suppression, `docker-compose` self-host
- [ ] Read-only web dashboard

## M2 — Mobile capture
- [ ] Android: keyboard-based capture (IME), Quick Settings tile, share target, battery-onboarding flow
- [ ] iOS: share extension, Siri Shortcut, widget, keyboard
- [ ] Import from Ditto / CopyQ / Win+V history

## M3 — Power features
- [ ] OCR on image clips (searchable screenshots)
- [ ] Paste stack (sequential FIFO paste)
- [ ] Named snippets / canned responses
- [ ] Merge/append clips, paste-as-plain-text, tracking-param stripping

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
