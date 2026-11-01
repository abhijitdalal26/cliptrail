//! ClipTrail desktop app: clipboard watcher → clipd-core pipeline, plus the
//! dashboard window, the quick picker, the tray icon and global hotkeys.
//!
//! Rule: never log clip content. Log ids, kinds, sizes.

mod commands;
mod maintenance;
mod settings;
mod shortcuts;
mod state;
mod tray;
mod windows;

use clipd_core::{IngestOutcome, RawClip};
use tauri::{AppHandle, Emitter, Manager};

use state::{AppState, guard, lock};

/// Passed by the login item so the app starts in the tray.
const HIDDEN_ARG: &str = "--hidden";

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,cliptrail_desktop_lib=info".into()),
        )
        .init();

    let start_hidden = std::env::args().any(|a| a == HIDDEN_ARG);

    tauri::Builder::default()
        // Must be first: a second launch just shows the running dashboard.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_dashboard(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .plugin(shortcuts::plugin())
        .manage(AppState::new(clipd_core::default_data_dir()))
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            state.init();

            // Thumbnails / images are served from the data dir via asset://.
            if let Err(e) = app
                .asset_protocol_scope()
                .allow_directory(&state.data_dir, true)
            {
                tracing::warn!("asset scope: {e}");
            }

            windows::create_windows(&handle, start_hidden)?;
            tray::create(&handle)?;

            if let Err(e) = shortcuts::apply(&handle, &state.settings()) {
                tracing::warn!("global shortcut not registered: {e}");
            }

            start_watcher(&handle);
            maintenance::spawn(handle.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_clips,
            commands::get_clip,
            commands::stats,
            commands::day_counts,
            commands::source_apps,
            commands::set_pinned,
            commands::delete_clip,
            commands::copy_clip,
            commands::paste_clip,
            commands::hide_picker,
            commands::show_dashboard,
            commands::open_url,
            commands::reveal_path,
            commands::open_data_dir,
            commands::get_autostart,
            commands::set_autostart,
            commands::capture_state,
            commands::pause_capture,
            commands::resume_capture,
            commands::window_effects,
            commands::set_window_theme,
            commands::backend_status,
            commands::get_settings,
            commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClipTrail");
}

fn start_watcher(app: &AppHandle) {
    let app2 = app.clone();
    match guard("clipboard watcher", || {
        clipd_capture::spawn_watcher(move |raw| on_raw_clip(&app2, raw))
    }) {
        Ok(h) => *lock(&app.state::<AppState>().watcher) = Some(h),
        Err(e) => tracing::error!("{e}"),
    }
}

fn on_raw_clip(app: &AppHandle, raw: RawClip) {
    let state = app.state::<AppState>();
    if state.is_paused() {
        tracing::debug!("capture paused; dropped a clipboard change");
        return;
    }
    let Ok(p) = state.pipeline() else { return };
    match guard("ingest", || p.ingest(raw)) {
        Ok(outcome @ (IngestOutcome::Stored { .. } | IngestOutcome::Bumped { .. })) => {
            if let IngestOutcome::Stored { id, kind } | IngestOutcome::Bumped { id, kind } =
                &outcome
            {
                tracing::debug!(%id, kind = kind.as_str(), "clip ingested");
            }
            let _ = app.emit("clips-changed", &outcome);
            tray::refresh(app);
        }
        Ok(IngestOutcome::Skipped { reason }) => tracing::debug!(%reason, "clip skipped"),
        Err(e) => tracing::warn!("{e}"),
    }
}
