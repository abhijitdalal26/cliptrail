@AGENTS.md

# Working notes for Claude

## Current decisions (these override older docs where they disagree)

- **Phase:** v1 = Windows laptop + Android phone. Make it work first, then polish. The desktop UI must look modern and beautiful; that is a core differentiator.
- **Branch:** `main` is protected (PR required + 5 CI checks: Rust ubuntu/windows/macos, Linux live clipboard tests, Relay Docker image); work on a branch and merge via PR.
- **Crates:** `clipd-core` (model, store, classifier, pipeline; no OS code), `clipd-capture` (desktop clipboard watcher, write-back, paste-back), `clipd-cli` (binary `clipd`). Contract types live in `crates/clipd-core/src/model.rs`.
- **Storage:** SQLite (rusqlite, bundled) + FTS5 trigram index for substring search. Images live as PNG files under the data dir (`images/`, `thumbs/`); the DB stores paths.
- **Encryption:** local DBs (`clips.db`, `sync.db`) are encrypted with SQLite3 Multiple Ciphers (ChaCha20-Poly1305) through the patched `third_party/libsqlite3-sys`, not SQLCipher (no OpenSSL); key `db-key` in the keystore, see `docs/adr/0002-local-db-encryption.md`. Every process must call `clipd_sync::unlock_data_dir` before opening the pipeline. XChaCha20-Poly1305 for sync envelopes and (next) image files. One cipher family everywhere.
- **Categories** exist to tell the user what kind of data was copied and to drive how it's shown and pasted; they're not about splitting code from text for its own sake. Kinds: link, code, text, contact, color, file, image, sensitive.
- **Android capture (user, 2026-11-21):** no Accessibility service and no custom keyboard. Sideloaded APKs that declare an accessibility service are blocked at install by Play Protect's fraud protection in India ("can request sensitive data… financial fraud"), so the app only reads the clipboard when the user asks: notification "Save clipboard" button, Quick Settings tile, share target, text-selection menu, opening the app. A foreground service keeps the notification and sync alive. Desktop → phone clips arrive in the phone history in the background; the user taps one to copy it. Ships as a sideloaded APK. Test phone: Samsung Galaxy S23 FE (One UI, arm64). Older plan: `docs/android-plan.md`.
- **Product principle (user, 2026-11-01):** cross-platform AND cross-device. Each OS gets its own app (desktop Tauri for Windows/Linux/macOS, Android app, iOS later), all on the same Rust core. Any device ↔ any device: copy on Windows → paste on Android, image on Android → macOS, code on Linux → Windows. Sync is optional — every app must be fully useful alone on one device; pairing only adds devices when the user has them.
- **Sync for v1:** `crates/clipd-sync`. A desktop app hosts the hub on the LAN; other devices (phone, or another desktop) pair by QR. Envelopes are storage-agnostic so a self-hostable/hosted relay can be added later for "laptop is off" and over-the-internet sync.

- **AI assistants (MCP), v0.1.7 (user, 2026-11-22):** the MCP server is built into the desktop app (`cliptrail --mcp`, handled in `main.rs` before Tauri starts; `clipd-mcp` is a library + standalone binary). Settings → AI assistants: one switch connects every installed AI app; "Choose apps" for per-app; "Copy config" for others. Installed = the app's program exists (install folder / macOS bundle / command on PATH), never data folders (they outlive uninstalls and Connect writes into some). Claude Desktop's real config can live in `%LOCALAPPDATA%\Claude-Data\<package>\` (MSIX). Connect self-tests the server first; `mcp-clients.json` in the data dir records which assistant last started it ("last used" in Settings). Desktop only; phone clips are covered through sync. "Sync anywhere" (relay) UI is hidden (`SHOW_RELAY`).
- **Name is final: ClipTrail (user, 2026-11-02).** Public releases and package-manager manifests are unblocked.
- **Off-LAN sync (user, 2026-11-01):** build sync that works when devices are on different networks; whether it ships in v1 is decided later.

## Reference repos

Cloned (shallow) into `refs/`, which is gitignored and never pushed. See `refs/README.md`. Read any of them for ideas. MIT/Apache repos (clipper, tipsxBase/clipboard, SyncClipboard, decentpaste, magika) may be adapted with attribution. GPL/AGPL/PolyForm repos (ClipCascade, CopyQ, Ditto, tiez, UniClipboard, Qopy, Recopy) are ideas only: write our own code, never copy or line-by-line translate it.

## Commands

- `cargo test --workspace` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo fmt --all`
- `cargo run -p clipd-cli -- watch` (capture), `-- list`, `-- search <text>`, `-- stats`
- Set `CLIPTRAIL_DATA_DIR` to use a throwaway data dir when testing.
- **This Windows machine:** Visual Studio isn't registered, so rustc can't find `link.exe` on its own, and Git Bash's `/usr/bin/link.exe` shadows it. Run every cargo/tauri build through `scratch\msvc.cmd` from PowerShell, e.g. `& .\scratch\msvc.cmd cargo test --workspace`. (`scratch/` is gitignored and local.)
