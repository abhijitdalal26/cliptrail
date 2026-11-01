# Notes for coding agents

## What this is

ClipTrail: an open-source, end-to-end-encrypted clipboard history that syncs across Windows, macOS, Linux and Android. Rust core shared by all clients; Tauri desktop; Kotlin/Swift shells where the OS forces us.

## Read first

1. `docs/product-spec.md` — the product spec (goals, data model, classifier, dashboard)
2. `docs/system-design.md` — the architecture doc: components, pipeline, data model, sync, crypto, platform matrix
3. `docs/technical-blueprint.md` — tech decisions, feature priorities, UI design, landmines
4. `docs/build-plan.md` — the 3-day MVP plan and this repo's intended layout
5. `docs/market-research.md` — competitive research backing the above
6. `docs/reference-codebases.md` — external repos to study, with license rules per repo

## Hard rules

- **License discipline:** MIT/Apache/BSD dependencies only (`cargo deny` enforces). Reference repos marked 📖 (GPL/AGPL/PolyForm) are read-for-ideas — reimplement, never copy.
- **No telemetry, no analytics, no phone-home.** The one exception is the desktop update check (GitHub Releases `latest.json`): it must stay opt-in (off until the user chooses), send nothing about the user, and never install without confirmation.
- **Secrets handling:** clip content is toxic data — never log clip bodies, never send plaintext to the relay, encrypt before persisting. Password managers are excluded by default.
- Commits on `main` stay releasable; new work on `devin/*` or feature branches with PRs.

## Intended structure (scaffolded in the 3-day plan)

```
crates/clipd-core     model · SQLite+FTS5 · AES-GCM · classifier · rules · sync protocol
crates/clipd-relay    axum WS+REST relay, ciphertext only
crates/clipd-cli      search/import/export CLI
crates/clipd-uniffi   mobile bindings
apps/desktop          Tauri 2 app
apps/android          Kotlin shell (IME + tile + share target)
apps/web              read-only dashboard
docker/               self-host one-liner
```
