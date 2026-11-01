//! The relay protocol, shared by devices and the `clipd-relay` server
//! (no server dependencies here, so Android builds it too).
//!
//! A relay is a dumb mailbox reachable over the internet: per space it
//! stores sealed envelopes (with a relay-assigned `seq`), sealed image
//! blobs and the public keys of enrolled devices. It never holds a key that
//! can open anything. See `docs/adr/0001-off-lan-sync.md`.
//!
//! **Auth.** Every device has its own Ed25519 key (keystore entry
//! [`RELAY_KEY`]); the relay stores only the public half. Requests carry
//!
//! `Authorization: CTR1 space=<rsid>, device=<id>, ts=<unix ms>, nonce=<b64url 16 B>, sig=<b64url>`
//!
//! where `sig` signs method, path-and-query (relative to the relay base
//! URL), time, nonce and the SHA-256 of the body. No secret ever crosses
//! the wire, so a TLS-terminating proxy or a leaked log can't replay more
//! than the one request (and the relay refuses repeats inside the clock
//! window).
//!
//! **Joining.** The device that adds a relay registers the space with its
//! own key plus a space *enroll key* (also Ed25519). The enroll key travels
//! to the other devices inside a sealed `SpaceConfig` event; each device
//! then enrolls its own key by signing an enroll proof with it. Revoking a
//! device deletes its key on the relay and rotates the enroll key.

use clipd_core::Millis;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::auth::MAX_SKEW_MS;
use crate::crypto::{SpaceKeys, aad, b64, random_bytes, sha256, unb64, unb64_array};
use crate::envelope::Envelope;

pub const SCHEME: &str = "CTR1";
pub const RELAY_PROTO: u32 = 1;
/// Keystore entry holding this device's Ed25519 relay key (32-byte seed).
pub const RELAY_KEY: &str = "relay-key";

/// Opaque id of a space on relays: a keyed tag, so the relay never learns
/// the space id the envelopes are bound to (and different spaces can't be
/// correlated by it).
pub fn relay_space_id(keys: &SpaceKeys, space_id: &str) -> String {
    b64(&keys.tags.mac(&[b"relay-space", space_id.as_bytes()])[..16])
}

/// Ids the relay accepts (device ids, relay space ids, event ids).
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ---- URLs ------------------------------------------------------------------

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RelayUrlError {
    #[error("not a valid relay URL (expected https://relay.example.com)")]
    Invalid,
    #[error(
        "relay URLs on the internet must use https:// (plain http is only allowed for localhost and private LAN addresses)"
    )]
    InsecureHttp,
}

/// Normalize a relay base URL: `https://host[:port][/prefix]`, no query,
/// no trailing slash. Plain `http://` is refused unless the host is
/// loopback, a private/link-local IP or a `.local`/`localhost` name.
pub fn normalize_url(input: &str) -> Result<String, RelayUrlError> {
    let mut s = input.trim().to_owned();
    if !s.contains("://") {
        s = format!("https://{s}");
    }
    let url = reqwest::Url::parse(&s).map_err(|_| RelayUrlError::Invalid)?;
    let host = url.host_str().ok_or(RelayUrlError::Invalid)?.to_owned();
    if url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(RelayUrlError::Invalid);
    }
    match url.scheme() {
        "https" => {}
        "http" if is_local_host(&host) => {}
        "http" => return Err(RelayUrlError::InsecureHttp),
        _ => return Err(RelayUrlError::Invalid),
    }
    let mut out = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(&host));
    if let Some(p) = url.port() {
        out.push_str(&format!(":{p}"));
    }
    let path = url.path().trim_end_matches('/');
    out.push_str(path);
    Ok(out)
}

/// Loopback, private or link-local addresses and `localhost`/`*.local`.
pub fn is_local_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    if h.eq_ignore_ascii_case("localhost") || h.to_ascii_lowercase().ends_with(".local") {
        return true;
    }
    match h.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || (o[0] == 100 && (64..128).contains(&o[1]))
        }
        Ok(std::net::IpAddr::V6(v6)) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_local_host(&v4.to_string());
            }
            let seg = v6.segments()[0];
            v6.is_loopback() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
        }
        Err(_) => false,
    }
}

/// `http(s)://base` + `/v1/...` → `ws(s)://base/v1/...`.
pub fn ws_url(base: &str, path_and_query: &str) -> String {
    let b = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_owned()
    };
    format!("{b}{path_and_query}")
}

// ---- keys --------------------------------------------------------------------

/// An Ed25519 key (device key or space enroll key). Never prints.
pub struct DeviceKey(SigningKey);

impl DeviceKey {
    pub fn generate() -> DeviceKey {
        DeviceKey(SigningKey::from_bytes(&random_bytes::<32>()))
    }

    pub fn from_seed(seed: &[u8]) -> Option<DeviceKey> {
        let arr: [u8; 32] = seed.try_into().ok()?;
        Some(DeviceKey(SigningKey::from_bytes(&arr)))
    }

    pub fn seed(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// Public key, base64url (43 chars).
    pub fn public_b64(&self) -> String {
        b64(self.0.verifying_key().as_bytes())
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.0.sign(msg).to_bytes()
    }
}

impl std::fmt::Debug for DeviceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceKey(..)")
    }
}

/// Parse a base64url Ed25519 public key.
pub fn parse_public(b64_pub: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = unb64_array(b64_pub).ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// Strict Ed25519 verification (rejects malleable/small-order forms).
pub fn verify(public: &VerifyingKey, msg: &[u8], sig: &[u8]) -> bool {
    let Ok(sig) = Signature::from_slice(sig) else {
        return false;
    };
    public.verify_strict(msg, &sig).is_ok()
}

// ---- request auth ----------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn request_message(
    space: &str,
    device: &str,
    method: &str,
    path_and_query: &str,
    ts: &str,
    nonce: &[u8],
    body: &[u8],
) -> Vec<u8> {
    aad(&[
        b"cliptrail-relay-auth-v1",
        space.as_bytes(),
        device.as_bytes(),
        method.as_bytes(),
        path_and_query.as_bytes(),
        ts.as_bytes(),
        nonce,
        &sha256(body),
    ])
}

/// `Authorization` header value for a relay request.
pub fn sign_request(
    space: &str,
    device: &str,
    key: &DeviceKey,
    method: &str,
    path_and_query: &str,
    ts: Millis,
    body: &[u8],
) -> String {
    let ts_s = ts.to_string();
    let nonce: [u8; 16] = random_bytes();
    let sig = key.sign(&request_message(
        space,
        device,
        method,
        path_and_query,
        &ts_s,
        &nonce,
        body,
    ));
    format!(
        "{SCHEME} space={space}, device={device}, ts={ts_s}, nonce={}, sig={}",
        b64(&nonce),
        b64(&sig)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayAuth {
    pub space: String,
    pub device: String,
    pub ts: Millis,
    pub nonce: Vec<u8>,
    pub sig: Vec<u8>,
}

impl RelayAuth {
    pub fn parse(value: &str) -> Option<RelayAuth> {
        let rest = value.trim().strip_prefix(SCHEME)?.trim_start();
        let (mut space, mut device, mut ts, mut nonce, mut sig) = (None, None, None, None, None);
        for field in rest.split(',') {
            let (k, v) = field.trim().split_once('=')?;
            let v = v.trim();
            match k.trim() {
                "space" => space = Some(v.to_owned()),
                "device" => device = Some(v.to_owned()),
                "ts" => ts = v.parse::<Millis>().ok(),
                "nonce" => nonce = unb64(v).ok(),
                "sig" => sig = unb64(v).ok(),
                _ => {}
            }
        }
        Some(RelayAuth {
            space: space.filter(|s| valid_id(s))?,
            device: device.filter(|d| valid_id(d))?,
            ts: ts?,
            nonce: nonce.filter(|n| n.len() == 16)?,
            sig: sig.filter(|s| s.len() == 64)?,
        })
    }

    pub fn fresh(&self, now: Millis) -> bool {
        (now - self.ts).abs() <= MAX_SKEW_MS
    }

    pub fn verify(
        &self,
        public: &VerifyingKey,
        method: &str,
        path_and_query: &str,
        body: &[u8],
    ) -> bool {
        let msg = request_message(
            &self.space,
            &self.device,
            method,
            path_and_query,
            &self.ts.to_string(),
            &self.nonce,
            body,
        );
        verify(public, &msg, &self.sig)
    }
}

/// What a device signs with the space enroll key to join a relay.
pub fn enroll_message(space: &str, device: &str, device_pub: &str, ts: Millis) -> Vec<u8> {
    aad(&[
        b"cliptrail-relay-enroll-v1",
        space.as_bytes(),
        device.as_bytes(),
        device_pub.as_bytes(),
        &ts.to_le_bytes(),
    ])
}

// ---- wire shapes -------------------------------------------------------------------

/// `GET /v1/hello` (no auth).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayHello {
    pub proto: u32,
    /// Always "clipd-relay" (lets a client tell a relay from a LAN hub).
    pub kind: String,
    pub version: String,
    /// "open", "invite" (needs the operator's invite code) or "closed".
    pub registration: String,
    pub limits: RelayLimits,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayLimits {
    pub max_event_bytes: u64,
    pub max_blob_bytes: u64,
    pub max_space_bytes: u64,
    pub max_devices: u32,
    pub retention_days: u32,
}

/// `POST /v1/spaces` (CTR1-signed with `device_pub`'s key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub space_id: String,
    pub device_id: String,
    pub device_pub: String,
    pub enroll_pub: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite: Option<String>,
}

/// `POST /v1/enroll` (CTR1-signed with `device_pub`'s key; `proof` is an
/// Ed25519 signature by the space enroll key over [`enroll_message`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub space_id: String,
    pub device_id: String,
    pub device_pub: String,
    pub ts: Millis,
    pub proof: String,
}

/// `PUT /v1/enroll-key`: replace the space enroll key (after a revoke).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateEnrollRequest {
    pub enroll_pub: String,
}

/// One envelope pushed to a relay, with the routing metadata it needs to
/// compact its log like the hub does. `tag` is the keyed per-clip tag
/// ([`SpaceKeys::hash_tag`]), never a content hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayPushItem {
    #[serde(flatten)]
    pub env: Envelope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// "ClipAdded" | "ClipUpdated" | "ClipDeleted" | "SpaceConfig".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Blob this event references (deleted with the event's last copy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// `POST /v1/events` body on a relay. Reply: [`crate::wire::PushResponse`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPushRequest {
    pub events: Vec<RelayPushItem>,
}

/// Event kinds a relay accepts as compaction metadata.
pub const EVENT_KINDS: &[&str] = &["ClipAdded", "ClipUpdated", "ClipDeleted", "SpaceConfig"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayDevice {
    pub device_id: String,
    pub enrolled_at: Millis,
    pub last_seen: Option<Millis>,
    pub revoked_at: Option<Millis>,
}

/// `GET /v1/space`: usage and enrolled devices.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceInfo {
    pub space_id: String,
    pub head_seq: i64,
    pub envelopes: i64,
    pub blobs: i64,
    pub bytes: i64,
    pub max_bytes: i64,
    pub retention_days: u32,
    pub devices: Vec<RelayDevice>,
}

/// WebSocket close code a relay sends to a revoked device.
pub const REVOKED_CLOSE_CODE: u16 = 4001;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            normalize_url("relay.example.com").unwrap(),
            "https://relay.example.com"
        );
        assert_eq!(
            normalize_url(" https://Relay.Example.com:8443/ct/ ").unwrap(),
            "https://relay.example.com:8443/ct"
        );
        assert_eq!(
            normalize_url("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080"
        );
        assert_eq!(
            normalize_url("http://192.168.1.4:8080/").unwrap(),
            "http://192.168.1.4:8080"
        );
        assert_eq!(
            normalize_url("http://localhost:9").unwrap(),
            "http://localhost:9"
        );
        assert_eq!(
            normalize_url("http://relay.example.com"),
            Err(RelayUrlError::InsecureHttp)
        );
        assert_eq!(
            normalize_url("http://8.8.8.8"),
            Err(RelayUrlError::InsecureHttp)
        );
        assert_eq!(
            normalize_url("ftp://x.example"),
            Err(RelayUrlError::Invalid)
        );
        assert_eq!(
            normalize_url("https://u:p@x.example"),
            Err(RelayUrlError::Invalid)
        );
        assert_eq!(
            normalize_url("https://x.example/?a=1"),
            Err(RelayUrlError::Invalid)
        );
        assert_eq!(
            ws_url("https://r.example/ct", "/v1/stream?since=3"),
            "wss://r.example/ct/v1/stream?since=3"
        );
        assert_eq!(
            ws_url("http://127.0.0.1:1", "/v1/stream"),
            "ws://127.0.0.1:1/v1/stream"
        );
    }

    #[test]
    fn sign_parse_verify() {
        let key = DeviceKey::generate();
        let public = parse_public(&key.public_b64()).unwrap();
        let h = sign_request("SPACE", "DEV1", &key, "POST", "/v1/events", 1000, b"{}");
        let a = RelayAuth::parse(&h).unwrap();
        assert_eq!((a.space.as_str(), a.device.as_str()), ("SPACE", "DEV1"));
        assert!(a.verify(&public, "POST", "/v1/events", b"{}"));
        assert!(!a.verify(&public, "POST", "/v1/events", b"{ }"));
        assert!(!a.verify(&public, "GET", "/v1/events", b"{}"));
        assert!(!a.verify(&public, "POST", "/v1/events?since=1", b"{}"));
        let other = parse_public(&DeviceKey::generate().public_b64()).unwrap();
        assert!(!a.verify(&other, "POST", "/v1/events", b"{}"));
        let mut forged = a.clone();
        forged.device = "DEV2".into();
        assert!(!forged.verify(&public, "POST", "/v1/events", b"{}"));
        let mut forged = a.clone();
        forged.space = "OTHER".into();
        assert!(!forged.verify(&public, "POST", "/v1/events", b"{}"));
        assert!(a.fresh(1000 + MAX_SKEW_MS) && !a.fresh(1001 + MAX_SKEW_MS));
        // Same request twice → different nonces and signatures.
        let h2 = sign_request("SPACE", "DEV1", &key, "POST", "/v1/events", 1000, b"{}");
        assert_ne!(RelayAuth::parse(&h2).unwrap().sig, a.sig);
        for bad in [
            "",
            "CT1 device=x, ts=1, mac=AAAA",
            "CTR1 space=a b, device=x, ts=1, nonce=AAAAAAAAAAAAAAAAAAAAAA, sig=AA",
            "CTR1 space=s, device=x, ts=1, nonce=short, sig=AA",
        ] {
            assert!(RelayAuth::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn keys_roundtrip_and_never_print() {
        let k = DeviceKey::generate();
        let k2 = DeviceKey::from_seed(&k.seed()).unwrap();
        assert_eq!(k.public_b64(), k2.public_b64());
        assert_eq!(format!("{k:?}"), "DeviceKey(..)");
        let msg = enroll_message("S", "D", &k.public_b64(), 5);
        let sig = k.sign(&msg);
        let p = parse_public(&k.public_b64()).unwrap();
        assert!(verify(&p, &msg, &sig));
        assert!(!verify(
            &p,
            &enroll_message("S", "D2", &k.public_b64(), 5),
            &sig
        ));
        assert!(DeviceKey::from_seed(&[1, 2]).is_none());
    }

    #[test]
    fn push_item_json_is_flat() {
        let item = RelayPushItem {
            env: Envelope {
                v: 1,
                event_id: "E".into(),
                device_id: "D".into(),
                ts: 1,
                nonce: "n".repeat(32),
                ct: "c".into(),
            },
            tag: Some("t".into()),
            kind: Some("ClipAdded".into()),
            blob: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"event_id\":\"E\"") && json.contains("\"tag\":\"t\""));
        assert!(!json.contains("blob"));
        assert_eq!(serde_json::from_str::<RelayPushItem>(&json).unwrap(), item);
    }
}
