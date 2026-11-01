//! Tauri commands called by the UI (see apps/desktop/src/lib/api.ts).
//! Errors are plain strings and never contain clip content.
//!
//! Commands marked `#[cfg(desktop)]` touch the desktop clipboard, windows,
//! tray or login item; Android has its own versions in `mobile::commands`.

use std::sync::Arc;

#[cfg(desktop)]
use clipd_capture::WriteContent;
use clipd_core::{
    ClearScope, Clip, ClipSummary, DayCount, Kind, ListQuery, Millis, Pipeline, Stats, Transform,
};
use serde::Serialize;
#[cfg(desktop)]
use tauri::Window;
use tauri::{AppHandle, Emitter, Manager};
#[cfg(desktop)]
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_opener::OpenerExt;

use crate::settings::Settings;
use crate::state::{AppState, CaptureState, Pause, guard, lock};
use crate::tray;
#[cfg(desktop)]
use crate::{shortcuts, windows};

type CmdResult<T> = Result<T, String>;

/// Run blocking store / OS work off the main thread.
pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| "internal error".to_string())?
}

pub(crate) fn pipeline(app: &AppHandle) -> CmdResult<Arc<Pipeline>> {
    app.state::<AppState>().pipeline()
}

/// Tell every window that clip rows changed (pin/delete/prune), so lists refresh.
pub fn notify_updated(app: &AppHandle) {
    let _ = app.emit("clips-updated", ());
    tray::refresh(app);
}

#[tauri::command]
pub async fn list_clips(app: AppHandle, query: ListQuery) -> CmdResult<Vec<ClipSummary>> {
    let p = pipeline(&app)?;
    blocking(move || guard("listing clips", || p.store().list(&query))).await
}

#[tauri::command]
pub async fn get_clip(app: AppHandle, id: String) -> CmdResult<Option<Clip>> {
    let p = pipeline(&app)?;
    blocking(move || guard("reading clip", || p.store().get(&id))).await
}

#[tauri::command]
pub async fn stats(app: AppHandle) -> CmdResult<Stats> {
    let p = pipeline(&app)?;
    blocking(move || guard("stats", || p.store().stats())).await
}

#[tauri::command]
pub async fn day_counts(app: AppHandle, from: Millis, to: Millis) -> CmdResult<Vec<DayCount>> {
    let p = pipeline(&app)?;
    blocking(move || guard("day counts", || p.store().day_counts(from, to))).await
}

#[tauri::command]
pub async fn source_apps(app: AppHandle) -> CmdResult<Vec<String>> {
    let p = pipeline(&app)?;
    blocking(move || guard("source apps", || p.store().source_apps())).await
}

#[tauri::command]
pub async fn set_pinned(app: AppHandle, id: String, pinned: bool) -> CmdResult<()> {
    let p = pipeline(&app)?;
    blocking(move || guard("pinning", || p.store().set_pinned(&id, pinned))).await?;
    notify_updated(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_clip(app: AppHandle, id: String) -> CmdResult<()> {
    let p = pipeline(&app)?;
    blocking(move || guard("deleting", || p.delete(&id))).await?;
    notify_updated(&app);
    Ok(())
}

/// How many clips "Clear history" would remove (for the confirm dialog).
#[tauri::command]
pub async fn clear_count(app: AppHandle, keep_pinned: bool) -> CmdResult<i64> {
    let p = pipeline(&app)?;
    blocking(move || guard("counting clips", || p.count_clearable(keep_pinned))).await
}

/// Delete the whole history (optionally keeping pinned clips) and its image
/// files. By default this device only, like retention: paired devices keep
/// their copies. `everywhere` also emits delete tombstones so the sync
/// engine clears the other devices. Returns how many clips were removed.
#[tauri::command]
pub async fn clear_history(
    app: AppHandle,
    keep_pinned: bool,
    everywhere: bool,
) -> CmdResult<usize> {
    let p = pipeline(&app)?;
    let scope = if everywhere {
        ClearScope::AllDevices
    } else {
        ClearScope::LocalOnly
    };
    let removed =
        blocking(move || guard("clearing history", || p.clear_history(keep_pinned, scope))).await?;
    tracing::info!(removed, keep_pinned, everywhere, "history cleared");
    notify_updated(&app);
    if everywhere && removed > 0 {
        // Publish the tombstones now instead of on the next poll.
        #[cfg(desktop)]
        crate::sync::local_change(&app);
        #[cfg(mobile)]
        crate::mobile::sync::notify_local();
    }
    Ok(removed)
}

/// Put a stored clip back on the OS clipboard.
#[cfg(desktop)]
pub fn write_back(p: &Pipeline, id: &str, plain: bool) -> CmdResult<Kind> {
    let clip = guard("reading clip", || p.store().get(id))?
        .ok_or_else(|| "That clip no longer exists".to_string())?;
    let kind = clip.summary.kind;
    // "Plain" for a file clip = paste its paths as text.
    let joined_paths = clip.summary.files.join("\n");
    guard("writing to clipboard", || {
        let has_files = !clip.summary.files.is_empty();
        let content = match kind {
            Kind::Image => match clip.image_path.as_deref() {
                Some(path) => WriteContent::ImagePng(path),
                None => anyhow::bail!("image file is missing"),
            },
            Kind::File if has_files && !plain => WriteContent::Files(&clip.summary.files),
            Kind::File if has_files => WriteContent::Text {
                text: &joined_paths,
                html: None,
            },
            _ => WriteContent::Text {
                text: clip.text.as_deref().unwrap_or_default(),
                html: if plain { None } else { clip.html.as_deref() },
            },
        };
        clipd_capture::write_clip(content)
    })?;
    tracing::debug!(%id, kind = kind.as_str(), plain, "wrote clip to clipboard");
    Ok(kind)
}

/// A text clip run through a transform. Sensitive (masked) and non-text
/// clips are refused: transforms are for things you meant to paste as text.
pub(crate) fn transformed_text(p: &Pipeline, id: &str, t: Transform) -> CmdResult<String> {
    let clip = guard("reading clip", || p.store().get(id))?
        .ok_or_else(|| "That clip no longer exists".to_string())?;
    if clip.summary.masked || clip.summary.kind == Kind::Sensitive {
        return Err("Sensitive clips can't be transformed".into());
    }
    let Some(text) = clip.text.as_deref() else {
        return Err("This clip has no text to transform".into());
    };
    clipd_core::apply_transform_for(t, clip.summary.subkind.as_deref(), text)
        .map_err(|e| format!("That action doesn't apply to this clip ({e})"))
}

/// Write a clip to the clipboard; with a transform, the transformed text
/// goes out as plain text (no HTML flavor).
#[cfg(desktop)]
fn write_back_with(
    p: &Pipeline,
    id: &str,
    plain: bool,
    transform: Option<Transform>,
) -> CmdResult<()> {
    let Some(t) = transform else {
        return write_back(p, id, plain).map(|_| ());
    };
    let text = transformed_text(p, id, t)?;
    guard("writing to clipboard", || {
        clipd_capture::write_clip(WriteContent::Text {
            text: &text,
            html: None,
        })
    })?;
    tracing::debug!(%id, transform = ?t, "wrote transformed clip to clipboard");
    Ok(())
}

/// Preview of a transform: the text that copy/paste with it would write.
#[tauri::command]
pub async fn transform_clip(app: AppHandle, id: String, transform: Transform) -> CmdResult<String> {
    let p = pipeline(&app)?;
    blocking(move || transformed_text(&p, &id, transform)).await
}

/// Transforms that make sense for this clip and would change it.
#[tauri::command]
pub async fn clip_actions(app: AppHandle, id: String) -> CmdResult<Vec<Transform>> {
    let p = pipeline(&app)?;
    blocking(move || {
        let clip = guard("reading clip", || p.store().get(&id))?
            .ok_or_else(|| "That clip no longer exists".to_string())?;
        if clip.summary.masked {
            return Ok(Vec::new());
        }
        Ok(clipd_core::available_transforms(
            clip.summary.kind,
            clip.summary.subkind.as_deref(),
            clip.text.as_deref().unwrap_or_default(),
        ))
    })
    .await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn copy_clip(
    app: AppHandle,
    id: String,
    plain: bool,
    transform: Option<Transform>,
) -> CmdResult<()> {
    let p = pipeline(&app)?;
    blocking(move || write_back_with(&p, &id, plain, transform)).await
}

/// Write the clip, hide the picker and paste into the window that was
/// focused before it opened. Errors when paste-back isn't possible (no
/// target, e.g. Wayland); the clip is on the clipboard in that case.
#[cfg(desktop)]
#[tauri::command]
pub async fn paste_clip(
    app: AppHandle,
    id: String,
    plain: bool,
    transform: Option<Transform>,
) -> CmdResult<()> {
    let p = pipeline(&app)?;
    let target = *lock(&app.state::<AppState>().paste_target);
    blocking(move || write_back_with(&p, &id, plain, transform)).await?;
    let Some(target) = target else {
        return Err("paste-unavailable: no window to paste into".into());
    };
    windows::hide_picker(&app);
    blocking(move || guard("paste", || clipd_capture::paste_into(target)))
        .await
        .map_err(|e| format!("paste-unavailable: {e}"))
}

#[cfg(desktop)]
#[tauri::command]
pub fn hide_picker(app: AppHandle) {
    windows::hide_picker(&app);
}

#[cfg(desktop)]
#[tauri::command]
pub fn show_dashboard(app: AppHandle) {
    windows::hide_picker(&app);
    windows::show_dashboard(&app);
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:"))
    {
        return Err("Only web and mail links can be opened".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "Couldn't open the link".to_string())
}

#[cfg(desktop)]
#[tauri::command]
pub fn reveal_path(app: AppHandle, path: String) -> CmdResult<()> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|_| "Couldn't show that file".to_string())
}

#[cfg(desktop)]
#[tauri::command]
pub fn open_data_dir(app: AppHandle) -> CmdResult<()> {
    let dir = app.state::<AppState>().data_dir.clone();
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|_| "Couldn't open the data folder".to_string())
}

#[cfg(desktop)]
#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[cfg(desktop)]
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> CmdResult<()> {
    let al = app.autolaunch();
    let r = if enabled { al.enable() } else { al.disable() };
    r.map_err(|_| "Couldn't change the login item".to_string())?;
    let _ = app.emit("settings-changed", ());
    tray::refresh(&app);
    Ok(())
}

pub fn pause(app: &AppHandle, p: Pause) {
    let state = app.state::<AppState>();
    state.set_pause(p);
    tracing::info!(?p, "capture pause changed");
    let _ = app.emit("capture-changed", state.capture_state());
    tray::refresh(app);
}

#[tauri::command]
pub fn capture_state(app: AppHandle) -> CaptureState {
    app.state::<AppState>().capture_state()
}

/// `minutes`: None = until resumed.
#[tauri::command]
pub fn pause_capture(app: AppHandle, minutes: Option<u32>) {
    let p = match minutes {
        Some(m) => Pause::Until(clipd_core::now_millis() + m as Millis * 60_000),
        None => Pause::Indefinite,
    };
    pause(&app, p);
}

#[tauri::command]
pub fn resume_capture(app: AppHandle) {
    pause(&app, Pause::Off);
}

/// Whether paste-back can post synthetic input. Always true off macOS;
/// the Settings page shows the Accessibility explainer when this is false.
#[cfg(desktop)]
#[tauri::command]
pub fn accessibility_state() -> bool {
    clipd_capture::accessibility_trusted()
}

/// Deep-link into System Settings → Privacy & Security → Accessibility so
/// the user can allow paste-back. (There's no programmatic grant; the OS
/// requires the toggle in Settings.)
#[cfg(desktop)]
#[tauri::command]
pub fn open_accessibility_settings() -> CmdResult<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status()
            .map_err(|_| "Couldn't open System Settings".to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Accessibility permission is only needed on macOS".into())
    }
}

#[cfg(desktop)]
#[tauri::command]
pub fn window_effects(window: Window, app: AppHandle) -> bool {
    app.state::<AppState>().effect(window.label())
}

#[cfg(desktop)]
#[tauri::command]
pub fn set_window_theme(app: AppHandle, dark: bool) {
    windows::set_theme(&app, dark);
}

#[tauri::command]
pub fn backend_status(app: AppHandle) -> Option<String> {
    app.state::<AppState>().init_error()
}

#[derive(Serialize)]
pub struct SettingsView {
    settings: Settings,
    default_excluded_apps: Vec<String>,
    autostart: bool,
    pub(crate) data_dir: String,
    version: String,
    capture: CaptureState,
    platform: &'static str,
}

fn settings_view(app: &AppHandle) -> SettingsView {
    let state = app.state::<AppState>();
    SettingsView {
        settings: state.settings(),
        default_excluded_apps: lock(&state.default_excluded).clone(),
        #[cfg(desktop)]
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        #[cfg(mobile)]
        autostart: false,
        data_dir: state.data_dir.display().to_string(),
        version: app.package_info().version.to_string(),
        capture: state.capture_state(),
        platform: std::env::consts::OS,
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> SettingsView {
    settings_view(&app)
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, settings: Settings) -> CmdResult<SettingsView> {
    let new = settings.normalized();
    let state = app.state::<AppState>();
    let old = state.settings();
    if new == old {
        return Ok(settings_view(&app));
    }

    #[cfg(desktop)]
    {
        let hotkeys_changed = new.picker_shortcut != old.picker_shortcut
            || new.dashboard_shortcut != old.dashboard_shortcut;
        if hotkeys_changed && let Err(e) = shortcuts::apply(&app, &new) {
            let _ = shortcuts::apply(&app, &old);
            return Err(e);
        }
    }

    *lock(&state.settings) = new.clone();
    new.save(&state.data_dir)
        .map_err(|e| format!("Couldn't save settings: {e:#}"))?;

    if new.excluded_apps != old.excluded_apps
        || new.sensitive_ttl_minutes != old.sensitive_ttl_minutes
    {
        state.reload_pipeline(&new)?;
    }
    if new.retention_days != old.retention_days && new.retention_days.is_some() {
        let app2 = app.clone();
        std::thread::spawn(move || crate::maintenance::apply_retention(&app2));
    }

    let _ = app.emit("settings-changed", ());
    tray::refresh(&app);
    Ok(settings_view(&app))
}
