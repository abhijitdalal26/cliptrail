//! Android (Tauri mobile) side of the app.
//!
//! One process, two ways in:
//! - the UI: `run()` (Tauri's mobile entry point, started by MainActivity);
//! - the Kotlin capture services (accessibility service, Quick Settings
//!   tile, share / text-selection activities, keep-alive service), which
//!   call the JNI functions in [`jni`] while no UI may be running.
//!
//! Both use the same process-wide [`Core`] (pipeline + settings), so there is
//! one clip store, one dedup and one sync client. After a native ingest the
//! sync client is poked and, if the UI is alive, `clips-changed` is emitted.
//!
//! Rule: never log clip content.

pub mod bridge;
pub mod commands;
#[cfg(target_os = "android")]
mod jni;
#[cfg(target_os = "android")]
mod logcat;
pub mod sync;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use clipd_core::{IngestOutcome, RawClip};
use clipd_sync::{FileKeyStore, KeyStore};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, Core, guard, lock};
use crate::{commands as cmd, maintenance};

/// What the Kotlin side tells us once per process.
struct Env {
    core: Arc<Core>,
    keystore: Arc<dyn KeyStore>,
    /// User-visible phone name ("Galaxy S23 FE"), used when pairing.
    device_name: String,
}

static ENV: OnceLock<Env> = OnceLock::new();
/// The running UI, if any (set in `setup`).
static APP: Mutex<Option<AppHandle>> = Mutex::new(None);

/// Initialize the process-wide core. Idempotent: the first caller (the
/// Application class, normally) wins. `keys_dir` is app-private and
/// excluded from backups (`noBackupFilesDir`).
pub fn init(data_dir: PathBuf, keys_dir: PathBuf, device_name: String) -> &'static Arc<Core> {
    let env = ENV.get_or_init(|| {
        tracing::info!("initializing the clip store");
        // TODO(android): wrap these with an Android Keystore key. For now a
        // 0600 file in noBackupFilesDir (app-private, never backed up). It
        // holds the sync keys and the key of the encrypted clip database.
        let keystore: Arc<dyn KeyStore> = Arc::new(FileKeyStore::new(&keys_dir));
        Env {
            core: Arc::new(Core::new(data_dir, Some(keystore.clone()))),
            keystore,
            device_name: if device_name.trim().is_empty() {
                "Android phone".into()
            } else {
                device_name.trim().to_owned()
            },
        }
    });
    env.core.init();
    // Sync runs with the process, not with the UI.
    sync::ensure_started();
    &env.core
}

fn env() -> Result<&'static Env, String> {
    ENV.get()
        .ok_or_else(|| "the clip store is not initialized".to_string())
}

pub fn core() -> Result<Arc<Core>, String> {
    env().map(|e| e.core.clone())
}

pub fn keystore() -> Result<Arc<dyn KeyStore>, String> {
    env().map(|e| e.keystore.clone())
}

pub fn device_name() -> String {
    env()
        .map(|e| e.device_name.clone())
        .unwrap_or_else(|_| "Android phone".into())
}

/// Emit to the UI if it's running.
pub fn emit<S: Serialize + Clone>(event: &str, payload: S) {
    let app = lock(&APP).clone();
    if let Some(app) = app {
        let _ = app.emit(event, payload);
    }
}

/// Ingest a clip from the Kotlin side (or the UI's "save clipboard").
pub fn ingest(mut raw: RawClip) -> Result<IngestOutcome, String> {
    let core = core()?;
    if core.is_paused() {
        return Ok(IngestOutcome::Skipped {
            reason: "capture paused".into(),
        });
    }
    let p = core.pipeline()?;
    if core.settings().clean_links {
        crate::clean_copied_link(&mut raw);
    }
    let outcome = guard("ingest", || p.ingest(raw))?;
    match &outcome {
        IngestOutcome::Stored { id, kind } | IngestOutcome::Bumped { id, kind } => {
            tracing::debug!(%id, kind = kind.as_str(), "clip ingested");
            sync::notify_local();
            emit("clips-changed", &outcome);
        }
        IngestOutcome::Skipped { reason } => tracing::debug!(%reason, "clip skipped"),
    }
    Ok(outcome)
}

pub fn run() {
    #[cfg(target_os = "android")]
    logcat::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(bridge::plugin())
        .register_asynchronous_uri_scheme_protocol(
            crate::imageproto::SCHEME,
            crate::imageproto::handle,
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let core = match ENV.get() {
                Some(e) => e.core.clone(),
                // Normally the Application class initialized us already.
                None => {
                    let dir = app.path().app_local_data_dir()?;
                    init(dir.join("files"), dir.join("keys"), String::new()).clone()
                }
            };
            core.init();
            app.manage(AppState::with_core(core));
            *lock(&APP) = Some(handle.clone());
            maintenance::spawn(handle);
            sync::ensure_started();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cmd::list_clips,
            cmd::get_clip,
            cmd::stats,
            cmd::day_counts,
            cmd::source_apps,
            cmd::set_pinned,
            cmd::delete_clip,
            cmd::clear_count,
            cmd::clear_history,
            cmd::transform_clip,
            cmd::clip_actions,
            cmd::open_url,
            cmd::capture_state,
            cmd::pause_capture,
            cmd::resume_capture,
            cmd::backend_status,
            cmd::get_settings,
            cmd::update_settings,
            commands::copy_clip,
            commands::paste_clip,
            commands::hide_picker,
            commands::show_dashboard,
            commands::reveal_path,
            commands::open_data_dir,
            commands::get_autostart,
            commands::set_autostart,
            commands::window_effects,
            commands::set_window_theme,
            commands::android_status,
            commands::android_set_auto_capture,
            commands::android_open,
            commands::android_request_notifications,
            commands::android_scan_qr,
            commands::android_minimize,
            commands::android_take_pair_link,
            commands::android_save_clipboard,
            commands::android_read_clipboard,
            sync::sync_status,
            sync::sync_set_enabled,
            sync::sync_set_port,
            sync::sync_restart,
            sync::sync_create_offer,
            sync::sync_cancel_offer,
            sync::sync_revoke,
            sync::sync_relay_check,
            sync::sync_relay_add,
            sync::sync_relay_remove,
            sync::sync_pair,
            sync::sync_unpair,
            sync::sync_fetch_image,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClipTrail");
}
