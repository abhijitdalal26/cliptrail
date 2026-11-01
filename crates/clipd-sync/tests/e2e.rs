#![cfg(feature = "hub")]

//! Hub + two clients in one process (temp dirs, random port): add, pin,
//! delete and images both ways, relay client → hub → client, offline
//! catch-up, no echo, revoke.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clipd_core::{
    IngestOutcome, Kind, ListQuery, Pipeline, PipelineConfig, RawClip, RawContent, now_millis,
};
use clipd_sync::crypto::{SecretKey, SpaceKeys};
use clipd_sync::db::SyncDb;
use clipd_sync::envelope::Event;
use clipd_sync::keystore::MASTER_KEY;
use clipd_sync::{ClientConfig, HubConfig, KeyStore, MemoryKeyStore, client, hub};

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("clipd-e2e-{tag}-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn pipeline(dir: &Path) -> Pipeline {
    Pipeline::open(PipelineConfig::with_defaults(dir.to_path_buf()).unwrap()).unwrap()
}

fn text(p: &Pipeline, s: &str) -> String {
    match p
        .ingest(RawClip {
            content: RawContent::Text {
                text: s.into(),
                html: None,
            },
            source_app: Some("notepad.exe".into()),
            captured_at: now_millis(),
        })
        .unwrap()
    {
        IngestOutcome::Stored { id, .. } | IngestOutcome::Bumped { id, .. } => id,
        o => panic!("{o:?}"),
    }
}

fn image(p: &Pipeline, seed: u8) -> String {
    let (w, h) = (120u32, 60u32);
    let rgba = (0..w * h * 4)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    match p
        .ingest(RawClip {
            content: RawContent::Image {
                width: w,
                height: h,
                rgba,
            },
            source_app: Some("snippingtool.exe".into()),
            captured_at: now_millis(),
        })
        .unwrap()
    {
        IngestOutcome::Stored { id, .. } => id,
        o => panic!("{o:?}"),
    }
}

fn find(p: &Pipeline, needle: &str) -> Option<clipd_core::ClipSummary> {
    p.store()
        .list(&ListQuery {
            limit: Some(1000),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .find(|c| c.preview == needle)
}

async fn wait(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn start_client(dir: &Path, ks: &Arc<MemoryKeyStore>) -> clipd_sync::ClientHandle {
    client::start(ClientConfig {
        data_dir: dir.to_path_buf(),
        keystore: ks.clone(),
        fetch_images: true,
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hub_and_clients_sync_everything() {
    let (hub_dir, a_dir, b_dir) = (TempDir::new("hub"), TempDir::new("a"), TempDir::new("b"));
    let (hub_ks, a_ks, b_ks) = (
        Arc::new(MemoryKeyStore::default()),
        Arc::new(MemoryKeyStore::default()),
        Arc::new(MemoryKeyStore::default()),
    );
    let hp = pipeline(&hub_dir.0);
    let ap = pipeline(&a_dir.0);
    let bp = pipeline(&b_dir.0);

    // History from before pairing is shared both ways (backfill).
    text(&hp, "hub history");
    text(&ap, "client history");
    // Sensitive clips never leave the device.
    text(&hp, "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8");

    let hub = hub::start(HubConfig {
        data_dir: hub_dir.0.clone(),
        keystore: hub_ks.clone(),
        port: 0,
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        advertise: Some(vec!["127.0.0.1".into()]),
        name: "Test Hub".into(),
    })
    .await
    .unwrap();
    let mut notices = hub.subscribe();

    // Pair A (and check the check codes agree).
    let offer = hub.create_offer();
    assert!(offer.uri.contains(&format!("127.0.0.1:{}", hub.port())));
    let out = client::pair(&a_dir.0, a_ks.clone(), &offer.uri, "Client A", "windows")
        .await
        .unwrap();
    assert_eq!(out.hub_name, "Test Hub");
    assert_eq!(out.backfilled, 1);
    let paired_code = loop {
        if let Ok(clipd_sync::SyncNotice::Paired { check_code, .. }) = notices.recv().await {
            break check_code;
        }
    };
    assert_eq!(paired_code, out.check_code);
    // The same QR can't be used twice.
    let err = client::pair(&b_dir.0, b_ks.clone(), &offer.uri, "Client B", "android")
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("already used"), "{err:#}");
    let offer_b = hub.create_offer();
    client::pair(&b_dir.0, b_ks.clone(), &offer_b.uri, "Client B", "android")
        .await
        .unwrap();

    let a = start_client(&a_dir.0, &a_ks);
    let b = start_client(&b_dir.0, &b_ks);

    wait("backfill reaches A and B", || {
        find(&ap, "hub history").is_some()
            && find(&bp, "hub history").is_some()
            && find(&bp, "client history").is_some()
            && find(&hp, "client history").is_some()
    })
    .await;
    assert!(find(&ap, "••••••").is_none() && find(&bp, "••••••").is_none());
    let remote = find(&ap, "hub history").unwrap();
    assert_eq!(
        remote.device_id,
        hp.config().device_id,
        "origin device kept"
    );

    // Add: hub → clients within ~1 s, client → hub → other client.
    let t0 = std::time::Instant::now();
    text(&hp, "from hub");
    hub.notify_local_change();
    wait("hub clip on A", || find(&ap, "from hub").is_some()).await;
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    text(&ap, "from A");
    a.notify_local_change();
    wait("A clip on hub and B", || {
        find(&hp, "from A").is_some() && find(&bp, "from A").is_some()
    })
    .await;

    // Pin both ways.
    let id = find(&hp, "from A").unwrap().id;
    hp.store().set_pinned(&id, true).unwrap();
    wait("pin on A and B", || {
        find(&ap, "from A").is_some_and(|c| c.pinned)
            && find(&bp, "from A").is_some_and(|c| c.pinned)
    })
    .await;
    let id = find(&bp, "from hub").unwrap().id;
    bp.store().set_pinned(&id, true).unwrap();
    wait("pin from B on hub and A", || {
        find(&hp, "from hub").is_some_and(|c| c.pinned)
            && find(&ap, "from hub").is_some_and(|c| c.pinned)
    })
    .await;

    // Delete both ways.
    let id = find(&ap, "from hub").unwrap().id;
    ap.delete(&id).unwrap();
    wait("delete from A everywhere", || {
        find(&hp, "from hub").is_none() && find(&bp, "from hub").is_none()
    })
    .await;
    let id = find(&hp, "hub history").unwrap().id;
    hp.delete(&id).unwrap();
    wait("delete from hub everywhere", || {
        find(&ap, "hub history").is_none() && find(&bp, "hub history").is_none()
    })
    .await;

    // Images: thumbnail inline, full image fetched by blob id.
    let hub_img = image(&hp, 3);
    let hub_hash = hp.store().content_hash(&hub_img).unwrap().unwrap();
    wait("hub image fully on A and B", || {
        [&ap, &bp].iter().all(|p| {
            p.store()
                .get_by_hash(&hub_hash)
                .unwrap()
                .is_some_and(|c| c.clip.image_path.is_some() && c.clip.summary.thumb_path.is_some())
        })
    })
    .await;
    let full_a = ap.store().get_by_hash(&hub_hash).unwrap().unwrap().clip;
    let full_hub = hp.store().get(&hub_img).unwrap().unwrap();
    assert_eq!(
        std::fs::read(full_a.image_path.unwrap()).unwrap(),
        std::fs::read(full_hub.image_path.unwrap()).unwrap()
    );
    assert_eq!(full_a.summary.kind, Kind::Image);
    let a_img = image(&ap, 7);
    let a_hash = ap.store().content_hash(&a_img).unwrap().unwrap();
    wait("A image fully on hub and B", || {
        [&hp, &bp].iter().all(|p| {
            p.store()
                .get_by_hash(&a_hash)
                .unwrap()
                .is_some_and(|c| c.clip.image_path.is_some())
        })
    })
    .await;

    // Offline catch-up: A goes away, both sides keep changing.
    a.shutdown().await;
    text(&hp, "while A was away");
    let id = find(&hp, "from A").unwrap().id;
    hp.store().set_pinned(&id, false).unwrap();
    text(&ap, "A offline copy");
    hub.notify_local_change();
    wait("hub change reaches B", || {
        find(&bp, "while A was away").is_some()
    })
    .await;
    assert!(find(&ap, "while A was away").is_none());
    let a = start_client(&a_dir.0, &a_ks);
    wait("catch-up both ways", || {
        find(&ap, "while A was away").is_some()
            && find(&ap, "from A").is_some_and(|c| !c.pinned)
            && find(&hp, "A offline copy").is_some()
            && find(&bp, "A offline copy").is_some()
    })
    .await;
    let st = a.status().unwrap();
    assert!(st.connected);
    assert_eq!(st.outbox, 0);
    assert!(st.devices.iter().any(|d| d.name == "Test Hub"));

    // No echo: every envelope's sender is the device the change came from.
    let master = SecretKey::from_slice(&hub_ks.get(MASTER_KEY).unwrap().unwrap()).unwrap();
    let keys = SpaceKeys::new(master);
    let db = SyncDb::open(&hub_dir.0.join("sync.db")).unwrap();
    let space = db.space().unwrap().unwrap();
    let a_id = ap.config().device_id.clone();
    for e in db.envelopes_since(0, 10_000).unwrap() {
        if let Event::ClipAdded(c) = e.env.open(&keys, &space.space_id).unwrap() {
            assert_eq!(c.origin, e.env.device_id, "re-broadcast of a synced clip");
            assert_ne!(c.kind, Kind::Sensitive);
        }
    }
    let from_a = db
        .envelopes_since(0, 10_000)
        .unwrap()
        .into_iter()
        .filter(|e| e.env.device_id == a_id)
        .count();
    assert!(from_a >= 3);

    // Revoke B: its token dies, its stream closes, it reports unpaired.
    let b_id = bp.config().device_id.clone();
    assert!(hub.revoke(&b_id).unwrap());
    wait("B notices the revoke", || b.is_unpaired()).await;
    text(&hp, "after revoke");
    hub.notify_local_change();
    wait("A still syncs", || find(&ap, "after revoke").is_some()).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(find(&bp, "after revoke").is_none());

    a.shutdown().await;
    b.shutdown().await;
    hub.shutdown().await;
}
