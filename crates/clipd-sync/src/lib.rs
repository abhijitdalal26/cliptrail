//! ClipTrail sync v1: end-to-end encrypted clip history between paired
//! devices, with a desktop acting as the hub on the LAN.
//!
//! - [`crypto`]: XChaCha20-Poly1305, X25519, HKDF/HMAC-SHA256.
//! - [`pairing`]: QR offer → proof → sealed MasterKey + device token.
//! - [`envelope`]: events (`ClipAdded`/`ClipUpdated`/`ClipDeleted`) sealed
//!   into storage-agnostic envelopes.
//! - [`engine`]: local journal → events, remote events → clip store.
//! - [`hub`] (feature `hub`): the axum server (desktop).
//! - [`client`]: the client loop (desktop/CLI and Android; no desktop deps).
//! - [`relay`] / [`relay_link`]: off-LAN sync through a self-hostable,
//!   ciphertext-only relay (`clipd-relay`); the hub forwards its log there
//!   and clients use it whenever the hub is out of reach.
//!
//! Rule: never log clip content or keys, never put them in errors.

pub mod auth;
pub mod client;
pub mod crypto;
pub mod db;
pub mod engine;
pub mod envelope;
#[cfg(feature = "hub")]
pub mod hub;
pub mod keystore;
pub mod pairing;
pub mod relay;
pub mod relay_link;
pub mod wire;

use std::path::Path;

use anyhow::{Context, Result, bail};
use clipd_core::{Millis, now_millis};
use serde::{Deserialize, Serialize};

pub use client::{ClientConfig, ClientHandle, PairOutcome, pair};
pub use db::{DeviceRow, Role};
pub use engine::SyncNotice;
#[cfg(feature = "hub")]
pub use hub::{HubConfig, HubHandle, OfferView};
pub use keystore::{
    DB_KEY, FileKeyStore, KeyStore, MemoryKeyStore, default_keystore, unlock_data_dir,
};
pub use relay_link::{RelayView, relay_add, relay_check, relay_remove};

use crate::crypto::SecretKey;
use crate::db::SpaceState;
use crate::engine::{Engine, LocalEvent, open_db, open_pipeline};
use crate::envelope::SeqEnvelope;
use crate::keystore::{DEVICE_TOKEN, MASTER_KEY};

/// Default LAN port of the hub.
pub const DEFAULT_PORT: u16 = 47821;
pub const PROTOCOL_VERSION: u32 = 1;

/// A device as shown in UIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub paired_at: Millis,
    pub last_seen: Option<Millis>,
    pub revoked_at: Option<Millis>,
    pub is_self: bool,
    /// Hub: has a live connection now. Client: unknown (false).
    pub online: bool,
}

impl DeviceView {
    pub fn from_row(d: DeviceRow, online: bool) -> DeviceView {
        DeviceView {
            id: d.id,
            name: d.name,
            platform: d.platform,
            paired_at: d.paired_at,
            last_seen: d.last_seen,
            revoked_at: d.revoked_at,
            is_self: d.is_self,
            online,
        }
    }
}

/// Snapshot for status lines and settings screens.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    pub configured: bool,
    pub role: Option<Role>,
    pub space_id: Option<String>,
    pub device_id: String,
    pub device_name: Option<String>,
    pub hub_name: Option<String>,
    /// The hub server / client loop is running in this process.
    pub running: bool,
    /// Client: connected to the hub. Hub: at least one device online.
    pub connected: bool,
    pub port: Option<u16>,
    /// Hub: `host:port` it advertises. Client: hub candidates.
    pub hosts: Vec<String>,
    pub devices: Vec<DeviceView>,
    /// Client: events waiting to be sent.
    pub outbox: i64,
    pub pending_images: i64,
    /// Client: last applied hub seq. Hub: head of the log.
    pub last_seq: i64,
    pub head_seq: i64,
    pub last_error: Option<String>,
    pub keystore: String,
    /// Off-LAN relays of the space and their state on this device.
    pub relays: Vec<RelayView>,
}

/// OS name as used in device lists.
pub fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "android" => "android",
        "ios" => "ios",
        _ => "linux",
    }
}

/// A friendly default name for this machine.
pub fn default_device_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(|s| s.trim().to_owned())
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| match platform() {
            "windows" => "Windows PC".into(),
            "macos" => "Mac".into(),
            _ => "Computer".into(),
        })
}

/// This data dir's device id (the clip store's).
pub fn local_device_id(data_dir: &Path) -> Result<String> {
    Ok(clipd_core::PipelineConfig::with_defaults(data_dir.to_path_buf())?.device_id)
}

/// Status from disk (nothing running in this process).
pub fn status(data_dir: &Path, keystore: &dyn KeyStore) -> Result<SyncStatus> {
    let db = open_db(data_dir)?;
    let device_id = local_device_id(data_dir)?;
    let mut st = SyncStatus {
        device_id,
        keystore: keystore.describe(),
        ..Default::default()
    };
    let Some(space) = db.space()? else {
        return Ok(st);
    };
    st.configured = true;
    st.role = Some(space.role);
    st.space_id = Some(space.space_id.clone());
    st.device_name = Some(space.device_name.clone());
    st.hub_name = Some(space.hub_name.clone());
    st.devices = db
        .devices()?
        .into_iter()
        .map(|d| DeviceView::from_row(d, false))
        .collect();
    st.outbox = db.outbox_len()?;
    st.pending_images = db.pending_blob_count()?;
    st.relays = relay_link::relay_views(&db)?;
    match space.role {
        Role::Hub => {
            st.head_seq = db.head_seq()?;
            st.last_seq = st.head_seq;
            st.port = db.get("port")?.and_then(|p| p.parse().ok());
        }
        Role::Client => {
            st.last_seq = db.get_i64("last_seq")?;
            st.hosts = space.hub_hosts.clone();
        }
    }
    if keystore.get(MASTER_KEY)?.is_none() {
        st.last_error = Some("sync key missing from the key store".into());
    }
    Ok(st)
}

/// Seal a local event and append it to the hub log.
pub(crate) fn hub_append(engine: &Engine, ev: &LocalEvent) -> Result<Option<SeqEnvelope>> {
    let env = engine.seal(&ev.event)?;
    let hash = ev.event.hash()?;
    let (seq, new) = engine.db.append_envelope(
        &env,
        &engine.keys.hash_tag(&hash),
        ev.event.kind_name(),
        now_millis(),
    )?;
    Ok(new.then_some(SeqEnvelope { seq, env }))
}

/// Seal a local event into the client outbox.
pub(crate) fn client_enqueue(engine: &Engine, ev: &LocalEvent) -> Result<()> {
    let env = engine.seal(&ev.event)?;
    engine.db.push_outbox(
        &env,
        ev.blob.as_ref().map(|(id, h)| (id.as_str(), h.as_slice())),
        now_millis(),
    )
}

/// Make this data dir the hub of a new space (or return the existing
/// one). A new space shares the newest [`engine::BACKFILL_LIMIT`] clips.
pub fn init_hub(data_dir: &Path, keystore: &dyn KeyStore, name: &str) -> Result<SpaceState> {
    let db = open_db(data_dir)?;
    if let Some(s) = db.space()? {
        match s.role {
            Role::Hub => {
                if keystore.get(MASTER_KEY)?.is_none() {
                    bail!("the sync key is missing from the key store; reset sync to start over");
                }
                return Ok(s);
            }
            Role::Client => bail!(
                "this data dir is paired with \"{}\" as a client; reset sync first",
                s.hub_name
            ),
        }
    }
    let pipeline = open_pipeline(data_dir)?;
    let device_id = pipeline.config().device_id.clone();
    let master = SecretKey::generate();
    keystore
        .set(MASTER_KEY, master.as_bytes())
        .context("store the sync key")?;
    let name = if name.trim().is_empty() {
        default_device_name()
    } else {
        name.trim().to_owned()
    };
    let space = SpaceState {
        role: Role::Hub,
        space_id: ulid::Ulid::generate().to_string(),
        device_id: device_id.clone(),
        device_name: name.clone(),
        hub_device_id: device_id.clone(),
        hub_name: name.clone(),
        hub_hosts: vec![],
    };
    let now = now_millis();
    db.upsert_device(
        &DeviceRow {
            id: device_id,
            name,
            platform: platform().into(),
            paired_at: now,
            revoked_at: None,
            last_seen: Some(now),
            is_self: true,
        },
        None,
    )?;
    db.set_i64("journal_cursor", pipeline.store().journal_head()?)?;
    db.save_space(&space)?;
    drop((db, pipeline));

    let engine = Engine::open(data_dir, keystore)?;
    let events = engine.backfill(engine::BACKFILL_LIMIT)?;
    let n = events.len();
    for ev in &events {
        hub_append(&engine, ev)?;
    }
    tracing::info!(space = %space.space_id, backfilled = n, "created sync space");
    Ok(space)
}

/// Forget the space: sync state, keys and cached blobs. Clips stay.
pub fn reset(data_dir: &Path, keystore: &dyn KeyStore) -> Result<()> {
    let db = open_db(data_dir)?;
    db.wipe()?;
    keystore.delete(MASTER_KEY)?;
    keystore.delete(DEVICE_TOKEN)?;
    keystore.delete(relay::RELAY_KEY)?;
    let _ = std::fs::remove_dir_all(data_dir.join(engine::BLOB_DIR));
    Ok(())
}

/// Hub: revoke a device from disk (works while the hub runs in another
/// process: tokens are checked on every request, streams every 5 s).
pub fn revoke(data_dir: &Path, device_id: &str) -> Result<bool> {
    let db = open_db(data_dir)?;
    match db.space()? {
        Some(s) if s.role == Role::Hub => db.revoke_device(device_id, now_millis()),
        Some(_) => bail!("only the hub can revoke devices"),
        None => bail!("sync is not set up"),
    }
}

/// Device id → name for every device this data dir knows (badges).
pub fn device_names(data_dir: &Path) -> Result<Vec<(String, String)>> {
    Ok(open_db(data_dir)?
        .devices()?
        .into_iter()
        .map(|d| (d.id, d.name))
        .collect())
}
