//! Android sync: the phone is a client of the desktop hub (it never hosts).
//!
//! The client loop belongs to the process, not to the UI: it starts when
//! the core is initialized (by whichever Android component started first)
//! so copies captured by the accessibility service are pushed while the
//! app's window is closed. Images sync as thumbnails; the full image is
//! fetched when a clip is opened (`sync_fetch_image`).
//!
//! Same command names and `SyncView` shape as the desktop's `sync.rs`, plus
//! `sync_pair` / `sync_unpair` / `sync_fetch_image`.
//!
//! Rule: never log clip content or keys.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use clipd_sync::relay::RelayHello;
use clipd_sync::{ClientConfig, ClientHandle, PairOutcome, Role, SyncNotice, SyncStatus};
use serde::Serialize;

use crate::state::lock;

type CmdResult<T> = Result<T, String>;

#[derive(Default)]
struct SyncCtl {
    running: Mutex<Option<Arc<ClientHandle>>>,
    error: Mutex<Option<String>>,
    /// Serializes start / stop / pair / unpair.
    op: tokio::sync::Mutex<()>,
    started: AtomicBool,
}

static CTL: LazyLock<SyncCtl> = LazyLock::new(SyncCtl::default);

/// What the Devices settings show (same shape as the desktop's).
#[derive(Serialize)]
pub struct SyncView {
    enabled: bool,
    port: u16,
    status: SyncStatus,
    error: Option<String>,
    names: HashMap<String, String>,
}

/// Start the client once per process (no-op if already started).
pub fn ensure_started() {
    if !CTL.started.swap(true, Ordering::SeqCst) {
        tauri::async_runtime::spawn(async { restart().await });
    }
}

/// A clip was just captured: push it now instead of on the next poll.
pub fn notify_local() {
    if let Some(c) = &*lock(&CTL.running) {
        c.notify_local_change();
    }
}

fn changed() {
    super::emit("sync-changed", ());
}

async fn stop() {
    let taken = lock(&CTL.running).take();
    if let Some(c) = taken {
        // A fetch_image in flight holds a clone for a moment.
        let mut c = c;
        for _ in 0..20 {
            match Arc::try_unwrap(c) {
                Ok(h) => return h.shutdown().await,
                Err(still) => c = still,
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // Dropping the last handle stops the loop as well.
        drop(c);
    }
}

async fn restart() {
    let _guard = CTL.op.lock().await;
    restart_locked().await;
}

async fn restart_locked() {
    stop().await;
    *lock(&CTL.error) = None;
    match start().await {
        Ok(Some(c)) => {
            let rx = c.subscribe();
            *lock(&CTL.running) = Some(Arc::new(c));
            forward_notices(rx);
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!("sync not started: {e}");
            *lock(&CTL.error) = Some(e);
        }
    }
    changed();
}

/// Start the client when sync is on and this data dir is paired.
async fn start() -> CmdResult<Option<ClientHandle>> {
    let core = super::core()?;
    if !core.settings().sync_enabled {
        return Ok(None);
    }
    let ks = super::keystore()?;
    let data_dir = core.data_dir.clone();
    let role = {
        let (dd, ks) = (data_dir.clone(), ks.clone());
        tauri::async_runtime::spawn_blocking(move || clipd_sync::status(&dd, &*ks))
            .await
            .map_err(|_| "sync status task failed".to_string())?
            .map_err(|e| format!("{e:#}"))?
            .role
    };
    match role {
        Some(Role::Client) => clipd_sync::client::start(ClientConfig {
            data_dir,
            keystore: ks,
            fetch_images: false,
        })
        .map(Some)
        .map_err(|e| format!("{e:#}")),
        Some(Role::Hub) => Err("this phone's data is set up as a sync hub; unpair to reset".into()),
        None => Ok(None),
    }
}

fn forward_notices(mut rx: tokio::sync::broadcast::Receiver<SyncNotice>) {
    tauri::async_runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match rx.recv().await {
                Ok(SyncNotice::ClipsChanged { count }) => {
                    tracing::debug!(count, "remote clips applied");
                    super::emit("clips-changed", ());
                }
                Ok(SyncNotice::StatusChanged) => changed(),
                Ok(SyncNotice::Paired { .. }) | Ok(SyncNotice::Event { .. }) => {}
                Err(RecvError::Lagged(_)) => {
                    super::emit("clips-changed", ());
                    changed();
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

async fn view() -> CmdResult<SyncView> {
    let core = super::core()?;
    let settings = core.settings();
    let live = lock(&CTL.running).clone();
    let mut status = match live {
        Some(c) => c.status().map_err(|e| format!("{e:#}"))?,
        None => {
            let ks = super::keystore()?;
            let dd = core.data_dir.clone();
            tauri::async_runtime::spawn_blocking(move || clipd_sync::status(&dd, &*ks))
                .await
                .map_err(|_| "sync status task failed".to_string())?
                .map_err(|e| format!("{e:#}"))?
        }
    };
    if status.keystore.is_empty()
        && let Ok(ks) = super::keystore()
    {
        status.keystore = ks.describe();
    }
    let names = status
        .devices
        .iter()
        .map(|d| (d.id.clone(), d.name.clone()))
        .collect();
    Ok(SyncView {
        enabled: settings.sync_enabled,
        port: settings.sync_port,
        status,
        error: lock(&CTL.error).clone(),
        names,
    })
}

fn save_enabled(enabled: bool) -> CmdResult<()> {
    let core = super::core()?;
    let mut s = core.settings();
    if s.sync_enabled == enabled {
        return Ok(());
    }
    s.sync_enabled = enabled;
    let s = s.normalized();
    s.save(&core.data_dir)
        .map_err(|e| format!("Couldn't save settings: {e:#}"))?;
    *lock(&core.settings) = s;
    super::emit("settings-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn sync_status() -> CmdResult<SyncView> {
    view().await
}

#[tauri::command]
pub async fn sync_set_enabled(enabled: bool) -> CmdResult<SyncView> {
    save_enabled(enabled)?;
    restart().await;
    view().await
}

#[tauri::command]
pub fn sync_set_port(port: u16) -> CmdResult<SyncView> {
    let _ = port;
    Err("The phone doesn't host sync; the port is set on your computer".into())
}

#[tauri::command]
pub async fn sync_restart() -> CmdResult<SyncView> {
    restart().await;
    view().await
}

#[tauri::command]
pub fn sync_create_offer() -> CmdResult<()> {
    Err("Pair from your computer: ClipTrail › Settings › Devices › Pair a phone".into())
}

#[tauri::command]
pub fn sync_cancel_offer() {}

#[tauri::command]
pub fn sync_revoke(device_id: String) -> CmdResult<()> {
    let _ = device_id;
    Err("Only your computer can remove devices. To leave, unpair this phone.".into())
}

/// Pair with the hub in a scanned / pasted `cliptrail://pair?...` link.
#[tauri::command]
pub async fn sync_pair(uri: String) -> CmdResult<PairOutcome> {
    let uri = uri.trim().to_owned();
    if !uri.starts_with("cliptrail://pair") {
        return Err("That isn't a ClipTrail pairing link (it starts with cliptrail://pair)".into());
    }
    let _guard = CTL.op.lock().await;
    let core = super::core()?;
    let ks = super::keystore()?;
    stop().await;
    let out = clipd_sync::pair(
        &core.data_dir,
        ks,
        &uri,
        &super::device_name(),
        clipd_sync::platform(),
    )
    .await
    .map_err(|e| format!("{e:#}"));
    if out.is_ok() {
        save_enabled(true)?;
    }
    restart_locked().await;
    out
}

/// Leave the space: tell the hub (if reachable) and forget the keys. Clips
/// stay on the phone. Ok(true) when the computer confirmed.
#[tauri::command]
pub async fn sync_unpair() -> CmdResult<bool> {
    let _guard = CTL.op.lock().await;
    let core = super::core()?;
    let ks = super::keystore()?;
    stop().await;
    let res = clipd_sync::client::unpair(&core.data_dir, ks)
        .await
        .map_err(|e| format!("{e:#}"));
    *lock(&CTL.error) = None;
    changed();
    res
}

/// Download the full image of a synced image clip. Ok(false) when it isn't
/// available (not paired, offline, or not a synced image).
#[tauri::command]
pub async fn sync_fetch_image(id: String) -> CmdResult<bool> {
    let client = lock(&CTL.running).clone();
    let Some(client) = client else {
        return Ok(false);
    };
    let got = client
        .fetch_image(&id)
        .await
        .map_err(|e| format!("{e:#}"))?;
    if got {
        super::emit("clips-updated", ());
    }
    Ok(got)
}

// ---- off-LAN relays ("Sync anywhere") ---------------------------------------

/// Check a relay URL without changing anything (the "Test" button).
#[tauri::command]
pub async fn sync_relay_check(url: String) -> CmdResult<RelayHello> {
    clipd_sync::relay_check(&url)
        .await
        .map_err(|e| format!("{e:#}"))
}

fn relay_changed() {
    if let Some(c) = &*lock(&CTL.running) {
        c.relay_changed();
    }
}

/// Register this space on a relay and share it with the computer (and the
/// other devices) through the hub.
#[tauri::command]
pub async fn sync_relay_add(url: String, invite: Option<String>) -> CmdResult<SyncView> {
    let core = super::core()?;
    let ks = super::keystore()?;
    let invite = invite.filter(|i| !i.trim().is_empty());
    clipd_sync::relay_add(&core.data_dir, ks, &url, invite.as_deref())
        .await
        .map_err(|e| format!("{e:#}"))?;
    relay_changed();
    changed();
    view().await
}

/// Stop using a relay (on every device); `purge` also deletes this space's
/// data from it.
#[tauri::command]
pub async fn sync_relay_remove(url: String, purge: bool) -> CmdResult<SyncView> {
    let core = super::core()?;
    let ks = super::keystore()?;
    let found = clipd_sync::relay_remove(&core.data_dir, ks, &url, purge)
        .await
        .map_err(|e| format!("{e:#}"))?;
    if !found {
        return Err("That relay isn't set up here".into());
    }
    relay_changed();
    changed();
    view().await
}
