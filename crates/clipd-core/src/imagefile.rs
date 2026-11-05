//! At-rest encryption of image and thumbnail files (`images/`, `thumbs/`).
//!
//! When the data dir is unlocked (see [`crate::dbkey::set_db_key`]) every
//! image file is written as `MAGIC || nonce(24) || XChaCha20-Poly1305(png)`,
//! with a key derived from the database key, so there is no second secret to
//! keep. [`read_image`] returns the PNG bytes for both encrypted files and
//! plaintext ones (older files, or a data dir without a key). Plaintext files
//! left from before encryption are encrypted by [`encrypt_existing`].
//!
//! The file name stays `<id>.png` so paths in the database don't change.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::dbkey;

/// Start of an encrypted image file. Never valid PNG (PNG starts 0x89 'P').
const MAGIC: &[u8; 8] = b"CTIMG\x00\x00\x01";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const KEY_INFO: &[u8] = b"cliptrail image files v1";

/// Image files live one level below the data dir (`<data>/images/<id>.png`).
fn data_dir_of(path: &Path) -> Option<&Path> {
    path.parent()?.parent()
}

fn file_key(path: &Path) -> Option<[u8; 32]> {
    let db_key = dbkey::key_for_dir(data_dir_of(path)?)?;
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(None, &db_key)
        .expand(KEY_INFO, &mut out)
        .expect("HKDF output length is small");
    Some(out)
}

/// True if `bytes` is an encrypted image file.
pub fn is_encrypted(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

fn seal(key: &[u8; 32], png: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| anyhow!("OS random number generator unavailable"))?;
    let ct = XChaCha20Poly1305::new(&Key::from(*key))
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: png,
                aad: MAGIC,
            },
        )
        .map_err(|_| anyhow!("encrypt image"))?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn open(key: &[u8; 32], file: &[u8]) -> Result<Vec<u8>> {
    let body = &file[MAGIC.len()..];
    if body.len() < NONCE_LEN + TAG_LEN {
        bail!("encrypted image is truncated");
    }
    let (nonce, ct) = body.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().expect("split at NONCE_LEN");
    XChaCha20Poly1305::new(&Key::from(*key))
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: ct,
                aad: MAGIC,
            },
        )
        .map_err(|_| anyhow!("image can't be decrypted (wrong key or damaged file)"))
}

/// The PNG bytes of an image file, decrypting it if needed.
pub fn read_image(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let path = path.as_ref();
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if !is_encrypted(&bytes) {
        return Ok(bytes);
    }
    let key =
        file_key(path).ok_or_else(|| anyhow!("image is encrypted but the data dir is locked"))?;
    open(&key, &bytes)
}

/// Write PNG bytes to `path` (encrypted when the data dir is unlocked),
/// through `<path>.tmp` + rename so readers never see a half-written file.
pub fn write_image(path: impl AsRef<Path>, png: &[u8]) -> Result<()> {
    let path = path.as_ref();
    let sealed;
    let bytes = match file_key(path) {
        Some(key) => {
            sealed = seal(&key, png)?;
            &sealed[..]
        }
        None => png,
    };
    // Unique temp name: two processes may migrate the same file at once.
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", ulid::Ulid::generate()));
    let tmp = PathBuf::from(tmp);
    let written = fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.with_context(|| format!("write {}", path.display()))
}

/// Encrypt plaintext image files left in `data_dir` from before encryption.
/// No-op while the data dir is locked. Returns how many files were encrypted.
pub fn encrypt_existing(data_dir: &Path, dirs: &[&str]) -> usize {
    if !dbkey::has_db_key(data_dir) {
        return 0;
    }
    let mut done = 0;
    for d in dirs {
        let Ok(entries) = fs::read_dir(data_dir.join(d)) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "png") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            if is_encrypted(&bytes) {
                continue;
            }
            match write_image(&path, &bytes) {
                Ok(()) => done += 1,
                Err(e) => tracing::warn!(error = %e, "could not encrypt an image file"),
            }
        }
    }
    if done > 0 {
        tracing::info!(files = done, "encrypted existing image files");
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("clipd-imgfile-{tag}-{}", ulid::Ulid::generate()));
            fs::create_dir_all(dir.join("images")).unwrap();
            fs::create_dir_all(dir.join("thumbs")).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really a png but fine";

    #[test]
    fn plaintext_without_key() {
        let d = TempDir::new("plain");
        let p = d.0.join("images/a.png");
        write_image(&p, PNG).unwrap();
        assert_eq!(fs::read(&p).unwrap(), PNG);
        assert_eq!(read_image(&p).unwrap(), PNG);
        assert_eq!(encrypt_existing(&d.0, &["images"]), 0);
    }

    #[test]
    fn encrypted_with_key_and_migrates_old_files() {
        let d = TempDir::new("enc");
        let old = d.0.join("thumbs/old.png");
        fs::write(&old, PNG).unwrap();
        fs::write(d.0.join("thumbs/notes.txt"), b"skip me").unwrap();

        dbkey::set_db_key(&d.0, [7u8; 32]);
        let p = d.0.join("images/a.png");
        write_image(&p, PNG).unwrap();
        let on_disk = fs::read(&p).unwrap();
        assert!(is_encrypted(&on_disk));
        assert!(!on_disk.windows(4).any(|w| w == b"PNG\r"));
        assert_eq!(read_image(&p).unwrap(), PNG);

        // Old plaintext still reads, then gets encrypted once.
        assert_eq!(read_image(&old).unwrap(), PNG);
        assert_eq!(encrypt_existing(&d.0, &["images", "thumbs"]), 1);
        assert!(is_encrypted(&fs::read(&old).unwrap()));
        assert_eq!(read_image(&old).unwrap(), PNG);
        assert_eq!(encrypt_existing(&d.0, &["images", "thumbs"]), 0);
        assert_eq!(fs::read(d.0.join("thumbs/notes.txt")).unwrap(), b"skip me");

        // Wrong key or tampering fails instead of returning garbage.
        let mut bad = on_disk.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        fs::write(&p, &bad).unwrap();
        assert!(read_image(&p).is_err());
        fs::write(&p, &on_disk).unwrap();
        dbkey::set_db_key(&d.0, [8u8; 32]);
        assert!(read_image(&p).is_err());
    }
}
