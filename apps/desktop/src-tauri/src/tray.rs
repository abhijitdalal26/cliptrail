//! Notification-area icon: left click = picker flyout, right click = menu.

use chrono::{Duration as ChronoDuration, Local, NaiveTime};
use clipd_core::{ClipSummary, Kind, ListQuery};
use tauri::image::Image;
use tauri::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::commands;
use crate::state::{AppState, Pause};
use crate::windows::{self, PickerMode};

const TRAY_ID: &str = "cliptrail";
const ICON: &[u8] = include_bytes!("../icons/tray.png");
const ICON_PAUSED: &[u8] = include_bytes!("../icons/tray-paused.png");

#[cfg(target_os = "windows")]
const AUTOSTART_LABEL: &str = "Start with Windows";
#[cfg(not(target_os = "windows"))]
const AUTOSTART_LABEL: &str = "Start at login";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    #[allow(unused_mut)]
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON)?)
        .tooltip("ClipTrail")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(on_tray);
    // Template icon: macOS tints it to fit light and dark menu bars.
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true);
    }
    let tray = builder.build(app)?;
    if !app.state::<AppState>().settings().show_tray_icon {
        let _ = tray.set_visible(false);
    }
    refresh(app);
    Ok(())
}

/// Show or hide the icon ("Show tray icon" setting). Capture keeps running.
pub fn set_visible(app: &AppHandle, visible: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(visible);
    }
}

fn on_tray(tray: &TrayIcon, event: TrayIconEvent) {
    let app = tray.app_handle();
    match event {
        TrayIconEvent::Enter { .. } => windows::note_tray_hover(app),
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            rect,
            ..
        } => windows::toggle_picker(app, PickerMode::Flyout, Some(rect)),
        _ => {}
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    let state = app.state::<AppState>();
    match id {
        "open" => windows::show_dashboard(app),
        "picker" => windows::toggle_picker(app, PickerMode::Center, None),
        "pause:15" => commands::pause(app, Pause::Until(clipd_core::now_millis() + 15 * 60_000)),
        "pause:60" => commands::pause(app, Pause::Until(clipd_core::now_millis() + 60 * 60_000)),
        "pause:forever" => commands::pause(app, Pause::Indefinite),
        "resume" => commands::pause(app, Pause::Off),
        "autostart" => {
            let al = app.autolaunch();
            let on = al.is_enabled().unwrap_or(false);
            let _ = if on { al.disable() } else { al.enable() };
            let _ = app.emit("settings-changed", ());
            refresh(app);
        }
        "settings" => {
            windows::show_dashboard(app);
            let _ = app.emit_to(windows::MAIN, "open-settings", ());
        }
        "quit" => app.exit(0),
        other => {
            if let Some(clip_id) = other.strip_prefix("recent:")
                && let Ok(p) = state.pipeline()
            {
                let id = clip_id.to_string();
                std::thread::spawn(move || {
                    if let Err(e) = commands::write_back(&p, &id, false) {
                        tracing::warn!("tray copy failed: {e}");
                    }
                });
            }
        }
    }
}

/// Rebuild menu, tooltip and icon from current state.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let paused = app.state::<AppState>().capture_state().paused;
    if let Ok(menu) = build_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    let tip = if paused {
        "ClipTrail — capture paused".to_string()
    } else {
        match clips_today(app) {
            Some(1) => "ClipTrail — 1 clip today".to_string(),
            Some(n) => format!("ClipTrail — {n} clips today"),
            None => "ClipTrail".to_string(),
        }
    };
    let _ = tray.set_tooltip(Some(&tip));
    if let Ok(img) = Image::from_bytes(if paused { ICON_PAUSED } else { ICON }) {
        let _ = tray.set_icon(Some(img));
    }
}

fn clips_today(app: &AppHandle) -> Option<i64> {
    let p = app.state::<AppState>().pipeline().ok()?;
    let start = Local::now()
        .date_naive()
        .and_time(NaiveTime::MIN)
        .and_local_timezone(Local)
        .earliest()?;
    let end = start + ChronoDuration::days(1);
    let counts = crate::state::guard("day_counts", || {
        p.store()
            .day_counts(start.timestamp_millis(), end.timestamp_millis())
    })
    .ok()?;
    Some(counts.iter().map(|d| d.count).sum())
}

fn recent(app: &AppHandle) -> Vec<ClipSummary> {
    let Ok(p) = app.state::<AppState>().pipeline() else {
        return Vec::new();
    };
    let q = ListQuery {
        limit: Some(25),
        ..Default::default()
    };
    crate::state::guard("list", || p.store().list(&q))
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.kind != Kind::Sensitive && !c.masked)
        .take(5)
        .collect()
}

/// One-line, ~40 char menu label. Ampersands are doubled so Windows menus
/// don't treat them as mnemonics.
fn label_for(c: &ClipSummary) -> String {
    let raw = match c.kind {
        Kind::Image => match (c.width, c.height) {
            (Some(w), Some(h)) => format!("Image · {w}×{h}"),
            _ => "Image".to_string(),
        },
        Kind::File => {
            let first = c
                .files
                .first()
                .map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).to_string());
            match (first, c.files.len()) {
                (Some(f), 1) => f,
                (Some(f), n) => format!("{f} +{}", n - 1),
                (None, _) => c.preview.clone(),
            }
        }
        _ => c.preview.split_whitespace().collect::<Vec<_>>().join(" "),
    };
    let mut s: String = raw.chars().take(40).collect();
    if raw.chars().count() > 40 {
        s = s.trim_end().to_string();
        s.push('…');
    }
    if s.is_empty() {
        s = "(empty)".into();
    }
    s.replace('&', "&&")
}

fn item(
    app: &AppHandle,
    id: &str,
    text: &str,
    accel: Option<&str>,
) -> tauri::Result<MenuItem<Wry>> {
    match accel {
        Some(a) => MenuItem::with_id(app, id, text, true, Some(a))
            .or_else(|_| MenuItem::with_id(app, id, text, true, None::<&str>)),
        None => MenuItem::with_id(app, id, text, true, None::<&str>),
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let capture = state.capture_state();

    let open = item(app, "open", "Open ClipTrail", None)?;
    let picker = item(
        app,
        "picker",
        "Quick picker",
        Some(&settings.picker_shortcut),
    )?;

    let recent_clips = recent(app);
    let recent_items: Vec<MenuItem<Wry>> = recent_clips
        .iter()
        .map(|c| item(app, &format!("recent:{}", c.id), &label_for(c), None))
        .collect::<tauri::Result<_>>()?;
    let empty = MenuItem::with_id(
        app,
        "recent:none",
        "Nothing copied yet",
        false,
        None::<&str>,
    )?;
    let recent_refs: Vec<&dyn IsMenuItem<Wry>> = if recent_items.is_empty() {
        vec![&empty]
    } else {
        recent_items
            .iter()
            .map(|i| i as &dyn IsMenuItem<Wry>)
            .collect()
    };
    let recent_menu = Submenu::with_id_and_items(app, "recent", "Recent", true, &recent_refs)?;

    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;

    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        AUTOSTART_LABEL,
        true,
        autostart_on,
        None::<&str>,
    )?;
    let settings_item = item(app, "settings", "Settings…", None)?;
    let quit = item(app, "quit", "Quit ClipTrail", None)?;

    let pause_or_resume: Box<dyn IsMenuItem<Wry>> = if capture.paused {
        let label = match capture.until {
            Some(t) => {
                let when = chrono::DateTime::from_timestamp_millis(t)
                    .map(|d| d.with_timezone(&Local).format("%H:%M").to_string())
                    .unwrap_or_default();
                format!("Resume capture (paused until {when})")
            }
            None => "Resume capture".to_string(),
        };
        Box::new(item(app, "resume", &label, None)?)
    } else {
        let p15 = item(app, "pause:15", "For 15 minutes", None)?;
        let p60 = item(app, "pause:60", "For 1 hour", None)?;
        let pf = item(app, "pause:forever", "Until resumed", None)?;
        Box::new(Submenu::with_id_and_items(
            app,
            "pause",
            "Pause capture",
            true,
            &[&p15, &p60, &pf],
        )?)
    };

    Menu::with_items(
        app,
        &[
            &open,
            &picker,
            &recent_menu,
            &sep1,
            pause_or_resume.as_ref(),
            &autostart,
            &sep2,
            &settings_item,
            &sep3,
            &quit,
        ],
    )
}
