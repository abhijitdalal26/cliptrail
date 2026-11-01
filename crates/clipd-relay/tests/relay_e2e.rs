//! Hub + relay + two clients in one process. Phone "A" can't reach the
//! hub (different network) and syncs only through the relay; desktop "B"
//! is on the LAN and keeps using the hub (relay on standby) until the hub
//! goes away. Covers text, images, pin, delete, offline catch-up, the hub
//! forwarding its log, laptop-off sync, revoke, and that the relay holds
//! no plaintext.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clipd_core::{
    IngestOutcome, ListQuery, Pipeline, PipelineConfig, RawClip, RawContent, now_millis,
};
use clipd_relay::RelayConfig;
use clipd_sync::db::SyncDb;
use clipd_sync::{ClientConfig, ClientHandle, HubConfig, MemoryKeyStore, client, hub};

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("clipd-rel-e2e-{tag}-{}", ulid::Ulid::generate()));
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

fn image(p: &Pipeline, seed: u8) -> Vec<u8> {
    let (w, h) = (90u32, 50u32);
    let rgba = (0..w * h * 4)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    let id = match p
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
    };
    p.store().content_hash(&id).unwrap().unwrap()
}

fn has_full_image(p: &Pipeline, hash: &[u8]) -> bool {
    p.store()
        .get_by_hash(hash)
        .unwrap()
        .is_some_and(|c| c.clip.image_path.is_some())
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while tokio::time::Instant::now() < deadline {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn start_client(dir: &Path, ks: &Arc<MemoryKeyStore>) -> ClientHandle {
    client::start(ClientConfig {
        data_dir: dir.to_path_buf(),
        keystore: ks.clone(),
        fetch_images: true,
    })
    .unwrap()
}

fn relay_state(c: &ClientHandle) -> String {
    c.status()
        .unwrap()
        .relays
        .first()
        .map(|r| r.state.clone())
        .unwrap_or_default()
}

/// Point a client at these hub addresses ("127.0.0.1:9" = phone left the
/// house; a new port = the restarted laptop's new address).
fn set_hub_hosts(dir: &Path, host: &str) {
    let db = SyncDb::open(&dir.join("sync.db")).unwrap();
    let mut s = db.space().unwrap().unwrap();
    s.hub_hosts = vec![host.into()];
    db.save_space(&s).unwrap();
}

async fn start_hub(dir: &Path, ks: &Arc<MemoryKeyStore>) -> hub::HubHandle {
    hub::start(HubConfig {
        data_dir: dir.to_path_buf(),
        keystore: ks.clone(),
        port: 0,
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        advertise: Some(vec!["127.0.0.1".into()]),
        name: "Laptop".into(),
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn off_lan_sync_through_relay() {
    let (relay_dir, hub_dir, a_dir, b_dir) = (
        TempDir::new("relay"),
        TempDir::new("hub"),
        TempDir::new("phone"),
        TempDir::new("desk"),
    );
    let (hub_ks, a_ks, b_ks) = (
        Arc::new(MemoryKeyStore::default()),
        Arc::new(MemoryKeyStore::default()),
        Arc::new(MemoryKeyStore::default()),
    );
    let (hp, ap, bp) = (pipeline(&hub_dir.0), pipeline(&a_dir.0), pipeline(&b_dir.0));

    let relay = clipd_relay::start(RelayConfig::new(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        relay_dir.0.clone(),
    ))
    .await
    .unwrap();
    let relay_url = relay.local_url();

    text(&hp, "laptop history");
    let hub = start_hub(&hub_dir.0, &hub_ks).await;
    let offer = hub.create_offer();
    client::pair(&a_dir.0, a_ks.clone(), &offer.uri, "Phone", "android")
        .await
        .unwrap();
    let offer = hub.create_offer();
    client::pair(&b_dir.0, b_ks.clone(), &offer.uri, "Desk", "linux")
        .await
        .unwrap();

    // Plain http is refused for internet hosts, fine for localhost.
    let e = clipd_sync::relay_add(&hub_dir.0, hub_ks.clone(), "http://relay.example.com", None)
        .await
        .unwrap_err();
    assert!(format!("{e:#}").contains("https"), "{e:#}");
    let view = clipd_sync::relay_add(&hub_dir.0, hub_ks.clone(), &relay_url, None)
        .await
        .unwrap();
    assert!(view.enrolled);
    hub.relay_changed();
    let e = clipd_sync::relay_add(&hub_dir.0, hub_ks.clone(), &relay_url, None)
        .await
        .unwrap_err();
    assert!(format!("{e:#}").contains("already"), "{e:#}");

    // Both clients meet the hub once on the LAN and learn the relay from
    // the sealed SpaceConfig event.
    let a = start_client(&a_dir.0, &a_ks);
    let b = start_client(&b_dir.0, &b_ks);
    wait("clients learn the relay", || {
        a.status().unwrap().relays.len() == 1 && b.status().unwrap().relays.len() == 1
    })
    .await;
    wait("history on both", || {
        find(&ap, "laptop history").is_some() && find(&bp, "laptop history").is_some()
    })
    .await;
    assert_eq!(a.status().unwrap().relays[0].url, relay_url);
    wait("hub bridge connected", || {
        hub.status()
            .unwrap()
            .relays
            .first()
            .is_some_and(|r| r.connected)
    })
    .await;
    // LAN path preferred: B keeps the relay on standby while the hub is up.
    wait("B on standby", || relay_state(&b) == "standby").await;

    // The phone leaves the house.
    a.shutdown().await;
    set_hub_hosts(&a_dir.0, "127.0.0.1:9");
    let a = start_client(&a_dir.0, &a_ks);
    wait("A streams from the relay", || {
        relay_state(&a) == "connected"
    })
    .await;
    assert!(!a.status().unwrap().connected, "hub unreachable for A");

    // Text both ways.
    text(&hp, "from laptop");
    hub.notify_local_change();
    wait("laptop clip on A via relay", || {
        find(&ap, "from laptop").is_some()
    })
    .await;
    text(&ap, "from phone");
    a.notify_local_change();
    wait("phone clip on laptop and LAN desk", || {
        find(&hp, "from phone").is_some() && find(&bp, "from phone").is_some()
    })
    .await;
    assert_eq!(
        relay_state(&b),
        "standby",
        "B got it from the hub, not the relay"
    );
    // A LAN-only device's clip goes desk → hub → (bridge, signed by the
    // hub's key) → relay → phone.
    text(&bp, "from LAN desk");
    b.notify_local_change();
    wait("LAN desk clip forwarded to the off-LAN phone", || {
        find(&ap, "from LAN desk").is_some()
    })
    .await;

    // Images both ways (thumbnail inline, full image as a sealed blob).
    let laptop_img = image(&hp, 3);
    hub.notify_local_change();
    wait("laptop image fully on A", || {
        has_full_image(&ap, &laptop_img)
    })
    .await;
    let phone_img = image(&ap, 7);
    a.notify_local_change();
    wait("phone image fully on laptop and B", || {
        has_full_image(&hp, &phone_img) && has_full_image(&bp, &phone_img)
    })
    .await;

    // Pin and delete both ways.
    let id = find(&ap, "from laptop").unwrap().id;
    ap.store().set_pinned(&id, true).unwrap();
    a.notify_local_change();
    wait("pin from A on laptop", || {
        find(&hp, "from laptop").is_some_and(|c| c.pinned)
    })
    .await;
    let id = find(&hp, "from phone").unwrap().id;
    hp.store().set_pinned(&id, true).unwrap();
    hub.notify_local_change();
    wait("pin from laptop on A", || {
        find(&ap, "from phone").is_some_and(|c| c.pinned)
    })
    .await;
    let id = find(&ap, "laptop history").unwrap().id;
    ap.delete(&id).unwrap();
    a.notify_local_change();
    wait("delete from A everywhere", || {
        find(&hp, "laptop history").is_none() && find(&bp, "laptop history").is_none()
    })
    .await;
    let id = find(&hp, "from phone").unwrap().id;
    hp.delete(&id).unwrap();
    hub.notify_local_change();
    wait("delete from laptop on A", || {
        find(&ap, "from phone").is_none()
    })
    .await;

    // Offline catch-up: the phone is off while the laptop copies, then the
    // laptop goes to sleep before the phone comes back.
    a.shutdown().await;
    text(&hp, "copied while phone off");
    hub.notify_local_change();
    wait("bridge forwarded it", || {
        let st = hub.status().unwrap();
        st.relays.first().is_some_and(|r| r.push_seq >= st.head_seq)
    })
    .await;
    text(&ap, "phone offline copy");
    hub.shutdown().await;
    let a = start_client(&a_dir.0, &a_ks);
    wait("A catches up with the laptop asleep", || {
        find(&ap, "copied while phone off").is_some()
    })
    .await;

    // Laptop off: the LAN desk falls back to the relay; phone ↔ desk.
    wait("B leaves standby", || relay_state(&b) == "connected").await;
    wait("phone offline copy reaches B", || {
        find(&bp, "phone offline copy").is_some()
    })
    .await;
    text(&bp, "desk while laptop off");
    b.notify_local_change();
    wait("desk clip on phone", || {
        find(&ap, "desk while laptop off").is_some()
    })
    .await;

    // Laptop back: it pulls what happened while it slept; B returns to
    // the LAN.
    let hub = start_hub(&hub_dir.0, &hub_ks).await;
    set_hub_hosts(&b_dir.0, &format!("127.0.0.1:{}", hub.port()));
    wait("laptop catches up from relay", || {
        find(&hp, "desk while laptop off").is_some() && find(&hp, "phone offline copy").is_some()
    })
    .await;
    wait("B back on standby", || relay_state(&b) == "standby").await;

    // Revoke the phone: the bridge deletes its relay key and rotates the
    // enroll key; the phone is refused.
    let a_id = ap.config().device_id.clone();
    assert!(hub.revoke(&a_id).unwrap());
    wait("A refused by the relay", || relay_state(&a) == "denied").await;
    text(&hp, "after revoke");
    hub.notify_local_change();
    wait("B still syncs", || find(&bp, "after revoke").is_some()).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(find(&ap, "after revoke").is_none());

    a.shutdown().await;
    b.shutdown().await;
    hub.shutdown().await;
    relay.shutdown().await;

    // The relay never saw plaintext.
    let mut all = Vec::new();
    for entry in std::fs::read_dir(&relay_dir.0).unwrap() {
        all.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    let hay = String::from_utf8_lossy(&all);
    for needle in [
        "laptop history",
        "from laptop",
        "from phone",
        "copied while phone off",
        "phone offline copy",
        "desk while laptop off",
        "after revoke",
        "notepad.exe",
        "snippingtool.exe",
    ] {
        assert!(!hay.contains(needle), "relay stored {needle:?}");
    }
}
