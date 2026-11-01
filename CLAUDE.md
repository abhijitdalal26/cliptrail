@AGENTS.md

# Working notes for Claude

## Current decisions (these override older docs where they disagree)

- **Phase:** v1 = Windows laptop + Android phone. Make it work first, then polish. The desktop UI must look modern and beautiful; that is a core differentiator.
- **Branch:** commit to `main` directly while bootstrapping; push after each milestone.
- **Crates:** `clipd-core` (model, store, classifier, pipeline; no OS code), `clipd-capture` (desktop clipboard watcher, write-back, paste-back), `clipd-cli` (binary `clipd`). Contract types live in `crates/clipd-core/src/model.rs`.
- **Storage:** SQLite (rusqlite, bundled) + FTS5 trigram index for substring search. Images live as PNG files under the data dir (`images/`, `thumbs/`); the DB stores paths.
- **Encryption:** the local DB is plaintext during the prototype. Before sync ships: SQLCipher for the DB, XChaCha20-Poly1305 for sync envelopes and image files. One cipher family everywhere.
- **Categories** exist to tell the user what kind of data was copied and to drive how it's shown and pasted; they're not about splitting code from text for its own sake. Kinds: link, code, text, contact, color, file, image, sensitive.
- **Android capture:** no custom keyboard (user decision). Use an Accessibility Service that detects copy events and briefly takes focus to read the clipboard, a foreground service to stay alive, and a Quick Settings tile + share target as guaranteed manual paths. Ship as a sideloaded APK first (Play reviews Accessibility closely).
- **Sync for v1:** the desktop app hosts the sync endpoint on the LAN; the phone pairs with a QR code. A hosted relay comes later.

## Reference repos

Cloned (shallow) into `refs/`, which is gitignored and never pushed. See `refs/README.md`. Read any of them for ideas. MIT/Apache repos (clipper, tipsxBase/clipboard, SyncClipboard, decentpaste, magika) may be adapted with attribution. GPL/AGPL/PolyForm repos (ClipCascade, CopyQ, Ditto, tiez, UniClipboard, Qopy, Recopy) are ideas only: write our own code, never copy or line-by-line translate it.

## Commands

- `cargo test --workspace` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo fmt --all`
- `cargo run -p clipd-cli -- watch` (capture), `-- list`, `-- search <text>`, `-- stats`
- Set `CLIPTRAIL_DATA_DIR` to use a throwaway data dir when testing.
- **This Windows machine:** Visual Studio isn't registered, so rustc can't find `link.exe` on its own, and Git Bash's `/usr/bin/link.exe` shadows it. Run every cargo/tauri build through `scratch\msvc.cmd` from PowerShell, e.g. `& .\scratch\msvc.cmd cargo test --workspace`. (`scratch/` is gitignored and local.)
