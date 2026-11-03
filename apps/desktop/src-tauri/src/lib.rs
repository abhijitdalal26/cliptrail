//! ClipTrail app. Desktop: clipboard watcher → clipd-core pipeline, plus the
//! dashboard window, the quick picker, the tray icon and global hotkeys.
//! Android (`mobile`): the same UI and core in one window; capture comes
//! from Kotlin services through JNI into this same library.
//!
//! Rule: never log clip content. Log ids, kinds, sizes.

mod commands;
mod maintenance;
#[cfg(mobile)]
mod mobile;
mod settings;
#[cfg(desktop)]
mod shortcuts;
mod state;
#[cfg(desktop)]
mod sync;
#[cfg(desktop)]
mod tray;
#[cfg(desktop)]
mod updater;
#[cfg(desktop)]
mod windows;

/// No tray on mobile.
#[cfg(mobile)]
mod tray {
    pub fn refresh(_app: &tauri::AppHandle) {}
}

#[cfg(desktop)]
use clipd_core::IngestOutcome;
use clipd_core::{RawClip, RawContent};
#[cfg(desktop)]
use tauri::{AppHandle, Emitter, Manager};

#[cfg(desktop)]
use state::{AppState, guard, lock};

/// Passed by the login item so the app starts in the tray.
#[cfg(desktop)]
const HIDDEN_ARG: &str = "--hidden";

/// Show the quick picker (centre mode). Wayland compositors have no global
/// shortcut API an app can register itself; the desktop file and docs
/// suggest binding a DE-level shortcut to `cliptrail --picker`, which
/// reaches the running instance through single_instance.
#[cfg(desktop)]
const PICKER_ARG: &str = "--picker";

#[cfg(mobile)]
#[tauri::mobile_entry_point]
pub fn run() {
    mobile::run();
}

#[cfg(desktop)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,cliptrail_desktop_lib=info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let start_hidden = args.iter().any(|a| a == HIDDEN_ARG);
    let want_picker = args.iter().any(|a| a == PICKER_ARG);

    tauri::Builder::default()
        // Must be first: a second launch reaches the running instance here
        // (`cliptrail --picker` opens the picker, anything else the dashboard).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if args.iter().any(|a| a == PICKER_ARG) {
                windows::toggle_picker(app, windows::PickerMode::Center, None);
            } else {
                windows::show_dashboard(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .plugin(shortcuts::plugin())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::new(clipd_core::default_data_dir()))
        .manage(sync::SyncState::default())
        .manage(updater::UpdaterState::default())
        .setup(move |app| {
            // Menu-bar app on macOS: no Dock icon / Cmd+Tab entry (LSUIElement
            // in Info.plist covers cold start; the runtime call covers dev).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

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

            windows::create_windows(&handle, start_hidden && !want_picker)?;
            tray::create(&handle)?;

            if want_picker {
                windows::toggle_picker(&handle, windows::PickerMode::Center, None);
            }

            shortcuts::apply_at_launch(&handle, &state.settings());

            start_watcher(&handle);
            maintenance::spawn(handle.clone());
            sync::start_if_enabled(&handle);
            updater::spawn(handle.clone());
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
            commands::clear_count,
            commands::clear_history,
            commands::copy_clip,
            commands::paste_clip,
            commands::transform_clip,
            commands::clip_actions,
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
            commands::accessibility_state,
            commands::open_accessibility_settings,
            commands::window_effects,
            commands::set_window_theme,
            commands::suspend_hotkeys,
            commands::backend_status,
            commands::get_settings,
            commands::update_settings,
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
            updater::update_check,
            updater::update_pending,
            updater::update_install,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClipTrail");
}

#[cfg(desktop)]
fn start_watcher(app: &AppHandle) {
    let app2 = app.clone();
    match guard("clipboard watcher", || {
        clipd_capture::spawn_watcher(move |raw| on_raw_clip(&app2, raw))
    }) {
        Ok(h) => *lock(&app.state::<AppState>().watcher) = Some(h),
        Err(e) => tracing::error!("{e}"),
    }
}

/// "Clean tracking parameters from links": rewrite a copied bare link before
/// it is stored (the clip keeps the cleaned URL; the HTML flavor would still
/// carry the old one, so it is dropped when the text changed).
pub(crate) fn clean_copied_link(raw: &mut RawClip) {
    let RawContent::Text { text, html } = &mut raw.content else {
        return;
    };
    if clipd_core::classify_text(text).kind != clipd_core::Kind::Link {
        return;
    }
    if let Ok(clean) = clipd_core::apply_transform(clipd_core::Transform::CleanUrl, text)
        && clean != text.trim()
    {
        *text = clean;
        *html = None;
    }
}

/// True when `app` (an executable name from the capture backend) is this
/// process, i.e. ClipTrail itself put the content on the clipboard.
#[cfg(desktop)]
fn is_own_exe(app: &str) -> bool {
    static OWN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    OWN.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
    })
    .as_deref()
    .is_some_and(|own| own.eq_ignore_ascii_case(app))
}

#[cfg(desktop)]
fn on_raw_clip(app: &AppHandle, raw: RawClip) {
    let state = app.state::<AppState>();
    if state.is_paused() {
        tracing::debug!("capture paused; dropped a clipboard change");
        return;
    }
    let Ok(p) = state.pipeline() else { return };
    let mut raw = raw;
    if raw.source_app.as_deref().is_some_and(is_own_exe) {
        // Our own write-back coming round. A plain write-back bumps the
        // existing clip; `None` keeps its original source app. Transformed
        // text is new content, credited to ClipTrail.
        let transformed = lock(&state.transform_write)
            .take()
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3));
        if !transformed {
            raw.source_app = None;
        }
    }
    if state.settings().clean_links {
        clean_copied_link(&mut raw);
    }
    match guard("ingest", || p.ingest(raw)) {
        Ok(outcome @ (IngestOutcome::Stored { .. } | IngestOutcome::Bumped { .. })) => {
            if let IngestOutcome::Stored { id, kind } | IngestOutcome::Bumped { id, kind } =
                &outcome
            {
                tracing::debug!(%id, kind = kind.as_str(), "clip ingested");
            }
            let _ = app.emit("clips-changed", &outcome);
            tray::refresh(app);
            sync::local_change(app);
        }
        Ok(IngestOutcome::Skipped { reason }) => tracing::debug!(%reason, "clip skipped"),
        Err(e) => tracing::warn!("{e}"),
    }
}
