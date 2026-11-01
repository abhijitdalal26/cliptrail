//! Primitives: XChaCha20-Poly1305 (AEAD), X25519 (pairing), HKDF-SHA256,
//! HMAC-SHA256. One cipher family everywhere. Keys never appear in logs,
//! errors or `Debug` output.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    /// Wrong key, wrong context (AAD) or tampered ciphertext.
    #[error("decryption failed (wrong key or tampered data)")]
    Open,
    #[error("encryption failed")]
    Seal,
    #[error("invalid key material")]
    BadKey,
    #[error("invalid encoding")]
    Encoding,
}

/// Cryptographically secure random bytes from the OS.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).expect("OS random number generator unavailable");
    out
}

/// A 32-byte secret key that wipes itself on drop and never prints.
#[derive(Clone, Zeroize, ZeroizeOnDrop, PartialEq, Eq)]
pub struct SecretKey([u8; KEY_LEN]);

impl SecretKey {
    pub fn generate() -> SecretKey {
        SecretKey(random_bytes())
    }

    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> SecretKey {
        SecretKey(bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<SecretKey, CryptoError> {
        let arr: [u8; KEY_LEN] = bytes.try_into().map_err(|_| CryptoError::BadKey)?;
        Ok(SecretKey(arr))
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// Domain-separated subkey: HKDF-SHA256(ikm = self, info = label).
    pub fn derive(&self, label: &str) -> SecretKey {
        let mut out = [0u8; KEY_LEN];
        hkdf_sha256(&self.0, None, label.as_bytes(), &mut out);
        SecretKey(out)
    }

    /// AEAD-encrypt `plaintext` bound to `aad`. Fresh random 24-byte nonce.
    pub fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Sealed, CryptoError> {
        let nonce: [u8; NONCE_LEN] = random_bytes();
        let ct = self
            .cipher()
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::Seal)?;
        Ok(Sealed { nonce, ct })
    }

    pub fn open(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        ct: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.cipher()
            .decrypt(&XNonce::from(*nonce), Payload { msg: ct, aad })
            .map_err(|_| CryptoError::Open)
    }

    /// `nonce || ciphertext` in one buffer (blobs, wrapped keys).
    pub fn seal_packed(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let s = self.seal(aad, plaintext)?;
        let mut out = Vec::with_capacity(NONCE_LEN + s.ct.len());
        out.extend_from_slice(&s.nonce);
        out.extend_from_slice(&s.ct);
        Ok(out)
    }

    pub fn open_packed(&self, aad: &[u8], packed: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if packed.len() < NONCE_LEN + TAG_LEN {
            return Err(CryptoError::Open);
        }
        let (nonce, ct) = packed.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| CryptoError::Open)?;
        self.open(&nonce, aad, ct)
    }

    /// HMAC-SHA256 keyed with this key.
    pub fn mac(&self, parts: &[&[u8]]) -> [u8; 32] {
        hmac_sha256(&self.0, parts)
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(&Key::from(self.0))
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

/// Output of [`SecretKey::seal`].
#[derive(Debug, Clone)]
pub struct Sealed {
    pub nonce: [u8; NONCE_LEN],
    pub ct: Vec<u8>,
}

/// Unambiguous associated data: each part is length-prefixed.
pub fn aad(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len() + 4).sum());
    for p in parts {
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(p);
    }
    out
}

/// HMAC-SHA256 over length-prefixed `parts`.
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key size");
    mac.update(&aad(parts));
    mac.finalize().into_bytes().into()
}

/// Constant-time check of an HMAC-SHA256 tag.
pub fn hmac_verify(key: &[u8], parts: &[&[u8]], tag: &[u8]) -> bool {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key size");
    mac.update(&aad(parts));
    mac.verify_slice(tag).is_ok()
}

pub fn hkdf_sha256(ikm: &[u8], salt: Option<&[u8]>, info: &[u8], out: &mut [u8]) {
    Hkdf::<Sha256>::new(salt, ikm)
        .expand(info, out)
        .expect("HKDF output length is small");
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unb64(s: &str) -> Result<Vec<u8>, CryptoError> {
    URL_SAFE_NO_PAD
        .decode(s.trim().trim_end_matches('='))
        .map_err(|_| CryptoError::Encoding)
}

pub fn unb64_array<const N: usize>(s: &str) -> Result<[u8; N], CryptoError> {
    unb64(s)?.try_into().map_err(|_| CryptoError::Encoding)
}

/// Keys derived from a space's MasterKey, one per purpose.
#[derive(Clone)]
pub struct SpaceKeys {
    master: SecretKey,
    /// Seals event envelopes.
    pub events: SecretKey,
    /// Seals image blobs.
    pub blobs: SecretKey,
    /// Keyed tags (blob ids, hub compaction tags) so a relay never sees
    /// content hashes.
    pub tags: SecretKey,
    /// Wraps device tokens at rest on the hub.
    pub wrap: SecretKey,
}

impl SpaceKeys {
    pub fn new(master: SecretKey) -> SpaceKeys {
        SpaceKeys {
            events: master.derive("cliptrail-events-v1"),
            blobs: master.derive("cliptrail-blobs-v1"),
            tags: master.derive("cliptrail-tags-v1"),
            wrap: master.derive("cliptrail-wrap-v1"),
            master,
        }
    }

    pub fn master(&self) -> &SecretKey {
        &self.master
    }

    /// Opaque id for the full image of a clip (same image → same id).
    pub fn blob_id(&self, content_hash: &[u8]) -> String {
        b64(&self.tags.mac(&[b"blob", content_hash])[..16])
    }

    /// Opaque per-clip tag the hub uses to compact its log.
    pub fn hash_tag(&self, content_hash: &[u8]) -> String {
        b64(&self.tags.mac(&[b"hash", content_hash])[..16])
    }
}

impl std::fmt::Debug for SpaceKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SpaceKeys(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip() {
        let k = SecretKey::generate();
        let s = k.seal(b"ctx", b"hello").unwrap();
        assert_eq!(s.ct.len(), 5 + TAG_LEN);
        assert_eq!(k.open(&s.nonce, b"ctx", &s.ct).unwrap(), b"hello");
        let packed = k.seal_packed(b"ctx", b"blob bytes").unwrap();
        assert_eq!(k.open_packed(b"ctx", &packed).unwrap(), b"blob bytes");
        // Nonces are fresh.
        assert_ne!(k.seal(b"ctx", b"hello").unwrap().nonce, s.nonce);
    }

    #[test]
    fn wrong_key_or_context_fails() {
        let k = SecretKey::generate();
        let other = SecretKey::generate();
        let s = k.seal(b"ctx", b"hello").unwrap();
        assert_eq!(other.open(&s.nonce, b"ctx", &s.ct), Err(CryptoError::Open));
        assert_eq!(k.open(&s.nonce, b"other", &s.ct), Err(CryptoError::Open));
        let mut nonce = s.nonce;
        nonce[0] ^= 1;
        assert_eq!(k.open(&nonce, b"ctx", &s.ct), Err(CryptoError::Open));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let k = SecretKey::generate();
        let s = k.seal(b"", b"hello world").unwrap();
        for i in 0..s.ct.len() {
            let mut ct = s.ct.clone();
            ct[i] ^= 0x80;
            assert_eq!(
                k.open(&s.nonce, b"", &ct),
                Err(CryptoError::Open),
                "byte {i}"
            );
        }
        let mut short = s.ct.clone();
        short.pop();
        assert!(k.open(&s.nonce, b"", &short).is_err());
        assert!(k.open_packed(b"", &[0u8; 10]).is_err());
    }

    #[test]
    fn aad_is_unambiguous() {
        assert_ne!(aad(&[b"ab", b"c"]), aad(&[b"a", b"bc"]));
    }

    #[test]
    fn hmac_and_hkdf() {
        let tag = hmac_sha256(b"key", &[b"a", b"b"]);
        assert!(hmac_verify(b"key", &[b"a", b"b"], &tag));
        assert!(!hmac_verify(b"key", &[b"a", b"c"], &tag));
        assert!(!hmac_verify(b"key2", &[b"a", b"b"], &tag));
        assert!(!hmac_verify(b"key", &[b"a", b"b"], &tag[..16]));
        let k = SecretKey::from_bytes([7; 32]);
        assert_ne!(k.derive("x").as_bytes(), k.derive("y").as_bytes());
        assert_eq!(k.derive("x"), k.derive("x"));
    }

    #[test]
    fn space_keys_are_separated() {
        let keys = SpaceKeys::new(SecretKey::generate());
        let h = [1u8; 32];
        assert_ne!(keys.blob_id(&h), keys.hash_tag(&h));
        assert_eq!(keys.blob_id(&h), keys.blob_id(&h));
        assert_ne!(keys.events.as_bytes(), keys.blobs.as_bytes());
        assert_eq!(format!("{keys:?}"), "SpaceKeys(..)");
        assert_eq!(unb64(&b64(b"\x00\xffxyz")).unwrap(), b"\x00\xffxyz");
    }
}
