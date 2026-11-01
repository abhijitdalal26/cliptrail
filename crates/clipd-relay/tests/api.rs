//! The relay over real HTTP (in-process, random port): registration,
//! enrollment, request auth (valid / bad / replayed / stale / revoked),
//! enroll-key rotation, invite codes, size limits, and that nothing the
//! relay stores contains plaintext.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use clipd_relay::{Registration, RelayConfig, RelayHandle};
use clipd_sync::crypto::{SecretKey, SpaceKeys, b64};
use clipd_sync::envelope::{ClipAdded, Envelope, Event, blob_aad};
use clipd_sync::relay::{self, DeviceKey, RelayPushItem};
use clipd_sync::relay_link::{RelayConn, RelayDenied, http_builder};

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("clipd-relay-{tag}-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn relay_with(dir: &Path, f: impl FnOnce(&mut RelayConfig)) -> RelayHandle {
    let mut cfg = RelayConfig::new(SocketAddr::from(([127, 0, 0, 1], 0)), dir.to_path_buf());
    f(&mut cfg);
    clipd_relay::start(cfg).await.unwrap()
}

fn conn(url: &str, space: &str, dev: &str) -> (RelayConn, [u8; 32]) {
    let key = DeviceKey::generate();
    let seed = key.seed();
    (RelayConn::new(url, space, dev, key).unwrap(), seed)
}

fn denied(e: &anyhow::Error) -> bool {
    e.is::<RelayDenied>()
}

const SECRET_TEXT: &str = "PLAINTEXT-CANARY correct horse battery staple 4111-1111";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_enroll_revoke_and_limits() {
    let dir = TempDir::new("auth");
    let r = relay_with(&dir.0, |c| {
        c.max_blob_bytes = 1000;
        c.max_devices = 3;
    })
    .await;
    let url = r.local_url();
    let http = http_builder().build().unwrap();

    let h = clipd_sync::relay_link::hello(&http, &url).await.unwrap();
    assert_eq!(
        (h.kind.as_str(), h.registration.as_str()),
        ("clipd-relay", "open")
    );
    assert_eq!(h.limits.max_blob_bytes, 1000);

    // Register; a second registration of the same space is refused.
    let enroll = DeviceKey::generate();
    let (d1, d1_seed) = conn(&url, "SPACE1", "DEV1");
    d1.register(&enroll, None).await.unwrap();
    let (other, _) = conn(&url, "SPACE1", "DEV9");
    let e = other
        .register(&DeviceKey::generate(), None)
        .await
        .unwrap_err();
    assert!(format!("{e:#}").contains("already registered"), "{e:#}");

    // Signed requests: valid, replayed, tampered, unknown device, stale.
    let d1_key = DeviceKey::from_seed(&d1_seed).unwrap();
    let pq = "/v1/events?since=0";
    let sig = relay::sign_request(
        "SPACE1",
        "DEV1",
        &d1_key,
        "GET",
        pq,
        clipd_core::now_millis(),
        b"",
    );
    let get = |path: &str, sig: &str| {
        http.get(format!("{url}{path}"))
            .header("authorization", sig.to_owned())
            .send()
    };
    assert_eq!(get(pq, &sig).await.unwrap().status(), 200);
    let resp = get(pq, &sig).await.unwrap();
    assert_eq!(resp.status(), 401);
    assert!(resp.text().await.unwrap().contains("replayed"));
    let sig = relay::sign_request(
        "SPACE1",
        "DEV1",
        &d1_key,
        "GET",
        pq,
        clipd_core::now_millis(),
        b"",
    );
    assert_eq!(get("/v1/events?since=5", &sig).await.unwrap().status(), 401);
    let stranger = DeviceKey::generate();
    let sig = relay::sign_request(
        "SPACE1",
        "DEV1",
        &stranger,
        "GET",
        pq,
        clipd_core::now_millis(),
        b"",
    );
    assert_eq!(get(pq, &sig).await.unwrap().status(), 401, "wrong key");
    let sig = relay::sign_request(
        "SPACE1",
        "NOBODY",
        &stranger,
        "GET",
        pq,
        clipd_core::now_millis(),
        b"",
    );
    assert_eq!(get(pq, &sig).await.unwrap().status(), 401, "unknown device");
    let stale = clipd_core::now_millis() - 10 * 60 * 1000;
    let sig = relay::sign_request("SPACE1", "DEV1", &d1_key, "GET", pq, stale, b"");
    let resp = get(pq, &sig).await.unwrap();
    assert_eq!(resp.status(), 401);
    assert!(resp.text().await.unwrap().contains("clock"));
    assert_eq!(get(pq, "Bearer x").await.unwrap().status(), 401);

    // Enroll with the right / wrong enroll key.
    let (d2, _) = conn(&url, "SPACE1", "DEV2");
    d2.enroll(&enroll).await.unwrap();
    let (d3, _) = conn(&url, "SPACE1", "DEV3");
    let e = d3.enroll(&DeviceKey::generate()).await.unwrap_err();
    assert!(denied(&e), "{e:#}");
    d2.events(0, 10).await.unwrap();

    // Push from D2 is visible to D1; duplicates are idempotent.
    let env = Envelope {
        v: 1,
        event_id: "01JTESTEVENT0000000000000A".into(),
        device_id: "DEV2".into(),
        ts: 1,
        nonce: "A".repeat(32),
        ct: "B".repeat(64),
    };
    let item = RelayPushItem {
        env: env.clone(),
        tag: Some("tagtagtagtagtagtagtagt".into()),
        kind: Some("ClipAdded".into()),
        blob: None,
    };
    let pr = d2.push(vec![item.clone(), item.clone()]).await.unwrap();
    assert_eq!(pr.accepted.len(), 2);
    assert_eq!(pr.head_seq, 1);
    let page = d1.events(0, 10).await.unwrap();
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].env, env);
    // Bad metadata is rejected per item.
    let mut bad = item.clone();
    bad.env.event_id = "01JTESTEVENT0000000000000B".into();
    bad.kind = Some("Nope".into());
    let pr = d2.push(vec![bad]).await.unwrap();
    assert_eq!(pr.rejected.len(), 1);

    // Blobs: size limit, roundtrip, 404.
    let blob_id = "AAAAAAAAAAAAAAAAAAAAAA";
    let e = d1.put_blob(blob_id, vec![7u8; 2000]).await.unwrap_err();
    assert!(format!("{e:#}").contains("too large"), "{e:#}");
    d1.put_blob(blob_id, vec![7u8; 500]).await.unwrap();
    assert_eq!(d2.get_blob(blob_id).await.unwrap(), Some(vec![7u8; 500]));
    assert_eq!(d2.get_blob("BBBBBBBBBBBBBBBBBBBBBB").await.unwrap(), None);

    // Device cap.
    let (d4, _) = conn(&url, "SPACE1", "DEV4");
    d4.enroll(&enroll).await.unwrap();
    let (d5, _) = conn(&url, "SPACE1", "DEV5");
    let e = d5.enroll(&enroll).await.unwrap_err();
    assert!(format!("{e:#}").contains("too many devices"), "{e:#}");

    // Revoke D2: every request fails, re-enrolling too; rotate the enroll
    // key so the old one can't add new devices.
    d1.revoke("DEV2").await.unwrap();
    let e = d2.events(0, 10).await.unwrap_err();
    assert!(denied(&e), "{e:#}");
    let e = d2.enroll(&enroll).await.unwrap_err();
    assert!(denied(&e), "{e:#}");
    let new_enroll = DeviceKey::generate();
    d1.rotate_enroll(&new_enroll).await.unwrap();
    let e = d5.enroll(&enroll).await.unwrap_err();
    assert!(denied(&e), "old enroll key: {e:#}");
    d5.enroll(&new_enroll).await.unwrap();
    // A device can leave on its own.
    d4.leave().await.unwrap();
    assert!(denied(&d4.events(0, 1).await.unwrap_err()));

    let info = d1.space_info().await.unwrap();
    assert_eq!(info.envelopes, 1);
    assert_eq!(info.blobs, 1);
    assert!(
        info.devices
            .iter()
            .any(|d| d.device_id == "DEV2" && d.revoked_at.is_some())
    );

    // Deleting the space wipes it.
    d1.delete_space().await.unwrap();
    assert!(denied(&d1.events(0, 1).await.unwrap_err()));
    r.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_and_closed_registration() {
    let dir = TempDir::new("invite");
    let r = relay_with(&dir.0, |c| {
        c.registration = Registration::Invite("letmein".into())
    })
    .await;
    let url = r.local_url();
    let (d1, _) = conn(&url, "SPACE2", "DEV1");
    for code in [None, Some("wrong")] {
        let e = d1.register(&DeviceKey::generate(), code).await.unwrap_err();
        assert!(format!("{e:#}").contains("invite"), "{e:#}");
    }
    d1.register(&DeviceKey::generate(), Some("letmein"))
        .await
        .unwrap();
    r.shutdown().await;

    let dir = TempDir::new("closed");
    let r = relay_with(&dir.0, |c| c.registration = Registration::Closed).await;
    let (d1, _) = conn(&r.local_url(), "SPACE3", "DEV1");
    let e = d1.register(&DeviceKey::generate(), None).await.unwrap_err();
    assert!(format!("{e:#}").contains("does not accept"), "{e:#}");
    r.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_never_stores_plaintext() {
    let dir = TempDir::new("plain");
    let r = relay_with(&dir.0, |_| {}).await;
    let url = r.local_url();
    let keys = SpaceKeys::new(SecretKey::generate());
    let space_id = "01JSPACE00000000000000000Z";
    let rsid = relay::relay_space_id(&keys, space_id);
    assert!(!rsid.contains(space_id));
    let (d1, _) = conn(&url, &rsid, "DEV1");
    d1.register(&DeviceKey::generate(), None).await.unwrap();

    let hash = clipd_sync::crypto::sha256(SECRET_TEXT.as_bytes());
    let blob_id = keys.blob_id(&hash);
    let event = Event::ClipAdded(Box::new(ClipAdded {
        hash: b64(&hash),
        kind: clipd_core::Kind::Text,
        subkind: None,
        text: Some(SECRET_TEXT.into()),
        html: Some(format!("<b>{SECRET_TEXT}</b>")),
        files: vec![],
        source_app: Some("secret-app.exe".into()),
        origin: "DEV1".into(),
        created_at: 1,
        last_seen_at: 1,
        copy_count: 1,
        image: None,
    }));
    let env = Envelope::seal(&keys, space_id, "DEV1", 1, &event).unwrap();
    let tag = keys.hash_tag(&hash);
    d1.push(vec![RelayPushItem {
        env,
        tag: Some(tag.clone()),
        kind: Some("ClipAdded".into()),
        blob: Some(blob_id.clone()),
    }])
    .await
    .unwrap();
    let png = format!("fake png bytes {SECRET_TEXT}");
    let sealed = keys
        .blobs
        .seal_packed(&blob_aad(space_id, &blob_id), png.as_bytes())
        .unwrap();
    d1.put_blob(&blob_id, sealed).await.unwrap();
    r.shutdown().await;

    // Everything on disk (db + WAL), raw bytes: no plaintext, no content
    // hash, no space id.
    let mut all = Vec::new();
    for entry in std::fs::read_dir(&dir.0).unwrap() {
        all.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    assert!(all.len() > 1000, "relay stored something");
    let hay = String::from_utf8_lossy(&all);
    for needle in [
        SECRET_TEXT,
        "PLAINTEXT-CANARY",
        "battery staple",
        "secret-app",
        space_id,
        &b64(&hash),
    ] {
        assert!(!hay.contains(needle), "relay stored {needle:?}");
    }
    assert!(hay.contains(&tag), "sanity: the opaque tag is there");
}
