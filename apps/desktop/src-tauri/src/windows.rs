//! Dashboard + quick picker windows: creation, backdrop effects, placement.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalRect, Rect, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::state::{AppState, guard, lock};

pub const MAIN: &str = "main";
pub const PICKER: &str = "picker";

const PICKER_W: f64 = 640.0;
const PICKER_H: f64 = 460.0;
/// Gap between the tray flyout and the taskbar / screen edge (logical px).
const FLYOUT_MARGIN: f64 = 12.0;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PickerMode {
    /// Opened by the hotkey: centered on the monitor with the cursor.
    Center,
    /// Opened from the tray icon: anchored above the taskbar.
    Flyout,
}

#[derive(Clone, Serialize)]
struct PickerShown {
    mode: PickerMode,
}

pub fn create_windows(app: &AppHandle, start_hidden: bool) -> tauri::Result<()> {
    let main = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("ClipTrail")
        .inner_size(1200.0, 800.0)
        .min_inner_size(880.0, 560.0)
        .center()
        .decorations(false)
        .shadow(true)
        .visible(false);
    #[cfg(target_os = "windows")]
    let main = main.transparent(true);
    let main = main.build()?;

    let picker =
        WebviewWindowBuilder::new(app, PICKER, WebviewUrl::App("index.html#/picker".into()))
            .title("ClipTrail Quick Paste")
            .inner_size(PICKER_W, PICKER_H)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .shadow(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false);
    #[cfg(target_os = "windows")]
    let picker = picker.transparent(true);
    let picker = picker.build()?;
    #[cfg(target_os = "macos")]
    macos::prepare_picker(&picker);

    let state = app.state::<AppState>();
    let dark = is_dark(app);
    apply_effects(&main, &picker, &state, dark);

    if !start_hidden {
        let _ = main.show();
        let _ = main.set_focus();
    }

    // Closing the dashboard hides it to the tray.
    let main_handle = main.clone();
    main.on_window_event(move |e| {
        if let WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            let _ = main_handle.hide();
        }
    });

    // The picker hides itself when it loses focus.
    let app2 = app.clone();
    picker.on_window_event(move |e| {
        if let WindowEvent::Focused(false) = e {
            hide_picker(&app2);
        }
    });
    Ok(())
}

fn is_dark(app: &AppHandle) -> bool {
    use crate::settings::Theme;
    match app.state::<AppState>().settings().theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => app
            .get_webview_window(MAIN)
            .and_then(|w| w.theme().ok())
            .map(|t| t == tauri::Theme::Dark)
            .unwrap_or(false),
    }
}

#[cfg(target_os = "windows")]
fn apply_effects(main: &WebviewWindow, picker: &WebviewWindow, state: &AppState, dark: bool) {
    let m = window_vibrancy::apply_mica(main, Some(dark)).is_ok();
    state.main_effect.store(m, Ordering::Relaxed);
    let tint = if dark {
        (24, 24, 27, 200)
    } else {
        (250, 250, 252, 200)
    };
    let p = window_vibrancy::apply_acrylic(picker, Some(tint)).is_ok();
    state.picker_effect.store(p, Ordering::Relaxed);
    tracing::info!(mica = m, acrylic = p, "window effects");
}

#[cfg(not(target_os = "windows"))]
fn apply_effects(_main: &WebviewWindow, _picker: &WebviewWindow, state: &AppState, _dark: bool) {
    state.main_effect.store(false, Ordering::Relaxed);
    state.picker_effect.store(false, Ordering::Relaxed);
}

/// Re-tint native backdrops and title-bar theme after a theme change.
pub fn set_theme(app: &AppHandle, dark: bool) {
    let theme = Some(if dark {
        tauri::Theme::Dark
    } else {
        tauri::Theme::Light
    });
    let (Some(main), Some(picker)) = (app.get_webview_window(MAIN), app.get_webview_window(PICKER))
    else {
        return;
    };
    let _ = main.set_theme(theme);
    let _ = picker.set_theme(theme);
    let state = app.state::<AppState>();
    let had =
        state.main_effect.load(Ordering::Relaxed) || state.picker_effect.load(Ordering::Relaxed);
    if had {
        apply_effects(&main, &picker, &state, dark);
    }
}

pub fn show_dashboard(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn toggle_dashboard(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let visible = w.is_visible().unwrap_or(false);
        let focused = w.is_focused().unwrap_or(false);
        if visible && focused {
            let _ = w.hide();
        } else {
            show_dashboard(app);
        }
    }
}

pub fn hide_picker(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(PICKER)
        && w.is_visible().unwrap_or(false)
    {
        let _ = w.hide();
        *lock(&app.state::<AppState>().picker_hidden_at) = Some(Instant::now());
        let _ = app.emit_to(PICKER, "picker-hidden", ());
    }
}

/// Remember the hovered-over foreground window while the cursor is on the
/// tray icon, before a click moves focus to the taskbar.
pub fn note_tray_hover(app: &AppHandle) {
    if let Ok(Some(w)) = guard("remember_foreground", || {
        Ok(clipd_capture::remember_foreground())
    }) {
        *lock(&app.state::<AppState>().hover_target) = Some((w, Instant::now()));
    }
}

/// Toggle the picker. `anchor` = tray icon rect for the flyout placement.
pub fn toggle_picker(app: &AppHandle, mode: PickerMode, anchor: Option<Rect>) {
    let Some(w) = app.get_webview_window(PICKER) else {
        return;
    };
    let state = app.state::<AppState>();

    if w.is_visible().unwrap_or(false) {
        hide_picker(app);
        return;
    }
    // A tray click first blurs (and hides) the open picker; don't reopen it.
    if mode == PickerMode::Flyout
        && lock(&state.picker_hidden_at).is_some_and(|t| t.elapsed() < Duration::from_millis(300))
    {
        return;
    }

    let target = match mode {
        PickerMode::Flyout => lock(&state.hover_target)
            .filter(|(_, t)| t.elapsed() < Duration::from_secs(20))
            .map(|(w, _)| w)
            .or_else(|| {
                guard("remember_foreground", || {
                    Ok(clipd_capture::remember_foreground())
                })
                .ok()
                .flatten()
            }),
        PickerMode::Center => guard("remember_foreground", || {
            Ok(clipd_capture::remember_foreground())
        })
        .ok()
        .flatten(),
    };
    *lock(&state.paste_target) = target;

    if let Some(pos) = placement(app, &w, mode, anchor) {
        let _ = w.set_position(pos);
    } else {
        let _ = w.center();
    }
    let _ = app.emit_to(PICKER, "picker-shown", PickerShown { mode });
    show_picker(&w);
}

/// Show the picker and give it keyboard focus. On macOS it must NOT
/// activate the app — the app the user was in stays frontmost so
/// `paste_into` can target it (this is how Spotlight-style launchers work).
#[cfg(not(target_os = "macos"))]
fn show_picker(w: &WebviewWindow) {
    let _ = w.show();
    let _ = w.set_focus();
}

#[cfg(target_os = "macos")]
fn show_picker(w: &WebviewWindow) {
    macos::show_picker(w);
}

/// macOS-only picker plumbing: the picker is a plain NSWindow with the
/// `NonactivatingPanel` style-mask bit — it shows and becomes key without
/// activating our (LSUIElement/accessory) app, so the previous app keeps
/// focus and `remember_foreground`/`paste_into` still see it.
#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::{NSFloatingWindowLevel, NSWindow, NSWindowLevel, NSWindowStyleMask};
    use tauri::WebviewWindow;

    fn ns_window(w: &WebviewWindow) -> Option<&'static NSWindow> {
        let ptr = w.ns_window().ok()?;
        // SAFETY: the pointer is the live NSWindow backing the webview.
        Some(unsafe { &*(ptr.cast::<NSWindow>()) })
    }

    /// Called once after the window is built.
    pub fn prepare_picker(w: &WebviewWindow) {
        let Some(win) = ns_window(w) else {
            tracing::warn!("picker: no NSWindow handle");
            return;
        };
        win.setStyleMask(win.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        win.setLevel(NSFloatingWindowLevel as NSWindowLevel);
        win.setHidesOnDeactivate(false);
    }

    /// Order in front and take key status WITHOUT activating the app.
    /// (Tauri's `set_focus` also calls `NSApp.activate`, which would steal
    /// focus from the app we want to paste into.)
    pub fn show_picker(w: &WebviewWindow) {
        let Some(win) = ns_window(w) else {
            let _ = w.show();
            return;
        };
        win.orderFrontRegardless();
        win.makeKeyWindow();
    }
}

fn placement(
    app: &AppHandle,
    w: &WebviewWindow,
    mode: PickerMode,
    anchor: Option<Rect>,
) -> Option<PhysicalPosition<i32>> {
    let size = w.outer_size().ok()?;
    let (pw, ph) = (size.width as f64, size.height as f64);

    match (mode, anchor) {
        (PickerMode::Flyout, Some(rect)) => {
            let scale = w.scale_factor().unwrap_or(1.0);
            let pos = rect.position.to_physical::<f64>(scale);
            let sz = rect.size.to_physical::<f64>(scale);
            let (cx, cy) = (pos.x + sz.width / 2.0, pos.y + sz.height / 2.0);
            let mon = app.monitor_from_point(cx, cy).ok().flatten()?;
            let wa = *mon.work_area();
            let margin = FLYOUT_MARGIN * mon.scale_factor();
            let (left, top, right, bottom) = bounds(&wa);
            let x = (cx - pw / 2.0).clamp(left + margin, (right - pw - margin).max(left + margin));
            // Taskbar at the top → drop down; otherwise sit above the taskbar.
            let y = if cy < top + (bottom - top) / 2.0 {
                top + margin
            } else {
                bottom - ph - margin
            };
            Some(PhysicalPosition::new(x.round() as i32, y.round() as i32))
        }
        _ => {
            let cursor = app.cursor_position().ok()?;
            let mon = app
                .monitor_from_point(cursor.x, cursor.y)
                .ok()
                .flatten()
                .or_else(|| app.primary_monitor().ok().flatten())?;
            let (left, top, right, bottom) = bounds(mon.work_area());
            let x = left + ((right - left) - pw) / 2.0;
            // Slightly above center reads better for a command palette.
            let y = top + ((bottom - top) - ph) * 0.4;
            Some(PhysicalPosition::new(x.round() as i32, y.round() as i32))
        }
    }
}

fn bounds(r: &PhysicalRect<i32, u32>) -> (f64, f64, f64, f64) {
    let l = r.position.x as f64;
    let t = r.position.y as f64;
    (l, t, l + r.size.width as f64, t + r.size.height as f64)
}
