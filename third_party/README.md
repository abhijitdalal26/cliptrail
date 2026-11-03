# third_party

Patched copies of upstream crates, wired in with `[patch.crates-io]` in the
workspace `Cargo.toml`. Excluded from the workspace, so `cargo clippy/test
--workspace` skips them.

## libsqlite3-sys (0.39.1, MIT, the rusqlite developers)

Why: the local databases are encrypted at rest (see
`docs/adr/0002-local-db-encryption.md`). Upstream `libsqlite3-sys` only
offers SQLCipher, which needs OpenSSL (Perl + a C toolchain to build it on
Windows, a cross build for Android). SQLite3 Multiple Ciphers needs nothing
beyond the C compiler we already use, and its default cipher is
ChaCha20-Poly1305 — the same family as our sync envelopes.

What changed from upstream (everything else is byte-for-byte):

- `build.rs`: the `bundled` build compiles `sqlite3mc_amalgamation.c` from the
  `sqlite3mc-src` crate instead of `sqlite3/sqlite3.c`; adds
  `SQLITE_TEMP_STORE=2` (temp data stays in memory) and links `bcrypt` /
  `advapi32` on Windows (OS RNG); drops the `rerun-if-changed` on a WASI file
  we don't ship.
- `Cargo.toml`: build-dependency `sqlite3mc-src = "=206.1.0"` (SQLite3MC
  2.6.1, SQLite 3.54.0); header comment.
- Removed files we never build: `sqlite3/sqlite3.c`, `sqlite3/sqlite3.h`, the
  `sqlcipher/` dir, the `*_ext.rs` loadable-extension bindings.

The committed bindings (`sqlite3/bindgen_bundled_version.rs`) are upstream's
for SQLite 3.54.1; 3.54.0 has the same API.

### Upgrading

1. Copy the new `libsqlite3-sys` release from `~/.cargo/registry/src/*/`.
2. Reapply the `build.rs` / `Cargo.toml` changes above (search for
   "ClipTrail").
3. Pick the `sqlite3mc-src` release that wraps the same SQLite version as
   upstream's bundled one, and pin it with `=`.
4. `cargo test --workspace` (the `clipd_core::dbkey` tests cover open,
   migrate and wrong-key).
