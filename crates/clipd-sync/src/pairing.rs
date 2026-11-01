//! QR pairing.
//!
//! 1. The hub makes a one-time offer: a fresh X25519 key pair and a random
//!    16-byte secret, valid for [`OFFER_TTL_MS`]. The QR carries
//!    `cliptrail://pair?v=1&h=<host:port,...>&k=<hub pub>&s=<secret>&n=<name>`.
//! 2. The new device makes its own X25519 key pair and POSTs
//!    `{device_id, name, platform, pub, proof}`, where
//!    `proof = HMAC(secret, pub ‖ device_id ‖ name ‖ platform ‖ hub pub)`.
//! 3. The hub checks the proof, burns the offer, computes
//!    `k = HKDF(X25519(hub, device), salt = secret, info = both pubs)` and
//!    returns the MasterKey + a fresh device token sealed under `k`.
//!
//! The QR is an out-of-band channel: an attacker on the LAN can't swap the
//! hub key in it, can't forge a proof without the secret, and can't open
//! the reply without the device's private key.

use clipd_core::Millis;
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::crypto::{
    CryptoError, KEY_LEN, SecretKey, aad, b64, hkdf_sha256, hmac_sha256, hmac_verify, random_bytes,
    unb64, unb64_array,
};

pub const PAIR_VERSION: u8 = 1;
/// How long a pairing QR stays valid.
pub const OFFER_TTL_MS: Millis = 10 * 60 * 1000;
pub const SECRET_LEN: usize = 16;
const URI_PREFIX: &str = "cliptrail://pair?";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PairError {
    #[error("not a ClipTrail pairing link")]
    BadUri,
    #[error("malformed pairing request")]
    BadRequest,
    #[error("pairing code is wrong or was not issued by this computer")]
    BadProof,
    #[error("pairing code expired; show a new QR code")]
    Expired,
    #[error("pairing code was already used; show a new QR code")]
    AlreadyUsed,
    #[error("too many pairing attempts; wait a minute")]
    RateLimited,
    #[error("pairing reply could not be verified")]
    BadResponse,
}

impl From<CryptoError> for PairError {
    fn from(_: CryptoError) -> Self {
        PairError::BadRequest
    }
}

/// What the QR code encodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairUri {
    /// `host:port` candidates (every private IPv4 of the hub).
    pub hosts: Vec<String>,
    pub hub_pub: [u8; 32],
    pub secret: [u8; SECRET_LEN],
    /// Hub display name.
    pub name: String,
}

impl PairUri {
    pub fn to_uri(&self) -> String {
        format!(
            "{URI_PREFIX}v={PAIR_VERSION}&h={}&k={}&s={}&n={}",
            self.hosts.join(","),
            b64(&self.hub_pub),
            b64(&self.secret),
            pct_encode(&self.name)
        )
    }

    pub fn parse(s: &str) -> Result<PairUri, PairError> {
        let q = s.trim().strip_prefix(URI_PREFIX).ok_or(PairError::BadUri)?;
        let (mut hosts, mut k, mut sec, mut name, mut v) = (None, None, None, None, None);
        for pair in q.split('&') {
            let (key, val) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "v" => v = val.parse::<u8>().ok(),
                "h" => {
                    hosts = Some(
                        val.split(',')
                            .map(|h| pct_decode(h).trim().to_owned())
                            .filter(|h| !h.is_empty())
                            .collect::<Vec<_>>(),
                    )
                }
                "k" => k = unb64_array::<32>(val).ok(),
                "s" => sec = unb64_array::<SECRET_LEN>(val).ok(),
                "n" => name = Some(pct_decode(val)),
                _ => {}
            }
        }
        if v != Some(PAIR_VERSION) {
            return Err(PairError::BadUri);
        }
        let hosts = hosts.filter(|h| !h.is_empty()).ok_or(PairError::BadUri)?;
        Ok(PairUri {
            hosts,
            hub_pub: k.ok_or(PairError::BadUri)?,
            secret: sec.ok_or(PairError::BadUri)?,
            name: name.unwrap_or_else(|| "ClipTrail".into()),
        })
    }
}

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(a), Some(b)) => {
                        out.push((a * 16 + b) as u8);
                        i += 3;
                        continue;
                    }
                    _ => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `POST /v1/pair` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    pub v: u8,
    pub device_id: String,
    pub name: String,
    /// "windows" | "macos" | "linux" | "android" | "ios".
    pub platform: String,
    /// Device X25519 public key, base64url.
    #[serde(rename = "pub")]
    pub pub_key: String,
    /// base64url HMAC, see module docs.
    pub proof: String,
}

/// `POST /v1/pair` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairResponse {
    pub v: u8,
    pub space_id: String,
    pub hub_device_id: String,
    pub hub_name: String,
    /// Sealed [`PairGrant`] (nonce ‖ ct), base64url.
    pub grant: String,
}

/// Secrets handed to the new device; only ever travels sealed.
#[derive(Serialize, Deserialize)]
struct PairGrant {
    master_key: String,
    token: String,
}

/// What a device knows after pairing.
#[derive(Debug, Clone)]
pub struct PairResult {
    pub space_id: String,
    pub hub_device_id: String,
    pub hub_name: String,
    pub master_key: SecretKey,
    pub token: SecretKey,
    /// 4 digits both screens can show to confirm they paired with each other.
    pub check_code: String,
}

fn proof_parts<'a>(
    req_pub: &'a [u8],
    device_id: &'a str,
    name: &'a str,
    platform: &'a str,
    hub_pub: &'a [u8],
) -> [&'a [u8]; 6] {
    [
        b"cliptrail-pair-proof-v1",
        req_pub,
        device_id.as_bytes(),
        name.as_bytes(),
        platform.as_bytes(),
        hub_pub,
    ]
}

fn pair_key(shared: &[u8; 32], secret: &[u8], hub_pub: &[u8], dev_pub: &[u8]) -> SecretKey {
    let mut k = [0u8; KEY_LEN];
    hkdf_sha256(
        shared,
        Some(secret),
        &aad(&[b"cliptrail-pair-v1", hub_pub, dev_pub]),
        &mut k,
    );
    SecretKey::from_bytes(k)
}

fn check_code(k: &SecretKey) -> String {
    let d = k.derive("cliptrail-pair-check-v1");
    let n = u32::from_le_bytes(d.as_bytes()[..4].try_into().expect("4 bytes")) % 10_000;
    format!("{n:04}")
}

fn grant_aad(space_id: &str, device_id: &str) -> Vec<u8> {
    aad(&[
        b"cliptrail-pair-grant-v1",
        space_id.as_bytes(),
        device_id.as_bytes(),
    ])
}

fn valid_text(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}

/// Device side of pairing.
pub struct ClientPairing {
    uri: PairUri,
    secret: StaticSecret,
    request: PairRequest,
}

impl ClientPairing {
    pub fn new(uri: PairUri, device_id: &str, name: &str, platform: &str) -> ClientPairing {
        let secret = StaticSecret::from(random_bytes::<32>());
        let public = PublicKey::from(&secret);
        let proof = hmac_sha256(
            &uri.secret,
            &proof_parts(public.as_bytes(), device_id, name, platform, &uri.hub_pub),
        );
        let request = PairRequest {
            v: PAIR_VERSION,
            device_id: device_id.to_owned(),
            name: name.to_owned(),
            platform: platform.to_owned(),
            pub_key: b64(public.as_bytes()),
            proof: b64(&proof),
        };
        ClientPairing {
            uri,
            secret,
            request,
        }
    }

    pub fn uri(&self) -> &PairUri {
        &self.uri
    }

    pub fn request(&self) -> &PairRequest {
        &self.request
    }

    pub fn finish(self, resp: &PairResponse) -> Result<PairResult, PairError> {
        if resp.v != PAIR_VERSION {
            return Err(PairError::BadResponse);
        }
        let hub = PublicKey::from(self.uri.hub_pub);
        let shared = self.secret.diffie_hellman(&hub);
        if !shared.was_contributory() {
            return Err(PairError::BadResponse);
        }
        let my_pub = PublicKey::from(&self.secret);
        let k = pair_key(
            shared.as_bytes(),
            &self.uri.secret,
            &self.uri.hub_pub,
            my_pub.as_bytes(),
        );
        let packed = unb64(&resp.grant).map_err(|_| PairError::BadResponse)?;
        let plain = k
            .open_packed(&grant_aad(&resp.space_id, &self.request.device_id), &packed)
            .map_err(|_| PairError::BadResponse)?;
        let grant: PairGrant =
            serde_json::from_slice(&plain).map_err(|_| PairError::BadResponse)?;
        let master_key =
            SecretKey::from_slice(&unb64(&grant.master_key).map_err(|_| PairError::BadResponse)?)
                .map_err(|_| PairError::BadResponse)?;
        let token =
            SecretKey::from_slice(&unb64(&grant.token).map_err(|_| PairError::BadResponse)?)
                .map_err(|_| PairError::BadResponse)?;
        Ok(PairResult {
            space_id: resp.space_id.clone(),
            hub_device_id: resp.hub_device_id.clone(),
            hub_name: resp.hub_name.clone(),
            master_key,
            token,
            check_code: check_code(&k),
        })
    }
}

/// One QR code's worth of pairing state (hub side, memory only).
pub struct PairOffer {
    pub uri: PairUri,
    secret: StaticSecret,
    pub created_at: Millis,
    pub expires_at: Millis,
    pub used: bool,
}

/// A device the hub accepted.
#[derive(Debug, Clone)]
pub struct AcceptedDevice {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    pub token: SecretKey,
    pub check_code: String,
}

/// Hub side: live offers plus a global attempt limiter.
#[derive(Default)]
pub struct PairOffers {
    offers: Vec<PairOffer>,
    attempts: Vec<Millis>,
}

/// Max `POST /v1/pair` attempts per minute (all sources together).
pub const MAX_PAIR_ATTEMPTS_PER_MIN: usize = 10;

impl PairOffers {
    pub fn create(&mut self, hosts: Vec<String>, hub_name: &str, now: Millis) -> PairUri {
        self.offers.retain(|o| now < o.expires_at + OFFER_TTL_MS);
        let secret = StaticSecret::from(random_bytes::<32>());
        let uri = PairUri {
            hosts,
            hub_pub: PublicKey::from(&secret).to_bytes(),
            secret: random_bytes(),
            name: hub_name.to_owned(),
        };
        self.offers.push(PairOffer {
            uri: uri.clone(),
            secret,
            created_at: now,
            expires_at: now + OFFER_TTL_MS,
            used: false,
        });
        uri
    }

    /// Latest offer that can still be used.
    pub fn active(&self, now: Millis) -> Option<&PairOffer> {
        self.offers
            .iter()
            .rev()
            .find(|o| !o.used && now < o.expires_at)
    }

    /// Withdraw every unused offer (the user closed the QR).
    pub fn cancel_all(&mut self) {
        for o in &mut self.offers {
            o.used = true;
        }
    }

    /// Check a request against the live offers. On success the offer is
    /// burned and the sealed reply is returned with the new device's token.
    pub fn accept(
        &mut self,
        req: &PairRequest,
        now: Millis,
        space_id: &str,
        hub_device_id: &str,
        hub_name: &str,
        master: &SecretKey,
    ) -> Result<(PairResponse, AcceptedDevice), PairError> {
        self.attempts.retain(|t| now - t < 60_000);
        if self.attempts.len() >= MAX_PAIR_ATTEMPTS_PER_MIN {
            return Err(PairError::RateLimited);
        }
        self.attempts.push(now);

        let id_ok = !req.device_id.is_empty()
            && req.device_id.len() <= 64
            && req
                .device_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-');
        if req.v != PAIR_VERSION
            || !id_ok
            || !valid_text(&req.name, 80)
            || !valid_text(&req.platform, 16)
        {
            return Err(PairError::BadRequest);
        }
        let dev_pub: [u8; 32] = unb64_array(&req.pub_key)?;
        let proof = unb64(&req.proof)?;

        let Some(offer) = self.offers.iter_mut().find(|o| {
            hmac_verify(
                &o.uri.secret,
                &proof_parts(
                    &dev_pub,
                    &req.device_id,
                    &req.name,
                    &req.platform,
                    &o.uri.hub_pub,
                ),
                &proof,
            )
        }) else {
            return Err(PairError::BadProof);
        };
        if offer.used {
            return Err(PairError::AlreadyUsed);
        }
        if now >= offer.expires_at {
            return Err(PairError::Expired);
        }
        offer.used = true;

        let dev = PublicKey::from(dev_pub);
        let shared = offer.secret.diffie_hellman(&dev);
        if !shared.was_contributory() {
            return Err(PairError::BadRequest);
        }
        let k = pair_key(
            shared.as_bytes(),
            &offer.uri.secret,
            &offer.uri.hub_pub,
            &dev_pub,
        );
        let token = SecretKey::generate();
        let grant = serde_json::to_vec(&PairGrant {
            master_key: b64(master.as_bytes()),
            token: b64(token.as_bytes()),
        })
        .map_err(|_| PairError::BadRequest)?;
        let packed = k
            .seal_packed(&grant_aad(space_id, &req.device_id), &grant)
            .map_err(|_| PairError::BadRequest)?;
        Ok((
            PairResponse {
                v: PAIR_VERSION,
                space_id: space_id.to_owned(),
                hub_device_id: hub_device_id.to_owned(),
                hub_name: hub_name.to_owned(),
                grant: b64(&packed),
            },
            AcceptedDevice {
                device_id: req.device_id.clone(),
                name: req.name.trim().to_owned(),
                platform: req.platform.trim().to_lowercase(),
                token,
                check_code: check_code(&k),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(now: Millis) -> (PairOffers, PairUri) {
        let mut offers = PairOffers::default();
        let uri = offers.create(
            vec!["192.168.1.5:47821".into(), "10.0.0.2:47821".into()],
            "My PC",
            now,
        );
        (offers, uri)
    }

    fn accept(
        offers: &mut PairOffers,
        req: &PairRequest,
        now: Millis,
        master: &SecretKey,
    ) -> Result<(PairResponse, AcceptedDevice), PairError> {
        offers.accept(req, now, "SPACE", "HUB", "My PC", master)
    }

    #[test]
    fn uri_roundtrip() {
        let (_, uri) = offer(0);
        let s = uri.to_uri();
        assert!(s.starts_with("cliptrail://pair?v=1&h=192.168.1.5:47821,10.0.0.2:47821&k="));
        assert!(s.ends_with("&n=My%20PC"), "{s}");
        assert_eq!(PairUri::parse(&s).unwrap(), uri);
        assert_eq!(PairUri::parse("https://x"), Err(PairError::BadUri));
        assert_eq!(
            PairUri::parse("cliptrail://pair?v=2&h=a:1"),
            Err(PairError::BadUri)
        );
        assert_eq!(pct_decode("a%2"), "a%2");
        assert_eq!(pct_decode("%E2%9C%93ok"), "✓ok");
    }

    #[test]
    fn good_proof_pairs_and_both_sides_agree() {
        let (mut offers, uri) = offer(1000);
        let master = SecretKey::generate();
        let client = ClientPairing::new(
            PairUri::parse(&uri.to_uri()).unwrap(),
            "PHONE1",
            "Pixel",
            "android",
        );
        let (resp, dev) = accept(&mut offers, client.request(), 2000, &master).unwrap();
        assert_eq!(dev.device_id, "PHONE1");
        assert_eq!(dev.platform, "android");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains(&b64(master.as_bytes())));
        let result = client.finish(&resp).unwrap();
        assert_eq!(result.master_key, master);
        assert_eq!(result.token, dev.token);
        assert_eq!(result.check_code, dev.check_code);
        assert_eq!(result.check_code.len(), 4);
        assert_eq!(result.space_id, "SPACE");
        assert!(offers.active(2000).is_none(), "offer burned");
    }

    #[test]
    fn bad_proof_is_rejected() {
        let (mut offers, uri) = offer(0);
        let master = SecretKey::generate();
        let mut wrong = uri.clone();
        wrong.secret = random_bytes();
        let client = ClientPairing::new(wrong, "PHONE1", "Pixel", "android");
        assert_eq!(
            accept(&mut offers, client.request(), 1, &master).err(),
            Some(PairError::BadProof)
        );
        // A good proof, then the name swapped in transit.
        let client = ClientPairing::new(uri, "PHONE1", "Pixel", "android");
        let mut req = client.request().clone();
        req.name = "Evil".into();
        assert_eq!(
            accept(&mut offers, &req, 1, &master).err(),
            Some(PairError::BadProof)
        );
        assert!(
            offers.active(1).is_some(),
            "failed attempts don't burn the offer"
        );
        assert!(accept(&mut offers, client.request(), 1, &master).is_ok());
    }

    #[test]
    fn replayed_secret_is_rejected() {
        let (mut offers, uri) = offer(0);
        let master = SecretKey::generate();
        let client = ClientPairing::new(uri.clone(), "PHONE1", "Pixel", "android");
        accept(&mut offers, client.request(), 1, &master).unwrap();
        assert_eq!(
            accept(&mut offers, client.request(), 2, &master).err(),
            Some(PairError::AlreadyUsed)
        );
        let other = ClientPairing::new(uri, "PHONE2", "Other", "android");
        assert_eq!(
            accept(&mut offers, other.request(), 3, &master).err(),
            Some(PairError::AlreadyUsed)
        );
    }

    #[test]
    fn expired_offer_is_rejected() {
        let (mut offers, uri) = offer(0);
        let master = SecretKey::generate();
        let client = ClientPairing::new(uri, "PHONE1", "Pixel", "android");
        assert_eq!(
            accept(&mut offers, client.request(), OFFER_TTL_MS, &master).err(),
            Some(PairError::Expired)
        );
    }

    #[test]
    fn tampered_reply_and_rate_limit() {
        let (mut offers, uri) = offer(0);
        let master = SecretKey::generate();
        let client = ClientPairing::new(uri, "PHONE1", "Pixel", "android");
        let (mut resp, _) = accept(&mut offers, client.request(), 1, &master).unwrap();
        resp.space_id = "OTHER".into();
        assert_eq!(client.finish(&resp).err(), Some(PairError::BadResponse));

        let (mut offers, uri) = offer(0);
        let mut bad = uri.clone();
        bad.secret = random_bytes();
        let c = ClientPairing::new(bad, "X", "x", "linux");
        for _ in 0..MAX_PAIR_ATTEMPTS_PER_MIN {
            assert_eq!(
                accept(&mut offers, c.request(), 5, &master).err(),
                Some(PairError::BadProof)
            );
        }
        let good = ClientPairing::new(uri, "Y", "y", "linux");
        assert_eq!(
            accept(&mut offers, good.request(), 6, &master).err(),
            Some(PairError::RateLimited)
        );
        assert!(accept(&mut offers, good.request(), 70_000, &master).is_ok());
    }
}
