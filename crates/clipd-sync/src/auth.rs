//! Request authentication. v1 runs over plain HTTP on the LAN (every
//! payload is sealed), so the device token never travels: each request
//! carries an HMAC over method, path, time and body hash instead.
//!
//! `Authorization: CT1 device=<id>, ts=<unix ms>, mac=<b64url HMAC-SHA256>`

use clipd_core::Millis;

use crate::crypto::{b64, hmac_sha256, hmac_verify, sha256, unb64};

pub const SCHEME: &str = "CT1";
/// Accepted clock difference between device and hub.
pub const MAX_SKEW_MS: Millis = 2 * 60 * 1000;

fn parts<'a>(
    device_id: &'a str,
    method: &'a str,
    path_and_query: &'a str,
    ts: &'a [u8],
    body_hash: &'a [u8],
) -> [&'a [u8]; 6] {
    [
        b"cliptrail-auth-v1",
        device_id.as_bytes(),
        method.as_bytes(),
        path_and_query.as_bytes(),
        ts,
        body_hash,
    ]
}

/// Header value for a request.
pub fn sign(
    device_id: &str,
    token: &[u8],
    method: &str,
    path_and_query: &str,
    ts: Millis,
    body: &[u8],
) -> String {
    let ts_s = ts.to_string();
    let body_hash = sha256(body);
    let mac = hmac_sha256(
        token,
        &parts(
            device_id,
            method,
            path_and_query,
            ts_s.as_bytes(),
            &body_hash,
        ),
    );
    format!("{SCHEME} device={device_id}, ts={ts_s}, mac={}", b64(&mac))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthHeader {
    pub device_id: String,
    pub ts: Millis,
    pub mac: Vec<u8>,
}

impl AuthHeader {
    pub fn parse(value: &str) -> Option<AuthHeader> {
        let rest = value.trim().strip_prefix(SCHEME)?.trim_start();
        let (mut device, mut ts, mut mac) = (None, None, None);
        for field in rest.split(',') {
            let (k, v) = field.trim().split_once('=')?;
            match k.trim() {
                "device" => device = Some(v.trim().to_owned()),
                "ts" => ts = v.trim().parse::<Millis>().ok(),
                "mac" => mac = unb64(v.trim()).ok(),
                _ => {}
            }
        }
        let device_id = device.filter(|d| {
            !d.is_empty()
                && d.len() <= 64
                && d.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })?;
        Some(AuthHeader {
            device_id,
            ts: ts?,
            mac: mac.filter(|m| m.len() == 32)?,
        })
    }

    pub fn fresh(&self, now: Millis) -> bool {
        (now - self.ts).abs() <= MAX_SKEW_MS
    }

    pub fn verify(&self, token: &[u8], method: &str, path_and_query: &str, body: &[u8]) -> bool {
        let ts_s = self.ts.to_string();
        let body_hash = sha256(body);
        hmac_verify(
            token,
            &parts(
                &self.device_id,
                method,
                path_and_query,
                ts_s.as_bytes(),
                &body_hash,
            ),
            &self.mac,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_parse_verify() {
        let token = [9u8; 32];
        let h = sign("DEV1", &token, "POST", "/v1/events", 1000, b"{}");
        let a = AuthHeader::parse(&h).unwrap();
        assert_eq!(a.device_id, "DEV1");
        assert!(a.verify(&token, "POST", "/v1/events", b"{}"));
        assert!(!a.verify(&token, "POST", "/v1/events", b"{ }"));
        assert!(!a.verify(&token, "GET", "/v1/events", b"{}"));
        assert!(!a.verify(&token, "POST", "/v1/events?since=0", b"{}"));
        assert!(!a.verify(&[8u8; 32], "POST", "/v1/events", b"{}"));
        assert!(a.fresh(1000 + MAX_SKEW_MS) && !a.fresh(1001 + MAX_SKEW_MS));
        // Changing the claimed device breaks the MAC.
        let mut other = a.clone();
        other.device_id = "DEV2".into();
        assert!(!other.verify(&token, "POST", "/v1/events", b"{}"));
    }

    #[test]
    fn rejects_garbage() {
        for h in [
            "",
            "Bearer abc",
            "CT1 device=, ts=1, mac=AAAA",
            "CT1 device=a b, ts=1, mac=AAAA",
            "CT1 device=x, ts=nope, mac=AAAA",
            "CT1 device=x, ts=1, mac=short",
        ] {
            assert!(AuthHeader::parse(h).is_none(), "{h}");
        }
    }
}
