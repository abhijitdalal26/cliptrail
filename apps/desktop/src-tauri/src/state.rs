//! Shared app state: the pipeline, settings, capture pause, paste target.
//!
//! [`Core`] (pipeline, settings, pause) is what every platform needs. On
//! Android it is process-wide (see `mobile::core`), because the Kotlin
//! capture services ingest through it while no UI is running.
//! [`AppState`] wraps it for Tauri and adds desktop-only window state.

use std::ops::Deref;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
#[cfg(desktop)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once, RwLock};
#[cfg(desktop)]
use std::time::Instant;

#[cfg(desktop)]
use clipd_capture::{ForeignWindow, WatcherHandle};
use clipd_core::{Millis, Pipeline, PipelineConfig};
use serde::Serialize;
#[cfg(desktop)]
use tauri_plugin_global_shortcut::Shortcut;

use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    Off,
    Until(Millis),
    Indefinite,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureState {
    pub paused: bool,
    /// Unix ms when capture resumes on its own; None = until resumed.
    pub until: Option<Millis>,
}

#[cfg(desktop)]
#[derive(Default)]
pub struct Hotkeys {
    pub picker: Option<Shortcut>,
    pub dashboard: Option<Shortcut>,
}

/// Pipeline + settings + capture pause: everything capture and the UI share.
pub struct Core {
    pub data_dir: PathBuf,
    pipeline: RwLock<Option<Arc<Pipeline>>>,
    init_error: Mutex<Option<String>>,
    pub settings: Mutex<Settings>,
    /// Excluded apps the core ships by default (shown in Settings).
    pub default_excluded: Mutex<Vec<String>>,
    pause: Mutex<Pause>,
    init_once: Once,
}

pub struct AppState {
    core: Arc<Core>,
    /// Window to paste into when the picker confirms.
    #[cfg(desktop)]
    pub paste_target: Mutex<Option<ForeignWindow>>,
    /// Foreground window seen while hovering the tray icon (clicking the tray
    /// focuses the taskbar, so we capture the real target before that).
    #[cfg(desktop)]
    pub hover_target: Mutex<Option<(ForeignWindow, Instant)>>,
    /// When the picker was last hidden (to make tray clicks toggle cleanly).
    #[cfg(desktop)]
    pub picker_hidden_at: Mutex<Option<Instant>>,
    #[cfg(desktop)]
    pub main_effect: AtomicBool,
    #[cfg(desktop)]
    pub picker_effect: AtomicBool,
    #[cfg(desktop)]
    pub watcher: Mutex<Option<WatcherHandle>>,
    #[cfg(desktop)]
    pub hotkeys: Mutex<Hotkeys>,
}

impl Deref for AppState {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.core
    }
}

/// Lock a mutex, recovering from poisoning (a panicked holder never leaves
/// our state half-written in a way that matters here).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Run `f`, turning a panic into an error. Keeps the app alive while the
/// core / capture crates still have unimplemented stubs.
pub fn guard<T>(what: &str, f: impl FnOnce() -> anyhow::Result<T>) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(format!("{what}: {e:#}")),
        Err(_) => Err(format!("{what}: not available (internal panic)")),
    }
}

impl AppState {
    #[cfg(desktop)]
    pub fn new(data_dir: PathBuf) -> AppState {
        AppState::with_core(Arc::new(Core::new(data_dir)))
    }

    pub fn with_core(core: Arc<Core>) -> AppState {
        AppState {
            core,
            #[cfg(desktop)]
            paste_target: Mutex::new(None),
            #[cfg(desktop)]
            hover_target: Mutex::new(None),
            #[cfg(desktop)]
            picker_hidden_at: Mutex::new(None),
            #[cfg(desktop)]
            main_effect: AtomicBool::new(false),
            #[cfg(desktop)]
            picker_effect: AtomicBool::new(false),
            #[cfg(desktop)]
            watcher: Mutex::new(None),
            #[cfg(desktop)]
            hotkeys: Mutex::new(Hotkeys::default()),
        }
    }

    #[cfg(desktop)]
    pub fn effect(&self, label: &str) -> bool {
        match label {
            "main" => self.main_effect.load(Ordering::Relaxed),
            "picker" => self.picker_effect.load(Ordering::Relaxed),
            _ => false,
        }
    }
}

impl Core {
    pub fn new(data_dir: PathBuf) -> Core {
        Core {
            data_dir,
            pipeline: RwLock::new(None),
            init_error: Mutex::new(None),
            settings: Mutex::new(Settings::default()),
            default_excluded: Mutex::new(Vec::new()),
            pause: Mutex::new(Pause::Off),
            init_once: Once::new(),
        }
    }

    /// Load settings (creating them on first run) and open the pipeline.
    /// Runs once; later calls are no-ops (Android initializes the core from
    /// whichever component the OS starts first).
    pub fn init(&self) {
        self.init_once.call_once(|| self.load());
    }

    fn load(&self) {
        let loaded = Settings::load(&self.data_dir);
        let settings = match loaded {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("ignoring unreadable settings: {e:#}");
                None
            }
        };

        let base = guard("config", || {
            PipelineConfig::with_defaults(self.data_dir.clone())
        });
        let settings = match (&base, settings) {
            (Ok(cfg), None) => {
                // First run: seed settings from the core defaults.
                let s = Settings {
                    excluded_apps: cfg.excluded_apps.clone(),
                    sensitive_ttl_minutes: (cfg.sensitive_ttl_ms / 60_000).max(1) as u32,
                    ..Settings::default()
                }
                .normalized();
                if let Err(e) = s.save(&self.data_dir) {
                    tracing::warn!("couldn't save settings: {e:#}");
                }
                s
            }
            (_, Some(s)) => s.normalized(),
            (Err(_), None) => Settings::default(),
        };
        if let Ok(cfg) = &base {
            *lock(&self.default_excluded) = cfg.excluded_apps.clone();
        }
        *lock(&self.settings) = settings.clone();

        match self.open_pipeline(&settings) {
            Ok(p) => {
                *self.pipeline.write().unwrap_or_else(|p| p.into_inner()) = Some(p);
                *lock(&self.init_error) = None;
            }
            Err(e) => {
                tracing::error!("pipeline unavailable: {e}");
                *lock(&self.init_error) = Some(e);
            }
        }
    }

    fn open_pipeline(&self, s: &Settings) -> Result<Arc<Pipeline>, String> {
        guard("opening the clip store", || {
            let mut cfg = PipelineConfig::with_defaults(self.data_dir.clone())?;
            cfg.excluded_apps = s.excluded_apps.clone();
            cfg.sensitive_ttl_ms = s.sensitive_ttl_minutes as Millis * 60_000;
            cfg.max_image_bytes = if s.max_image_mb == 0 {
                usize::MAX
            } else {
                s.max_image_mb as usize * 1024 * 1024
            };
            Ok(Arc::new(Pipeline::open(cfg)?))
        })
    }

    /// Reopen the pipeline with new config (excluded apps / sensitive TTL).
    pub fn reload_pipeline(&self, s: &Settings) -> Result<(), String> {
        let p = self.open_pipeline(s)?;
        *self.pipeline.write().unwrap_or_else(|p| p.into_inner()) = Some(p);
        *lock(&self.init_error) = None;
        Ok(())
    }

    pub fn pipeline(&self) -> Result<Arc<Pipeline>, String> {
        self.pipeline
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or_else(|| {
                lock(&self.init_error)
                    .clone()
                    .unwrap_or_else(|| "clip store is not ready".into())
            })
    }

    pub fn init_error(&self) -> Option<String> {
        lock(&self.init_error).clone()
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn capture_state(&self) -> CaptureState {
        let mut p = lock(&self.pause);
        if let Pause::Until(t) = *p
            && clipd_core::now_millis() >= t
        {
            *p = Pause::Off;
        }
        match *p {
            Pause::Off => CaptureState {
                paused: false,
                until: None,
            },
            Pause::Until(t) => CaptureState {
                paused: true,
                until: Some(t),
            },
            Pause::Indefinite => CaptureState {
                paused: true,
                until: None,
            },
        }
    }

    pub fn is_paused(&self) -> bool {
        self.capture_state().paused
    }

    pub fn set_pause(&self, p: Pause) {
        *lock(&self.pause) = p;
    }
}
