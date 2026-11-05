//! Background housekeeping: expire sensitive clips, apply the retention
//! setting, auto-resume a timed pause, keep the tray tooltip fresh.

use std::time::Duration;

use clipd_core::{ListQuery, Millis};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::notify_updated;
use crate::state::{AppState, guard};
use crate::tray;

const TICK: Duration = Duration::from_secs(20);
/// Prune / retention every N ticks (~5 minutes).
const PRUNE_EVERY: u32 = 15;
const DAY_MS: Millis = 86_400_000;

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("cliptrail-maintenance".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            let mut was_paused = app.state::<AppState>().is_paused();
            let mut tick = 0u32;
            loop {
                if tick.is_multiple_of(PRUNE_EVERY) {
                    prune(&app);
                    apply_retention(&app);
                    tray::refresh(&app);
                }
                let state = app.state::<AppState>();
                let paused = state.is_paused();
                if paused != was_paused {
                    // A timed pause ran out.
                    let _ = app.emit("capture-changed", state.capture_state());
                    tray::refresh(&app);
                    was_paused = paused;
                }
                tick = tick.wrapping_add(1);
                std::thread::sleep(TICK);
            }
        })
        .expect("spawn maintenance thread");
}

fn prune(app: &AppHandle) {
    let Ok(p) = app.state::<AppState>().pipeline() else {
        return;
    };
    if let Err(e) = guard("trash", || p.purge_old_trash()) {
        tracing::warn!("{e}");
    }
    match guard("prune", || p.prune()) {
        Ok(0) => {}
        Ok(n) => {
            tracing::info!(removed = n, "pruned expired clips");
            notify_updated(app);
        }
        Err(e) => tracing::warn!("{e}"),
    }
}

/// Delete unpinned clips older than the retention window.
pub fn apply_retention(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(days) = state.settings().retention_days else {
        return;
    };
    let Ok(p) = state.pipeline() else { return };
    let cutoff = clipd_core::now_millis() - days as Millis * DAY_MS;

    let mut before: Option<Millis> = None;
    let mut before_id: Option<String> = None;
    let mut removed = 0usize;
    loop {
        let q = ListQuery {
            to: Some(cutoff),
            before,
            before_id: before_id.clone(),
            limit: Some(500),
            ..Default::default()
        };
        let rows = match guard("retention list", || p.store().list(&q)) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("{e}");
                break;
            }
        };
        for c in rows.iter().filter(|c| !c.pinned) {
            // Retention is per device: don't delete the clip on synced devices.
            if guard("retention delete", || p.delete_local(&c.id)).is_ok() {
                removed += 1;
            }
        }
        match rows.last() {
            Some(last) if rows.len() == 500 => {
                before = Some(last.last_seen_at);
                before_id = Some(last.id.clone());
            }
            _ => break,
        }
    }
    if removed > 0 {
        tracing::info!(removed, days, "applied retention");
        notify_updated(app);
    }
}
