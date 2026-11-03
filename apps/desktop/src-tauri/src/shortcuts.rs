//! Global hotkeys (picker + optional dashboard), rebindable from Settings.

use std::str::FromStr;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::settings::Settings;
use crate::state::{AppState, Hotkeys, lock};
use crate::windows::{self, PickerMode};

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let state = app.state::<AppState>();
            let (is_picker, is_dash) = {
                let h = lock(&state.hotkeys);
                (
                    h.picker.as_ref() == Some(shortcut),
                    h.dashboard.as_ref() == Some(shortcut),
                )
            };
            if is_picker {
                windows::toggle_picker(app, PickerMode::Center, None);
            } else if is_dash {
                windows::toggle_dashboard(app);
            }
        })
        .build()
}

fn parse(s: &str) -> Result<Shortcut, String> {
    Shortcut::from_str(s).map_err(|_| format!("“{s}” isn't a valid shortcut"))
}

/// At launch: register the hotkeys, or as many as possible. A dashboard
/// hotkey owned by another app must not cost the user the picker hotkey.
/// The error (if any) is kept for the UI.
pub fn apply_at_launch(app: &AppHandle, s: &Settings) {
    let mut result = apply(app, s);
    if result.is_err() && s.dashboard_shortcut.is_some() {
        let picker_only = Settings {
            dashboard_shortcut: None,
            ..s.clone()
        };
        if apply(app, &picker_only).is_ok() {
            // Keep the original message (it names the dashboard key).
            result = result.map_err(|e| format!("Dashboard shortcut not set: {e}"));
        }
    }
    if let Err(e) = &result {
        tracing::warn!("global shortcut not registered: {e}");
    }
    *lock(&app.state::<AppState>().hotkey_error) = result.err();
}

/// Register the hotkeys from `s`, replacing the current ones. On failure
/// nothing new stays registered and the error says which key failed.
pub fn apply(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let picker = parse(&s.picker_shortcut)?;
    let dashboard = s.dashboard_shortcut.as_deref().map(parse).transpose()?;
    if dashboard.as_ref() == Some(&picker) {
        return Err("The picker and dashboard shortcuts must be different".into());
    }

    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let state = app.state::<AppState>();
    *lock(&state.hotkeys) = Hotkeys::default();

    gs.register(picker)
        .map_err(|_| format!("{} is already in use by another app", s.picker_shortcut))?;
    if let Some(d) = dashboard
        && gs.register(d).is_err()
    {
        let _ = gs.unregister(picker);
        return Err(format!(
            "{} is already in use by another app",
            s.dashboard_shortcut.as_deref().unwrap_or_default()
        ));
    }
    *lock(&state.hotkeys) = Hotkeys {
        picker: Some(picker),
        dashboard,
    };
    *lock(&state.hotkey_error) = None;
    Ok(())
}

/// Settings is recording a new hotkey: release ours so the keys reach the
/// recorder (pressing the current picker combo would otherwise open the
/// picker). `false` registers them again from the saved settings.
pub fn suspend(app: &AppHandle, suspended: bool) {
    let state = app.state::<AppState>();
    if suspended {
        let _ = app.global_shortcut().unregister_all();
        *lock(&state.hotkeys) = Hotkeys::default();
    } else if lock(&state.hotkeys).picker.is_none() {
        let settings = state.settings();
        if let Err(e) = apply(app, &settings) {
            tracing::warn!("global shortcut not registered: {e}");
            *lock(&state.hotkey_error) = Some(e);
        }
    }
}
