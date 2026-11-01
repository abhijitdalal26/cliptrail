# Clipboard app — 3-day build plan + open-source structure

Companion to `clipboard_build_blueprint.md` (tech decisions) and `clipboard_market_research.md` (market research).

## Open source — yes, here's how

Recommended model: **dual MIT/Apache-2.0** on the client, core, relay, and CLI (standard Rust convention — users can build on it freely, you keep a trademark). The **hosted control plane** (accounts, billing, device management) lives in a *separate private repo* later — same split GitLab/Plausible use. Everything self-hostable stays public and free.

Why open source wins here specifically:
- A clipboard app sees passwords — "audit the code, self-host the relay" is the only pitch privacy users trust.
- Every fast-growing player in this space (SyncClipboard 5k stars, CopyQ, Ditto, Maccy) is open source. Closed-source entrants (Paste, Pastefrog) eat subscription backlash.
- Your moat is the hosted service + product polish, not the code.

### GitHub repo structure (monorepo)

```
clipd/                              # pick a real name, check crates.io + domains early
├── Cargo.toml                      # workspace root
├── crates/
│   ├── clipd-core/                 # clip model, SQLite+FTS5, AES-GCM, classifier, rules engine, sync protocol
│   ├── clipd-relay/                # axum WS+REST relay, ciphertext-only store, device pairing
│   ├── clipd-cli/                  # search/import/export over the local DB (also our debugging tool)
│   └── clipd-uniffi/               # UniFFI bindings for Android/iOS shells
├── apps/
│   ├── desktop/                    # Tauri 2: tray, picker, dashboard (Win/Mac/Linux)
│   ├── android/                    # Kotlin shell: IME keyboard, tile, share target
│   ├── ios/                        # share ext + shortcut + keyboard (later)
│   └── web/                        # read-only dashboard (shared frontend build)
├── docker/
│   ├── Dockerfile.relay
│   └── docker-compose.yml          # self-host one-liner
├── docs/
│   ├── architecture.md             # pipeline, crates, data model
│   ├── security-model.md           # what the relay can/can't see
│   ├── sync-protocol.md
│   └── android-capture.md          # the 4 capture paths and why keyboard-first
├── .github/
│   ├── workflows/
│   │   ├── ci.yml                  # fmt, clippy, cargo test, frontend lint/build
│   │   └── release.yml             # tag → Tauri bundles per OS + relay docker image + crates publish
│   ├── ISSUE_TEMPLATE/{bug,feature}.yml
│   ├── PULL_REQUEST_TEMPLATE.md
│   ├── FUNDING.yml
│   └── CODEOWNERS
├── LICENSE-MIT / LICENSE-APACHE    # or single LICENSE for MIT
├── README.md                       # 30-sec demo GIF, install, self-host one-liner, feature matrix
├── CONTRIBUTING.md                 # dev setup, architecture tour, "good first issues"
├── CODE_OF_CONDUCT.md
├── SECURITY.md                     # threat model + vuln reporting (clipboard app MUST have this)
├── CHANGELOG.md                    # git-cliff or release-please generated
├── rust-toolchain.toml
├── deny.toml                       # cargo-deny license+advisory gate in CI
└── .gitignore
```

Community plumbing that costs nothing and pays off: enable Discussions, `good first issue` labels, a public `ROADMAP.md` (or GitHub Projects), issue templates, and a `docs/architecture.md` written for contributors — it's also the doc your coding agent reads first.

## 3-day build plan (MVP you can actually use)

Honest scope: day-3 gets you **capture + organized history + dashboard + paste-back + E2EE sync between two desktops + self-hosted relay**. Android keyboard, iOS, and the hosted account system are the next milestone after this.

### Day 1 — repo + core crate
- Scaffold the monorepo above; CI green on empty crates.
- `clipd-core`: clip model (ULID id, captured_at, device_id, source_app, kind/subkind, content_hash, text, thumb, file_path, section, pinned, sensitive, expires_at), SQLite schema + FTS5 index, AES-GCM field encryption.
- Pipeline: normalize → hash → dedup → classify → persist → notify.
- Classifier layers 1-2 (regex + code heuristic) + rules engine (tipsxBase shape). Magika wired but gated behind length threshold.
- **Done when:** `clipd-cli import` a real Ditto/CopyQ export, `clipd-cli search` returns categorized results with timing.

### Day 2 — desktop app
- Tauri 2 scaffold: tray icon, autostart, capture watcher (arboard or clipboard-rs, 250ms poll + hash check).
- **Picker**: global hotkey → panel at cursor → search/type → Enter pastes (Windows: foreground-restore + SendInput per Recopy; macOS: non-activating panel).
- **Dashboard**: timeline with date headers, category lane filter chips, section sidebar, search box, per-type cards, pin/delete/move-to-section, light+dark.
- **Done when:** you use it for real copying all afternoon without losing anything.

### Day 3 — relay + sync
- `clipd-relay`: axum — `POST /events` append, `GET /events?since=` catch-up, `/ws` live push, `POST /pair` device registration. Stores ciphertext only.
- Pairing: QR shows device pubkey + space passphrase; X25519 → shared MasterKey (UniClipboard's model).
- Echo suppression (content_hash + last-write quiet window).
- `docker-compose.yml` up on a VPS or laptop #2; two machines sync live; web dashboard served read-only by the relay.
- **Done when:** copy on machine A → appears in B's picker within ~1s, and the relay logs only ciphertext.

### Deferred (post-day-3, in priority order)
Android keyboard → Android tile/share + battery onboarding → hosted control plane (accounts, device mgmt, billing) → iOS extensions → OCR → paste stack → snippets → Ditto/CopyQ importer UI → semantic search.

## First-week open-source checklist
1. Name + handles check (crates.io, npm scope, GitHub org, domain) — decide before day 1, renaming later is painful.
2. LICENSE files + `deny.toml` license gate in CI so no GPL dep sneaks in.
3. SECURITY.md before public launch — a clipboard app without a stated threat model gets shredded on HN.
4. Sign up for nothing: no telemetry, no analytics — say so in the README (it's a feature).
5. README demo GIF is the #1 adoption driver — record the picker + dashboard on day 3.
