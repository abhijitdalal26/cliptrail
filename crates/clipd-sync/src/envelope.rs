//! Sync events and the sealed envelope they travel in.
//!
//! The envelope is storage-agnostic: the v1 hub (a user device that holds
//! the MasterKey) stores the same envelopes a dumb ciphertext-only relay
//! will store later. Only ids, the sender and a timestamp are visible.

use clipd_core::{Kind, Millis};
use serde::{Deserialize, Serialize};

use crate::crypto::{SpaceKeys, aad, b64, unb64, unb64_array};

pub const ENVELOPE_VERSION: u8 = 1;
/// Upper bound on one sealed event (text clips are capped at 2 MB by the
/// core; HTML can add the same again; thumbnails are small).
pub const MAX_EVENT_BYTES: usize = 6 * 1024 * 1024;

/// What the hub stores and relays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    /// ULID, unique per event (idempotency key).
    pub event_id: String,
    /// Sender.
    pub device_id: String,
    /// Sender's clock, unix ms.
    pub ts: Millis,
    /// 24-byte XChaCha20 nonce, base64url.
    pub nonce: String,
    /// Ciphertext of the JSON [`Event`], base64url.
    pub ct: String,
}

/// An envelope with its hub-assigned position in the space log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeqEnvelope {
    pub seq: i64,
    #[serde(flatten)]
    pub env: Envelope,
}

/// Plaintext inside an envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum Event {
    /// A clip was copied (or copied again: receivers merge by hash).
    ClipAdded(Box<ClipAdded>),
    /// Pinned flag changed at `at`.
    ClipUpdated {
        hash: String,
        pinned: bool,
        at: Millis,
    },
    /// Tombstone: receivers wipe the clip unless they copied it after `at`.
    ClipDeleted { hash: String, at: Millis },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipAdded {
    /// Content hash (base64url), identity across devices.
    pub hash: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subkind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    /// Device the clip was last copied on.
    pub origin: String,
    pub created_at: Millis,
    pub last_seen_at: Millis,
    pub copy_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    /// Fetch the sealed full image from `GET /v1/blobs/{blob_id}`.
    pub blob_id: String,
    pub w: u32,
    pub h: u32,
    /// Inline PNG thumbnail, base64url.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
}

impl Event {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Event::ClipAdded(_) => "ClipAdded",
            Event::ClipUpdated { .. } => "ClipUpdated",
            Event::ClipDeleted { .. } => "ClipDeleted",
        }
    }

    /// The content hash this event is about (base64url).
    pub fn hash_b64(&self) -> &str {
        match self {
            Event::ClipAdded(c) => &c.hash,
            Event::ClipUpdated { hash, .. } | Event::ClipDeleted { hash, .. } => hash,
        }
    }

    pub fn hash(&self) -> Result<Vec<u8>, EnvelopeError> {
        let h = unb64(self.hash_b64()).map_err(|_| EnvelopeError::Shape("hash"))?;
        if h.len() != 32 {
            return Err(EnvelopeError::Shape("hash"));
        }
        Ok(h)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EnvelopeError {
    #[error("malformed envelope ({0})")]
    Shape(&'static str),
    #[error("envelope does not decrypt with this space's key")]
    Open,
    #[error("event payload is not valid")]
    Payload,
    #[error("event too large")]
    TooLarge,
}

fn envelope_aad(space_id: &str, event_id: &str, device_id: &str, ts: Millis) -> Vec<u8> {
    aad(&[
        b"cliptrail-ev-v1",
        space_id.as_bytes(),
        event_id.as_bytes(),
        device_id.as_bytes(),
        &ts.to_le_bytes(),
    ])
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

impl Envelope {
    /// Seal `event` as `device_id` in `space_id`, with a fresh ULID.
    pub fn seal(
        keys: &SpaceKeys,
        space_id: &str,
        device_id: &str,
        ts: Millis,
        event: &Event,
    ) -> Result<Envelope, EnvelopeError> {
        let event_id = ulid::Ulid::generate().to_string();
        let plaintext = serde_json::to_vec(event).map_err(|_| EnvelopeError::Payload)?;
        if plaintext.len() > MAX_EVENT_BYTES {
            return Err(EnvelopeError::TooLarge);
        }
        let sealed = keys
            .events
            .seal(
                &envelope_aad(space_id, &event_id, device_id, ts),
                &plaintext,
            )
            .map_err(|_| EnvelopeError::Payload)?;
        Ok(Envelope {
            v: ENVELOPE_VERSION,
            event_id,
            device_id: device_id.to_owned(),
            ts,
            nonce: b64(&sealed.nonce),
            ct: b64(&sealed.ct),
        })
    }

    /// Structural checks a ciphertext-only relay can do too.
    pub fn validate_shape(&self) -> Result<(), EnvelopeError> {
        if self.v != ENVELOPE_VERSION {
            return Err(EnvelopeError::Shape("version"));
        }
        if !valid_id(&self.event_id) {
            return Err(EnvelopeError::Shape("event_id"));
        }
        if !valid_id(&self.device_id) {
            return Err(EnvelopeError::Shape("device_id"));
        }
        if self.nonce.len() != 32 {
            return Err(EnvelopeError::Shape("nonce"));
        }
        if self.ct.len() > MAX_EVENT_BYTES * 4 / 3 + 64 {
            return Err(EnvelopeError::TooLarge);
        }
        Ok(())
    }

    pub fn open(&self, keys: &SpaceKeys, space_id: &str) -> Result<Event, EnvelopeError> {
        self.validate_shape()?;
        let nonce = unb64_array(&self.nonce).map_err(|_| EnvelopeError::Shape("nonce"))?;
        let ct = unb64(&self.ct).map_err(|_| EnvelopeError::Shape("ct"))?;
        let plaintext = keys
            .events
            .open(
                &nonce,
                &envelope_aad(space_id, &self.event_id, &self.device_id, self.ts),
                &ct,
            )
            .map_err(|_| EnvelopeError::Open)?;
        let event: Event =
            serde_json::from_slice(&plaintext).map_err(|_| EnvelopeError::Payload)?;
        event.hash()?;
        Ok(event)
    }
}

/// AAD for a sealed image blob.
pub fn blob_aad(space_id: &str, blob_id: &str) -> Vec<u8> {
    aad(&[
        b"cliptrail-blob-v1",
        space_id.as_bytes(),
        blob_id.as_bytes(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SecretKey;

    fn sample() -> Event {
        Event::ClipAdded(Box::new(ClipAdded {
            hash: b64(&[3u8; 32]),
            kind: Kind::Link,
            subkind: Some("github.com".into()),
            text: Some("https://github.com".into()),
            html: None,
            files: vec![],
            source_app: Some("chrome.exe".into()),
            origin: "DEV1".into(),
            created_at: 1,
            last_seen_at: 2,
            copy_count: 1,
            image: None,
        }))
    }

    #[test]
    fn envelope_roundtrip_and_json_shape() {
        let keys = SpaceKeys::new(SecretKey::generate());
        let env = Envelope::seal(&keys, "SPACE", "DEV1", 42, &sample()).unwrap();
        assert_eq!(env.event_id.len(), 26);
        let json = serde_json::to_string(&SeqEnvelope {
            seq: 7,
            env: env.clone(),
        })
        .unwrap();
        // Nothing but ids, ts and ciphertext on the wire.
        assert!(!json.contains("github"));
        assert!(json.contains("\"seq\":7"));
        let back: SeqEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.env, env);
        assert_eq!(back.env.open(&keys, "SPACE").unwrap(), sample());
    }

    #[test]
    fn envelope_is_bound_to_key_space_and_header() {
        let keys = SpaceKeys::new(SecretKey::generate());
        let env = Envelope::seal(&keys, "SPACE", "DEV1", 42, &sample()).unwrap();
        let other = SpaceKeys::new(SecretKey::generate());
        assert_eq!(env.open(&other, "SPACE"), Err(EnvelopeError::Open));
        assert_eq!(env.open(&keys, "OTHER"), Err(EnvelopeError::Open));
        let mut forged = env.clone();
        forged.device_id = "DEV2".into();
        assert_eq!(forged.open(&keys, "SPACE"), Err(EnvelopeError::Open));
        let mut forged = env.clone();
        forged.ts += 1;
        assert_eq!(forged.open(&keys, "SPACE"), Err(EnvelopeError::Open));
        let mut tampered = env.clone();
        let mut ct = unb64(&tampered.ct).unwrap();
        ct[3] ^= 1;
        tampered.ct = b64(&ct);
        assert_eq!(tampered.open(&keys, "SPACE"), Err(EnvelopeError::Open));
        let mut bad = env;
        bad.event_id = "../x".into();
        assert_eq!(bad.validate_shape(), Err(EnvelopeError::Shape("event_id")));
    }

    #[test]
    fn event_json_tags() {
        let e = Event::ClipDeleted {
            hash: b64(&[1; 32]),
            at: 5,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.starts_with("{\"t\":\"ClipDeleted\""), "{json}");
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), e);
        assert_eq!(e.hash().unwrap(), vec![1; 32]);
        let bad = Event::ClipUpdated {
            hash: "short".into(),
            pinned: true,
            at: 1,
        };
        assert!(bad.hash().is_err());
    }
}
