//! Android versions of the desktop-only commands, plus Android-only ones.
//! Same names as on desktop so `src/lib/api.ts` works unchanged.

use clipd_core::{Kind, Transform};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::AppHandle;

use super::bridge;
use crate::commands::{blocking, pipeline, transformed_text};
use crate::state::guard;

type CmdResult<T> = Result<T, String>;

/// What `ClipTrailPlugin.writeClipboard` puts on the phone's clipboard.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct WriteArgs {
    text: Option<String>,
    html: Option<String>,
    image_path: Option<String>,
    /// Marks the clip sensitive (Android hides it in the clipboard preview).
    sensitive: bool,
}

/// Copy a stored clip to the phone's clipboard.
#[tauri::command]
pub async fn copy_clip(
    app: AppHandle,
    id: String,
    plain: bool,
    transform: Option<Transform>,
) -> CmdResult<()> {
    let p = pipeline(&app)?;
    let args = blocking(move || {
        if let Some(t) = transform {
            let text = transformed_text(&p, &id, t)?;
            return Ok(WriteArgs {
                text: Some(text),
                ..Default::default()
            });
        }
        let clip = guard("reading clip", || p.store().get(&id))?
            .ok_or_else(|| "That clip no longer exists".to_string())?;
        let s = &clip.summary;
        let sensitive = s.masked || s.kind == Kind::Sensitive;
        Ok(match s.kind {
            Kind::Image => WriteArgs {
                image_path: Some(
                    clip.image_path
                        .clone()
                        .or_else(|| s.thumb_path.clone())
                        .ok_or_else(|| "The image isn't on this phone yet".to_string())?,
                ),
                ..Default::default()
            },
            Kind::File if !s.files.is_empty() => WriteArgs {
                text: Some(s.files.join("\n")),
                ..Default::default()
            },
            _ => WriteArgs {
                text: Some(clip.text.clone().unwrap_or_default()),
                html: if plain { None } else { clip.html.clone() },
                sensitive,
                ..Default::default()
            },
        })
    })
    .await?;
    bridge::call::<Value>(&app, "writeClipboard", args).await?;
    Ok(())
}

#[tauri::command]
pub fn paste_clip(id: String, plain: bool, transform: Option<Transform>) -> CmdResult<()> {
    let _ = (id, plain, transform);
    Err("paste-unavailable: copy, then paste in the other app".into())
}

#[tauri::command]
pub fn hide_picker() {}

#[tauri::command]
pub fn show_dashboard() {}

#[tauri::command]
pub fn reveal_path(path: String) -> CmdResult<()> {
    let _ = path;
    Err("Not available on Android".into())
}

#[tauri::command]
pub fn open_data_dir() -> CmdResult<()> {
    Err("Not available on Android".into())
}

#[tauri::command]
pub fn get_autostart() -> bool {
    false
}

#[tauri::command]
pub fn set_autostart(enabled: bool) -> CmdResult<()> {
    let _ = enabled;
    Err("Not available on Android".into())
}

#[tauri::command]
pub fn window_effects() -> bool {
    false
}

/// Status / navigation bar icons and window background follow the theme.
/// `panel`: a full-screen panel-coloured view (the clip view) is under the
/// bars, so they take the panel colour instead of the page background.
#[tauri::command]
pub async fn set_window_theme(app: AppHandle, dark: bool, panel: Option<bool>) {
    let args = json!({ "dark": dark, "panel": panel.unwrap_or(false) });
    let _ = bridge::call::<Value>(&app, "setBarStyle", args).await;
}

/// Capture health + onboarding checklist (see ClipTrailPlugin.status).
#[tauri::command]
pub async fn android_status(app: AppHandle) -> CmdResult<Value> {
    bridge::call(&app, "status", json!({})).await
}

#[tauri::command]
pub async fn android_set_auto_capture(app: AppHandle, enabled: bool) -> CmdResult<Value> {
    bridge::call(&app, "setAutoCapture", json!({ "enabled": enabled })).await
}

/// Open a system screen: "accessibility" | "appInfo" | "battery" |
/// "batteryOptimization" | "notifications" | "overlay".
#[tauri::command]
pub async fn android_open(app: AppHandle, target: String) -> CmdResult<Value> {
    bridge::call(&app, "open", json!({ "target": target })).await
}

#[tauri::command]
pub async fn android_request_notifications(app: AppHandle) -> CmdResult<Value> {
    bridge::call(&app, "requestNotifications", json!({})).await
}

/// Scan a QR code with the camera. `{ text }` or `{ cancelled: true }`.
#[tauri::command]
pub async fn android_scan_qr(app: AppHandle) -> CmdResult<Value> {
    bridge::call(&app, "scanQr", json!({})).await
}

/// A `cliptrail://pair?...` link that opened the app (camera app, browser,
/// adb). `{ link: string | null }`, returned once.
#[tauri::command]
pub async fn android_take_pair_link(app: AppHandle) -> CmdResult<Value> {
    bridge::call(&app, "takePairLink", json!({})).await
}

/// Back gesture with nothing left to close: send the app to the background.
#[tauri::command]
pub async fn android_minimize(app: AppHandle) -> CmdResult<()> {
    bridge::call::<Value>(&app, "minimize", json!({})).await?;
    Ok(())
}

/// Plain text on the clipboard (for pasting a pairing link; the WebView
/// can't read the clipboard itself). `{ text }`, empty when there's none.
#[tauri::command]
pub async fn android_read_clipboard(app: AppHandle) -> CmdResult<Value> {
    bridge::call(&app, "readClipboardText", json!({})).await
}

/// Read the clipboard now (the app is focused) and save it.
/// `{ outcome: "stored" | "bumped" | "skipped" | "empty" | "sensitive" }`.
#[tauri::command]
pub async fn android_save_clipboard(app: AppHandle) -> CmdResult<Value> {
    bridge::call(&app, "saveClipboard", json!({})).await
}
