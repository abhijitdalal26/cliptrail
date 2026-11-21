//! Android sync: the phone is a client of the desktop hub (it never hosts).
//!
//! The client loop belongs to the process, not to the UI: it starts when
//! the core is initialized (by whichever Android component started first)
//! so copies saved from the tile or notification are pushed while the
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
use tauri::AppHandle;

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

// ---- copied files on the computer --------------------------------------------
//
// A copied file syncs as its path only. Its preview, and the file itself when
// the user copies or saves it, are fetched from the computer on demand, so
// copying 50 photos on the laptop doesn't fill the phone.

/// Previews of copied files: encrypted like other images (`write_image`),
/// served by `clipimg://`, and pruned to the newest `FILE_CACHE_KEEP`.
const FILE_CACHE_DIR: &str = "file-cache";
const FILE_CACHE_KEEP: usize = 60;
/// Plain copies handed to Android (clipboard via the FileProvider, or the
/// Gallery). Must match `file_paths.xml`; holds only the latest one.
const FILE_OUT_DIR: &str = "clipboard-out";

fn running_client() -> CmdResult<Arc<ClientHandle>> {
    lock(&CTL.running)
        .clone()
        .ok_or_else(|| "Pair this phone with your computer to get files from it".to_string())
}

fn friendly(e: anyhow::Error) -> String {
    let m = format!("{e:#}");
    let mut c = m.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or(m)
}

/// Preview of file `index` of a copied-files clip, fetched once and cached.
#[tauri::command]
pub async fn sync_file_preview(app: AppHandle, id: String, index: usize) -> CmdResult<String> {
    let dir = crate::commands::pipeline(&app)?
        .config()
        .data_dir
        .join(FILE_CACHE_DIR);
    let name: String = id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let path = dir.join(format!("{name}-{index}.png"));
    if path.exists() {
        return Ok(path.to_string_lossy().into_owned());
    }
    let png = running_client()?
        .fetch_file(&id, index, true)
        .await
        .map_err(friendly)?;
    crate::commands::blocking(move || {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        clipd_core::write_image(&path, &png).map_err(|e| format!("{e:#}"))?;
        prune_file_cache(&dir);
        Ok(path.to_string_lossy().into_owned())
    })
    .await
}

fn prune_file_cache(dir: &std::path::Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in files.into_iter().skip(FILE_CACHE_KEEP) {
        let _ = std::fs::remove_file(p);
    }
}

/// Fetch file `index` of a copied-files clip into the hand-off folder.
/// Returns its path and file name.
async fn fetch_to_out(app: &AppHandle, id: &str, index: usize) -> CmdResult<(String, String)> {
    let p = crate::commands::pipeline(app)?;
    let source = {
        let (p, id) = (p.clone(), id.to_owned());
        crate::commands::blocking(move || {
            let clip = p
                .store()
                .get(&id)
                .map_err(|e| format!("{e:#}"))?
                .ok_or_else(|| "That clip no longer exists".to_string())?;
            clip.summary
                .files
                .get(index)
                .cloned()
                .ok_or_else(|| "That file isn't in this clip".to_string())
        })
        .await?
    };
    // A Windows path on the phone: take the name after the last separator.
    let name: String = source
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("file")
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let name = if name.is_empty() {
        "file".to_owned()
    } else {
        name
    };
    let bytes = running_client()?
        .fetch_file(id, index, false)
        .await
        .map_err(friendly)?;
    let dir = p.config().data_dir.join(FILE_OUT_DIR);
    let out = dir.join(&name);
    crate::commands::blocking(move || {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
        Ok((out.to_string_lossy().into_owned(), name))
    })
    .await
}

/// Copy a file that's on the computer to the phone's clipboard.
#[tauri::command]
pub async fn sync_copy_file(app: AppHandle, id: String, index: usize) -> CmdResult<()> {
    let (path, _) = fetch_to_out(&app, &id, index).await?;
    super::bridge::call::<serde_json::Value>(
        &app,
        "writeClipboard",
        serde_json::json!({ "imagePath": path }),
    )
    .await?;
    Ok(())
}

/// Save a file that's on the computer to the phone's Pictures/ClipTrail.
#[tauri::command]
pub async fn sync_save_file(app: AppHandle, id: String, index: usize) -> CmdResult<()> {
    let (path, name) = fetch_to_out(&app, &id, index).await?;
    let res = super::bridge::call::<serde_json::Value>(
        &app,
        "saveToGallery",
        serde_json::json!({ "path": path, "name": name }),
    )
    .await;
    let _ = std::fs::remove_file(&path);
    res.map(|_| ())
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
