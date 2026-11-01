//! Desktop auto-update (Windows / Linux / macOS only; Android updates go
//! through the APK/store).
//!
//! Privacy: an update check is a plain HTTPS GET of the static `latest.json`
//! on GitHub Releases (endpoint in `tauri.conf.json`). Nothing about the
//! user, the device or any clip is sent, and the background check is OFF
//! until the user turns it on (`Settings::auto_update_check`). The manual
//! "Check for updates" button always works. Updates are never installed
//! silently: the UI asks, and the package signature is verified against the
//! public key in `tauri.conf.json` before anything is run.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::state::{AppState, lock};

type CmdResult<T> = Result<T, String>;

/// First background check this long after launch, then once a day.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the background thread looks at the setting.
const TICK: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct UpdaterState {
    /// The update found by the last check, kept so "Install" can use it.
    pending: Mutex<Option<Update>>,
    /// Serializes checks / installs.
    op: tokio::sync::Mutex<()>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

impl UpdateInfo {
    fn from(u: &Update) -> UpdateInfo {
        UpdateInfo {
            version: u.version.clone(),
            current_version: u.current_version.clone(),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
            date: u.date.map(|d| d.to_string()),
        }
    }
}

/// Result of a check. `status`: "available" | "upToDate" | "failed".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    status: &'static str,
    info: Option<UpdateInfo>,
    message: Option<String>,
}

impl CheckResult {
    fn up_to_date() -> CheckResult {
        CheckResult {
            status: "upToDate",
            info: None,
            message: None,
        }
    }

    fn failed(message: &str) -> CheckResult {
        CheckResult {
            status: "failed",
            info: None,
            message: Some(message.to_owned()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct Progress {
    /// "downloading" | "installing"
    phase: &'static str,
    downloaded: u64,
    total: Option<u64>,
}

fn friendly(e: &tauri_plugin_updater::Error) -> CheckResult {
    use tauri_plugin_updater::Error;
    match e {
        // GitHub answers 404 until the first release with a latest.json exists.
        Error::ReleaseNotFound | Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            CheckResult::up_to_date()
        }
        Error::Reqwest(r) if r.is_connect() || r.is_timeout() || r.is_request() => {
            CheckResult::failed("Couldn't reach GitHub. Are you offline?")
        }
        _ => CheckResult::failed("Couldn't check for updates right now."),
    }
}

/// Ask GitHub whether a newer version exists. Errors never include clip
/// data (there is none in this path).
async fn check_now(app: &AppHandle) -> CheckResult {
    let state = app.state::<UpdaterState>();
    let _guard = state.op.lock().await;
    let updater = match app.updater_builder().timeout(REQUEST_TIMEOUT).build() {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!("updater unavailable: {e}");
            return CheckResult::failed("Updates aren't available in this build.");
        }
    };
    match updater.check().await {
        Ok(Some(update)) => {
            let info = UpdateInfo::from(&update);
            tracing::info!(version = %info.version, "update available");
            *lock(&state.pending) = Some(update);
            CheckResult {
                status: "available",
                info: Some(info),
                message: None,
            }
        }
        Ok(None) => {
            *lock(&state.pending) = None;
            CheckResult::up_to_date()
        }
        Err(e) => {
            tracing::info!("update check failed: {e}");
            friendly(&e)
        }
    }
}

/// Manual "Check for updates". Works regardless of the automatic setting.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> CheckResult {
    let r = check_now(&app).await;
    if let Some(info) = &r.info {
        let _ = app.emit("update-available", info);
    }
    r
}

/// The update found earlier (by a background or manual check), if any.
#[tauri::command]
pub fn update_pending(app: AppHandle) -> Option<UpdateInfo> {
    lock(&app.state::<UpdaterState>().pending)
        .as_ref()
        .map(UpdateInfo::from)
}

/// Download, verify and install the pending update, then restart. Progress
/// goes out as `update-progress`. Only ever called from a user click.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<UpdaterState>();
    let _guard = state.op.lock().await;
    let update = lock(&state.pending)
        .clone()
        .ok_or_else(|| "No update is ready to install. Check for updates first.".to_string())?;

    let mut downloaded: u64 = 0;
    let app_chunk = app.clone();
    let app_done = app.clone();
    let result = update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = app_chunk.emit(
                    "update-progress",
                    Progress {
                        phase: "downloading",
                        downloaded,
                        total,
                    },
                );
            },
            move || {
                let _ = app_done.emit(
                    "update-progress",
                    Progress {
                        phase: "installing",
                        downloaded: 0,
                        total: None,
                    },
                );
            },
        )
        .await;
    match result {
        Ok(()) => {
            *lock(&state.pending) = None;
            // Windows exits inside the installer; elsewhere relaunch now.
            app.restart()
        }
        Err(e) => {
            tracing::warn!("update install failed: {e}");
            Err("The update couldn't be installed. Please try again later.".into())
        }
    }
}

/// Background checks, only while "Check for updates automatically" is on:
/// ~30 s after launch, then every 24 h. Looks at the setting every tick, so
/// turning it on starts checking within a minute and off stops immediately.
pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("cliptrail-updater".into())
        .spawn(move || {
            let started = Instant::now();
            let mut last: Option<Instant> = None;
            loop {
                std::thread::sleep(TICK);
                if app.state::<AppState>().settings().auto_update_check != Some(true) {
                    continue;
                }
                let due = match last {
                    None => started.elapsed() >= FIRST_CHECK_DELAY,
                    Some(t) => t.elapsed() >= CHECK_EVERY,
                };
                if !due {
                    continue;
                }
                last = Some(Instant::now());
                let r = tauri::async_runtime::block_on(check_now(&app));
                if let Some(info) = &r.info {
                    let _ = app.emit("update-available", info);
                }
            }
        })
        .expect("spawn updater thread");
}
