//! Sync with paired devices. The app runs the LAN hub while "Sync with my
//! devices" is on (a data dir paired as a client via `clipd sync pair`
//! runs the client loop instead). Remote changes refresh the UI through
//! `clips-changed`; status changes go out as `sync-changed`.
//!
//! Rule: never log clip content or keys.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use clipd_sync::relay::RelayHello;
use clipd_sync::{
    ClientConfig, ClientHandle, HubConfig, HubHandle, KeyStore, OfferView, Role, SyncNotice,
    SyncStatus,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, lock};
use crate::tray;

type CmdResult<T> = Result<T, String>;

enum Running {
    Hub(HubHandle),
    Client(ClientHandle),
}

#[derive(Default)]
pub struct SyncState {
    running: Mutex<Option<Running>>,
    error: Mutex<Option<String>>,
    keystore: Mutex<Option<Arc<dyn KeyStore>>>,
    /// Serializes start/stop.
    op: tokio::sync::Mutex<()>,
}

/// What the Devices settings and the sidebar show.
#[derive(Serialize)]
pub struct SyncView {
    enabled: bool,
    port: u16,
    status: SyncStatus,
    /// Why the hub isn't running, if it should be.
    error: Option<String>,
    /// device id → name, for clip badges.
    names: HashMap<String, String>,
}

fn keystore(app: &AppHandle) -> CmdResult<Arc<dyn KeyStore>> {
    let sync = app.state::<SyncState>();
    if let Some(ks) = lock(&sync.keystore).clone() {
        return Ok(ks);
    }
    let state = app.state::<AppState>();
    let device_id = state.pipeline()?.config().device_id.clone();
    let ks = clipd_sync::default_keystore(&state.data_dir, &device_id);
    *lock(&sync.keystore) = Some(ks.clone());
    Ok(ks)
}

/// Start sync at launch when enabled.
pub fn start_if_enabled(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { restart(&app).await });
}

/// A clip was just captured: publish it now instead of on the next poll.
pub fn local_change(app: &AppHandle) {
    let sync = app.state::<SyncState>();
    match &*lock(&sync.running) {
        Some(Running::Hub(h)) => h.notify_local_change(),
        Some(Running::Client(c)) => c.notify_local_change(),
        None => {}
    }
}

fn emit_changed(app: &AppHandle) {
    let _ = app.emit("sync-changed", ());
}

async fn stop(app: &AppHandle) {
    let taken = lock(&app.state::<SyncState>().running).take();
    match taken {
        Some(Running::Hub(h)) => h.shutdown().await,
        Some(Running::Client(c)) => c.shutdown().await,
        None => {}
    }
}

/// (Re)start according to the settings.
async fn restart(app: &AppHandle) {
    let sync = app.state::<SyncState>();
    let _guard = sync.op.lock().await;
    stop(app).await;
    *lock(&sync.error) = None;
    let settings = app.state::<AppState>().settings();
    if !settings.sync_enabled {
        emit_changed(app);
        return;
    }
    let result = start(app, settings.sync_port).await;
    match result {
        Ok(running) => {
            let notices = match &running {
                Running::Hub(h) => h.subscribe(),
                Running::Client(c) => c.subscribe(),
            };
            *lock(&sync.running) = Some(running);
            forward_notices(app.clone(), notices);
        }
        Err(e) => {
            tracing::warn!("sync not started: {e}");
            *lock(&sync.error) = Some(e);
        }
    }
    emit_changed(app);
}

async fn start(app: &AppHandle, port: u16) -> CmdResult<Running> {
    let ks = keystore(app)?;
    let data_dir = app.state::<AppState>().data_dir.clone();
    let role = {
        let (dd, ks) = (data_dir.clone(), ks.clone());
        tauri::async_runtime::spawn_blocking(move || clipd_sync::status(&dd, &*ks))
            .await
            .map_err(|_| "sync status task failed".to_string())?
            .map_err(|e| format!("{e:#}"))?
            .role
    };
    if role == Some(Role::Client) {
        let c = clipd_sync::client::start(ClientConfig {
            data_dir,
            keystore: ks,
            fetch_images: true,
        })
        .map_err(|e| format!("{e:#}"))?;
        return Ok(Running::Client(c));
    }
    let mut cfg = HubConfig::new(data_dir, ks);
    cfg.port = port;
    // Dev/test knob: `CLIPTRAIL_SYNC_BIND=127.0.0.1` keeps the hub off the
    // LAN (no firewall prompt) and advertises loopback in the QR.
    if let Some(ip) = std::env::var("CLIPTRAIL_SYNC_BIND")
        .ok()
        .and_then(|v| v.parse::<std::net::IpAddr>().ok())
    {
        cfg.bind = ip;
        if ip.is_loopback() {
            cfg.advertise = Some(vec![ip.to_string()]);
        }
    }
    let h = clipd_sync::hub::start(cfg)
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(Running::Hub(h))
}

fn forward_notices(app: AppHandle, mut rx: tokio::sync::broadcast::Receiver<SyncNotice>) {
    tauri::async_runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match rx.recv().await {
                Ok(SyncNotice::ClipsChanged { count }) => {
                    tracing::debug!(count, "remote clips applied");
                    let _ = app.emit("clips-changed", ());
                    tray::refresh(&app);
                }
                Ok(SyncNotice::StatusChanged) => emit_changed(&app),
                Ok(n @ SyncNotice::Paired { .. }) => {
                    let _ = app.emit("sync-paired", &n);
                    emit_changed(&app);
                }
                Ok(SyncNotice::Event { .. }) => {}
                Err(RecvError::Lagged(_)) => {
                    let _ = app.emit("clips-changed", ());
                    emit_changed(&app);
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

async fn view(app: &AppHandle) -> CmdResult<SyncView> {
    let settings = app.state::<AppState>().settings();
    let sync = app.state::<SyncState>();
    let live = match &*lock(&sync.running) {
        Some(Running::Hub(h)) => Some(h.status()),
        Some(Running::Client(c)) => Some(c.status()),
        None => None,
    };
    let mut status = match live {
        Some(s) => s.map_err(|e| format!("{e:#}"))?,
        None => {
            let ks = keystore(app)?;
            let dd = app.state::<AppState>().data_dir.clone();
            tauri::async_runtime::spawn_blocking(move || clipd_sync::status(&dd, &*ks))
                .await
                .map_err(|_| "sync status task failed".to_string())?
                .map_err(|e| format!("{e:#}"))?
        }
    };
    if status.keystore.is_empty()
        && let Ok(ks) = keystore(app)
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
        error: lock(&sync.error).clone(),
        names,
    })
}

fn save_settings(app: &AppHandle, f: impl FnOnce(&mut crate::settings::Settings)) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut s = state.settings();
    f(&mut s);
    let s = s.normalized();
    s.save(&state.data_dir)
        .map_err(|e| format!("Couldn't save settings: {e:#}"))?;
    *lock(&state.settings) = s;
    let _ = app.emit("settings-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn sync_status(app: AppHandle) -> CmdResult<SyncView> {
    view(&app).await
}

#[tauri::command]
pub async fn sync_set_enabled(app: AppHandle, enabled: bool) -> CmdResult<SyncView> {
    save_settings(&app, |s| s.sync_enabled = enabled)?;
    restart(&app).await;
    view(&app).await
}

#[tauri::command]
pub async fn sync_set_port(app: AppHandle, port: u16) -> CmdResult<SyncView> {
    if port < 1024 {
        return Err("Pick a port between 1024 and 65535".into());
    }
    save_settings(&app, |s| s.sync_port = port)?;
    restart(&app).await;
    view(&app).await
}

/// Retry after an error (e.g. the port was busy).
#[tauri::command]
pub async fn sync_restart(app: AppHandle) -> CmdResult<SyncView> {
    restart(&app).await;
    view(&app).await
}

#[tauri::command]
pub fn sync_create_offer(app: AppHandle) -> CmdResult<OfferView> {
    match &*lock(&app.state::<SyncState>().running) {
        Some(Running::Hub(h)) => Ok(h.create_offer()),
        Some(Running::Client(_)) => {
            Err("This computer is paired to another hub; pair new devices there".into())
        }
        None => Err("Turn on sync first".into()),
    }
}

#[tauri::command]
pub fn sync_cancel_offer(app: AppHandle) {
    if let Some(Running::Hub(h)) = &*lock(&app.state::<SyncState>().running) {
        h.cancel_offers();
    }
}

#[tauri::command]
pub async fn sync_revoke(app: AppHandle, device_id: String) -> CmdResult<()> {
    let sync = app.state::<SyncState>();
    let revoked = {
        let running = lock(&sync.running);
        match &*running {
            Some(Running::Hub(h)) => Some(h.revoke(&device_id)),
            _ => None,
        }
    };
    let ok = match revoked {
        Some(r) => r.map_err(|e| format!("{e:#}"))?,
        None => {
            let dd = app.state::<AppState>().data_dir.clone();
            tauri::async_runtime::spawn_blocking(move || clipd_sync::revoke(&dd, &device_id))
                .await
                .map_err(|_| "revoke task failed".to_string())?
                .map_err(|e| format!("{e:#}"))?
        }
    };
    if !ok {
        return Err("That device is not paired".into());
    }
    emit_changed(&app);
    Ok(())
}

// ---- off-LAN relays ("Sync anywhere") ---------------------------------------

/// Check a relay URL without changing anything (the "Test" button).
#[tauri::command]
pub async fn sync_relay_check(url: String) -> CmdResult<RelayHello> {
    clipd_sync::relay_check(&url)
        .await
        .map_err(|e| format!("{e:#}"))
}

/// Tell the running hub / client that the relay list changed.
fn relay_changed(app: &AppHandle) {
    match &*lock(&app.state::<SyncState>().running) {
        Some(Running::Hub(h)) => h.relay_changed(),
        Some(Running::Client(c)) => c.relay_changed(),
        None => {}
    }
}

fn require_space(app: &AppHandle) -> CmdResult<()> {
    if !app.state::<AppState>().settings().sync_enabled {
        return Err("Turn on sync first".into());
    }
    Ok(())
}

/// Register this space on a relay and share it with the other devices.
#[tauri::command]
pub async fn sync_relay_add(
    app: AppHandle,
    url: String,
    invite: Option<String>,
) -> CmdResult<SyncView> {
    require_space(&app)?;
    let ks = keystore(&app)?;
    let dd = app.state::<AppState>().data_dir.clone();
    let invite = invite.filter(|i| !i.trim().is_empty());
    clipd_sync::relay_add(&dd, ks, &url, invite.as_deref())
        .await
        .map_err(|e| format!("{e:#}"))?;
    relay_changed(&app);
    emit_changed(&app);
    view(&app).await
}

/// Stop using a relay (on every device); `purge` also deletes this space's
/// data from it.
#[tauri::command]
pub async fn sync_relay_remove(app: AppHandle, url: String, purge: bool) -> CmdResult<SyncView> {
    let ks = keystore(&app)?;
    let dd = app.state::<AppState>().data_dir.clone();
    let found = clipd_sync::relay_remove(&dd, ks, &url, purge)
        .await
        .map_err(|e| format!("{e:#}"))?;
    if !found {
        return Err("That relay isn't set up here".into());
    }
    relay_changed(&app);
    emit_changed(&app);
    view(&app).await
}
