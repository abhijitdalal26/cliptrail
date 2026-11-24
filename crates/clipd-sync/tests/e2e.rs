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
            source_url: None,
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
            source_url: None,
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
    // Catch-up can finish a moment before the client marks itself connected.
    wait("A connected", || a.status().is_ok_and(|s| s.connected)).await;
    let st = a.status().unwrap();
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

    // A leaves on its own (the phone's "Unpair"): the hub revokes it, A's
    // sync state and keys are gone, its clips stay.
    a.shutdown().await;
    let a_id = ap.config().device_id.clone();
    assert!(client::unpair(&a_dir.0, a_ks.clone()).await.unwrap());
    let st = clipd_sync::status(&hub_dir.0, &*hub_ks).unwrap();
    let a_row = st.devices.iter().find(|d| d.id == a_id).unwrap();
    assert!(a_row.revoked_at.is_some());
    assert!(!clipd_sync::status(&a_dir.0, &*a_ks).unwrap().configured);
    assert!(a_ks.get(MASTER_KEY).unwrap().is_none());
    assert!(find(&ap, "after revoke").is_some());
    // Unpairing an unpaired data dir is a no-op.
    assert!(!client::unpair(&a_dir.0, a_ks.clone()).await.unwrap());

    b.shutdown().await;
    hub.shutdown().await;
}

/// Collections and which clips are in them sync both ways, including to a
/// device that reads the log after compaction moved a clip's add behind
/// its pin / collection event.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn collections_sync() {
    use clipd_core::CollectionFields;
    let (hub_dir, a_dir) = (TempDir::new("colhub"), TempDir::new("cola"));
    let (hub_ks, a_ks) = (
        Arc::new(MemoryKeyStore::default()),
        Arc::new(MemoryKeyStore::default()),
    );
    let hp = pipeline(&hub_dir.0);
    let ap = pipeline(&a_dir.0);
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

    // On the hub before A pairs: a pinned clip in "Work", copied again
    // afterwards so its newest add is logged after its state.
    let work = hp
        .create_collection(CollectionFields {
            name: "Work".into(),
            icon: Some("💼".into()),
            color: Some("blue".into()),
        })
        .unwrap();
    let w = text(&hp, "in work");
    hp.store().set_collection(&w, Some(&work.id)).unwrap();
    hp.store().set_pinned(&w, true).unwrap();
    hub.notify_local_change();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    text(&hp, "in work");
    hub.notify_local_change();
    tokio::time::sleep(Duration::from_millis(1200)).await;

    let offer = hub.create_offer();
    client::pair(&a_dir.0, a_ks.clone(), &offer.uri, "Phone", "android")
        .await
        .unwrap();
    let a = start_client(&a_dir.0, &a_ks);
    wait("collection and its clip on A", || {
        ap.store()
            .collections()
            .unwrap()
            .iter()
            .any(|c| c.id == work.id && c.name == "Work" && c.count == 1)
            && find(&ap, "in work")
                .is_some_and(|c| c.pinned && c.collection_id.as_deref() == Some(work.id.as_str()))
    })
    .await;

    // A makes a collection and moves the clip into it.
    let home = ap
        .create_collection(CollectionFields {
            name: "Home".into(),
            ..Default::default()
        })
        .unwrap();
    let id = find(&ap, "in work").unwrap().id;
    ap.store().set_collection(&id, Some(&home.id)).unwrap();
    a.notify_local_change();
    wait("A's collection and move on the hub", || {
        hp.store().collection(&home.id).unwrap().is_some()
            && find(&hp, "in work")
                .is_some_and(|c| c.collection_id.as_deref() == Some(home.id.as_str()) && c.pinned)
    })
    .await;

    // Rename on the hub; delete on A (its clips stay, outside it).
    hp.update_collection(
        &work.id,
        CollectionFields {
            name: "Office".into(),
            ..Default::default()
        },
    )
    .unwrap();
    hub.notify_local_change();
    wait("rename on A", || {
        ap.store()
            .collection(&work.id)
            .unwrap()
            .is_some_and(|c| c.name == "Office")
    })
    .await;
    ap.delete_collection(&home.id).unwrap();
    a.notify_local_change();
    wait("delete on the hub", || {
        hp.store().collection(&home.id).unwrap().is_none()
            && find(&hp, "in work").is_some_and(|c| c.collection_id.is_none() && c.pinned)
    })
    .await;

    a.shutdown().await;
    hub.shutdown().await;
}

/// "Try now" skips the backoff: after the hub was away long enough for the
/// client to wait several seconds between attempts, `reconnect_now`
/// connects at once when the hub is back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconnect_now_skips_the_backoff() {
    let (hub_dir, a_dir) = (TempDir::new("rnhub"), TempDir::new("rna"));
    let (hub_ks, a_ks) = (
        Arc::new(MemoryKeyStore::default()),
        Arc::new(MemoryKeyStore::default()),
    );
    let cfg = |port| HubConfig {
        data_dir: hub_dir.0.clone(),
        keystore: hub_ks.clone(),
        port,
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        advertise: Some(vec!["127.0.0.1".into()]),
        name: "Test Hub".into(),
    };
    let hub = hub::start(cfg(0)).await.unwrap();
    let port = hub.port();
    let offer = hub.create_offer();
    client::pair(&a_dir.0, a_ks.clone(), &offer.uri, "Phone", "android")
        .await
        .unwrap();
    let a = start_client(&a_dir.0, &a_ks);
    wait("connected", || a.status().is_ok_and(|s| s.connected)).await;

    // Hub away: failed attempts back off 1 s, 2 s, 4 s, then 8 s.
    hub.shutdown().await;
    wait("disconnected", || a.status().is_ok_and(|s| !s.connected)).await;
    tokio::time::sleep(Duration::from_millis(7500)).await;

    let hub = hub::start(cfg(port)).await.unwrap();
    let t0 = std::time::Instant::now();
    a.reconnect_now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while !a.status().is_ok_and(|s| s.connected) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "not reconnected within 3 s of reconnect_now"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(t0.elapsed() < Duration::from_secs(3));

    a.shutdown().await;
    hub.shutdown().await;
}

/// A phone that pairs again (reinstall, or unpair + pair) replaces its old
/// entry instead of being listed twice, and a dead remembered address
/// doesn't keep a client from the hub.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pairing_again_replaces_the_old_entry() {
    let hub_dir = TempDir::new("rehub");
    let hub_ks = Arc::new(MemoryKeyStore::default());
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
    let pair = async |dir: &TempDir, ks: &Arc<MemoryKeyStore>, name: &str| {
        let offer = hub.create_offer();
        client::pair(&dir.0, ks.clone(), &offer.uri, name, "android")
            .await
            .unwrap();
        pipeline(&dir.0).config().device_id.clone()
    };
    let active = |hub: &hub::HubHandle| -> Vec<String> {
        hub.status()
            .unwrap()
            .devices
            .into_iter()
            .filter(|d| !d.is_self && d.revoked_at.is_none())
            .map(|d| d.id)
            .collect()
    };

    // The same phone twice: the first entry is retired.
    let (old_dir, old_ks) = (TempDir::new("old"), Arc::new(MemoryKeyStore::default()));
    let (new_dir, new_ks) = (TempDir::new("new"), Arc::new(MemoryKeyStore::default()));
    let old_id = pair(&old_dir, &old_ks, "Phone").await;
    // A clip copied on the old pairing reaches the hub under the old id.
    let hp = pipeline(&hub_dir.0);
    {
        let old = start_client(&old_dir.0, &old_ks);
        text(&pipeline(&old_dir.0), "from the old phone");
        old.notify_local_change();
        wait("old phone's clip on the hub", || {
            find(&hp, "from the old phone").is_some_and(|c| c.device_id == old_id)
        })
        .await;
        old.shutdown().await;
    }
    let new_id = pair(&new_dir, &new_ks, "Phone").await;
    assert_eq!(active(&hub), vec![new_id.clone()]);
    // ...and is credited to the phone's new pairing, so it isn't listed twice.
    assert_eq!(find(&hp, "from the old phone").unwrap().device_id, new_id);
    assert!(
        clipd_sync::status(&hub_dir.0, &*hub_ks)
            .unwrap()
            .devices
            .iter()
            .any(|d| d.id == old_id && d.revoked_at.is_some())
    );

    // A dead address first in the list doesn't stop the client.
    let db = SyncDb::open(&new_dir.0.join("sync.db")).unwrap();
    let mut space = db.space().unwrap().unwrap();
    space.hub_hosts.insert(0, "127.0.0.1:1".into());
    db.save_space(&space).unwrap();
    drop(db);
    let phone = start_client(&new_dir.0, &new_ks);
    wait("phone connects", || phone.status().unwrap().connected).await;
    let hosts = phone.status().unwrap().hosts;
    assert_eq!(hosts[0], format!("127.0.0.1:{}", hub.port()), "{hosts:?}");

    // Another phone with the same name that's online now is a different
    // phone: it stays.
    let (twin_dir, twin_ks) = (TempDir::new("twin"), Arc::new(MemoryKeyStore::default()));
    let twin_id = pair(&twin_dir, &twin_ks, "Phone").await;
    let mut ids = active(&hub);
    ids.sort();
    let mut want = vec![new_id, twin_id];
    want.sort();
    assert_eq!(ids, want);

    phone.shutdown().await;
    hub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copied_files_are_fetched_on_demand() {
    let (hub_dir, a_dir, files) = (
        TempDir::new("fhub"),
        TempDir::new("fa"),
        TempDir::new("files"),
    );
    let (hub_ks, a_ks) = (
        Arc::new(MemoryKeyStore::default()),
        Arc::new(MemoryKeyStore::default()),
    );
    let hp = pipeline(&hub_dir.0);
    let ap = pipeline(&a_dir.0);

    // A copied image file: only its path is a clip.
    let photo = files.0.join("photo.png");
    image::RgbaImage::from_pixel(800, 400, image::Rgba([200, 40, 90, 255]))
        .save(&photo)
        .unwrap();
    let bytes = std::fs::read(&photo).unwrap();
    hp.ingest(RawClip {
        content: RawContent::Files(vec![photo.to_string_lossy().into_owned()]),
        source_app: Some("explorer.exe".into()),
        captured_at: now_millis(),
        source_url: None,
    })
    .unwrap();

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
    let offer = hub.create_offer();
    client::pair(&a_dir.0, a_ks.clone(), &offer.uri, "Phone", "android")
        .await
        .unwrap();
    let a = start_client(&a_dir.0, &a_ks);
    let file_clip = || {
        ap.store()
            .list(&ListQuery {
                kind: Some(Kind::File),
                ..Default::default()
            })
            .unwrap()
            .pop()
    };
    wait("file clip on the phone", || file_clip().is_some()).await;
    let id = file_clip().unwrap().id;
    wait("phone connected", || a.status().is_ok_and(|s| s.connected)).await;

    // The file itself, then a small preview.
    assert_eq!(a.fetch_file(&id, 0, false).await.unwrap(), bytes);
    let preview = a.fetch_file(&id, 0, true).await.unwrap();
    let img = image::load_from_memory(&preview).unwrap();
    assert_eq!((img.width(), img.height()), (640, 320));

    // No such file in the clip; then the file is gone from the computer.
    let err = a.fetch_file(&id, 1, false).await.unwrap_err();
    assert!(
        format!("{err:#}").contains("isn't on the computer"),
        "{err:#}"
    );
    std::fs::remove_file(&photo).unwrap();
    let err = a.fetch_file(&id, 0, false).await.unwrap_err();
    assert!(format!("{err:#}").contains("moved or deleted"), "{err:#}");

    a.shutdown().await;
    hub.shutdown().await;
}

/// The Import button on the phone: any copied file (a PDF here) comes over
/// on request, byte for byte, sealed; nothing arrives unasked; a file over
/// the cap, a non-image preview and a path that was never copied are refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copied_documents_import_on_request_only() {
    let (hub_dir, a_dir, files) = (
        TempDir::new("ihub"),
        TempDir::new("ia"),
        TempDir::new("ifiles"),
    );
    let (hub_ks, a_ks) = (
        Arc::new(MemoryKeyStore::default()),
        Arc::new(MemoryKeyStore::default()),
    );
    let hp = pipeline(&hub_dir.0);
    let ap = pipeline(&a_dir.0);

    let pdf = files.0.join("Report 2026.pdf");
    let pdf_bytes: Vec<u8> = b"%PDF-1.7\n"
        .iter()
        .copied()
        .chain((0..=255u8).cycle().take(200_000))
        .collect();
    std::fs::write(&pdf, &pdf_bytes).unwrap();
    // Sparse, so cheap: one byte over the cap.
    let big = files.0.join("big.iso");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(clipd_sync::MAX_FILE_FETCH_BYTES + 1)
        .unwrap();
    let secret = files.0.join("never-copied.pdf");
    std::fs::write(&secret, b"%PDF-secret").unwrap();
    // Several documents copied together are one clip with one entry per file.
    let (inv, rcpt) = (files.0.join("invoice.pdf"), files.0.join("receipt.pdf"));
    std::fs::write(&inv, b"%PDF-invoice").unwrap();
    std::fs::write(&rcpt, b"%PDF-receipt").unwrap();
    hp.ingest(RawClip {
        content: RawContent::Files(vec![
            inv.to_string_lossy().into_owned(),
            rcpt.to_string_lossy().into_owned(),
        ]),
        source_app: Some("explorer.exe".into()),
        captured_at: now_millis(),
        source_url: None,
    })
    .unwrap();
    // Three images copied together: a preview per file, by index.
    let shots: Vec<_> = [[200u8, 30, 30], [30, 200, 30], [30, 30, 200]]
        .iter()
        .enumerate()
        .map(|(n, c)| {
            let f = files.0.join(format!("shot{n}.png"));
            image::RgbaImage::from_pixel(100, 60, image::Rgba([c[0], c[1], c[2], 255]))
                .save(&f)
                .unwrap();
            f.to_string_lossy().into_owned()
        })
        .collect();
    hp.ingest(RawClip {
        content: RawContent::Files(shots),
        source_app: Some("explorer.exe".into()),
        captured_at: now_millis(),
        source_url: None,
    })
    .unwrap();
    for f in [&pdf, &big] {
        hp.ingest(RawClip {
            content: RawContent::Files(vec![f.to_string_lossy().into_owned()]),
            source_app: Some("explorer.exe".into()),
            captured_at: now_millis(),
            source_url: None,
        })
        .unwrap();
    }

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
    let offer = hub.create_offer();
    client::pair(&a_dir.0, a_ks.clone(), &offer.uri, "Phone", "android")
        .await
        .unwrap();
    let a = start_client(&a_dir.0, &a_ks);
    let list = || {
        ap.store()
            .list(&ListQuery {
                kind: Some(Kind::File),
                ..Default::default()
            })
            .unwrap()
    };
    wait("all file clips on the phone", || list().len() == 4).await;
    wait("phone connected", || a.status().is_ok_and(|s| s.connected)).await;
    let id_of = |name: &str| {
        list()
            .into_iter()
            .find(|c| c.files[0].ends_with(name))
            .unwrap()
            .id
    };

    // Syncing a file clip carries the path only: the phone holds no copy.
    assert!(!a_dir.0.join("file-cache").exists());
    let pdf_id = id_of("Report 2026.pdf");
    assert_eq!(a.fetch_file(&pdf_id, 0, false).await.unwrap(), pdf_bytes);

    // One clip, two files: each is fetched on its own by its index.
    let multi = id_of("invoice.pdf");
    assert_eq!(
        a.fetch_file(&multi, 1, false).await.unwrap(),
        b"%PDF-receipt"
    );
    assert_eq!(
        a.fetch_file(&multi, 0, false).await.unwrap(),
        b"%PDF-invoice"
    );
    assert!(a.fetch_file(&multi, 2, false).await.is_err());

    // Each image of a multi-image clip has its own preview, not just the first.
    let shots = id_of("shot0.png");
    let mut seen = Vec::new();
    for n in 0..3 {
        let png = a.fetch_file(&shots, n, true).await.unwrap();
        let px = image::load_from_memory(&png)
            .unwrap()
            .to_rgba8()
            .get_pixel(5, 5)
            .0;
        assert!(!seen.contains(&px), "preview {n} repeats an earlier file");
        seen.push(px);
    }

    // Not an image: no preview, with a message that says so.
    let err = a.fetch_file(&pdf_id, 0, true).await.unwrap_err();
    assert!(format!("{err:#}").contains("isn't an image"), "{err:#}");

    // Over 64 MB.
    let err = a.fetch_file(&id_of("big.iso"), 0, false).await.unwrap_err();
    assert!(format!("{err:#}").contains("too big"), "{err:#}");

    // A paired device can't make the computer read a file by syncing a clip
    // that lists it: only clips copied on the computer itself are served.
    ap.ingest(RawClip {
        content: RawContent::Files(vec![secret.to_string_lossy().into_owned()]),
        source_app: None,
        captured_at: now_millis(),
        source_url: None,
    })
    .unwrap();
    wait("phone's file clip reaches the computer", || {
        hp.store()
            .list(&ListQuery {
                kind: Some(Kind::File),
                ..Default::default()
            })
            .unwrap()
            .iter()
            .any(|c| c.files[0].ends_with("never-copied.pdf"))
    })
    .await;
    let injected = id_of("never-copied.pdf");
    let err = a.fetch_file(&injected, 0, false).await.unwrap_err();
    assert!(
        format!("{err:#}").contains("isn't on the computer"),
        "{err:#}"
    );

    // A clip that isn't in this history can't be used to read other files.
    let err = a.fetch_file("no-such-clip", 0, false).await.unwrap_err();
    assert!(
        format!("{err:#}").contains("isn't on this device"),
        "{err:#}"
    );
    assert!(secret.exists());

    a.shutdown().await;
    hub.shutdown().await;
}
