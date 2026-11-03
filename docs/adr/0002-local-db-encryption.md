# ADR 0002: Encrypting the local databases

- Status: accepted, implemented (`clipd_core::dbkey`, `clipd_sync::unlock_data_dir`, `third_party/libsqlite3-sys`)
- Date: 2026-11-03

## Context

Clip history is the most sensitive data ClipTrail holds. Until now
`clips.db` and `sync.db` were plaintext SQLite files in the data dir, so a
stolen unencrypted disk, a backup of `%APPDATA%`, or another user on the
machine could read every clip. `CLAUDE.md` said: SQLCipher for the DB before
sync ships, one cipher family (XChaCha20-Poly1305) everywhere.

## Decision

Use **SQLite3 Multiple Ciphers** (MIT) in place of SQLCipher, with its
ChaCha20-Poly1305 cipher, via a patched copy of `libsqlite3-sys`
(`third_party/`).

| | SQLCipher | SQLite3 Multiple Ciphers |
| --- | --- | --- |
| Crypto backend | OpenSSL (CommonCrypto on Apple) | built in, no dependency |
| Windows build | OpenSSL from source: needs Strawberry Perl + NASM | the existing C compiler |
| Android build | cross-build OpenSSL with the NDK from Windows | the existing NDK clang |
| Cipher | AES-256-CBC + HMAC-SHA512 | ChaCha20-Poly1305 (same family as sync) |
| rusqlite support | upstream feature | one-file build-script patch |

Cost: we carry a small patch on `libsqlite3-sys` and must reapply it on
upgrades (`third_party/README.md`).

## How it works

- **Key**: 32 random bytes per device, entry `db-key` in the same keystore as
  the sync keys (Windows Credential Manager, macOS Keychain; a private file on
  Linux and in Android's no-backup storage).
- **Unlock**: each process (desktop app, Android app, `clipd`, `clipd-mcp`)
  calls `clipd_sync::unlock_data_dir` once; every database later opened under
  that data dir through `clipd_core::dbkey::open_connection` is keyed. The
  relay's database is not keyed (it only stores ciphertext already).
- **Migration**: a plaintext database is encrypted in place on first keyed
  open (WAL folded back, `VACUUM`, `PRAGMA rekey`, verified). If that fails
  (e.g. another process holds it) the database opens unencrypted and the next
  open retries.
- **Safety**: a key is only created while no database in the dir is encrypted.
  An encrypted database with no key, or the wrong one, is an error; it is
  never overwritten or recreated.

## Not covered yet

- Image and thumbnail PNGs in `images/` and `thumbs/` are still plaintext
  (next step: XChaCha20-Poly1305 files + a decrypting asset protocol).
- Linux and Android keep the key in a file next to the data; Secret Service
  and Android Keystore wrapping come later.
- Old plaintext may survive in free disk blocks after migration; only full-disk
  encryption fixes that.
- Losing the keychain entry (profile reset, manual cleanup) makes the local
  history unreadable. Synced devices still hold their copies.
