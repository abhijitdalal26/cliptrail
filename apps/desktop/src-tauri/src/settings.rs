//! User settings persisted as `settings.json` in the data dir.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default global hotkey for the quick picker. Change here to change the default.
/// macOS uses Cmd (Win/Linux Ctrl): Spotlight is Cmd+Space, so Cmd+Shift+Space
/// is the classic clipboard-picker slot.
#[cfg(target_os = "macos")]
pub const DEFAULT_PICKER_SHORTCUT: &str = "Cmd+Shift+Space";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_PICKER_SHORTCUT: &str = "Ctrl+Shift+Space";
/// Default global hotkey for the dashboard (rarely bound by other apps).
/// Not Cmd+Alt+H on macOS: that is "Hide Others" in nearly every app.
#[cfg(target_os = "macos")]
pub const DEFAULT_DASHBOARD_SHORTCUT: &str = "Cmd+Ctrl+H";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_DASHBOARD_SHORTCUT: &str = "Ctrl+Alt+H";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub picker_shortcut: String,
    /// None = no dashboard hotkey.
    pub dashboard_shortcut: Option<String>,
    pub theme: Theme,
    /// Delete unpinned clips older than this many days. None = keep forever.
    pub retention_days: Option<u32>,
    /// Copied images that encode to a PNG bigger than this many megabytes
    /// are kept thumbnail-only. 0 = no limit.
    pub max_image_mb: u32,
    /// Lifetime of sensitive clips (OTPs, keys) in minutes.
    pub sensitive_ttl_minutes: u32,
    /// Lowercased executable / process names whose copies are never stored.
    pub excluded_apps: Vec<String>,
    /// Strip tracking parameters from copied links before they are stored.
    pub clean_links: bool,
    /// Run the LAN sync hub so paired devices share history.
    pub sync_enabled: bool,
    /// TCP port of the sync hub.
    pub sync_port: u16,
    /// Background update check (contacts GitHub). None = never asked yet,
    /// the dashboard offers it once; Some(false) = declined.
    pub auto_update_check: Option<bool>,
    /// Show the notification-area / menu-bar icon. Off: ClipTrail keeps
    /// running and opens from its hotkeys or by launching it again.
    pub show_tray_icon: bool,
    /// The first-launch welcome card was dismissed.
    pub welcome_seen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            picker_shortcut: DEFAULT_PICKER_SHORTCUT.into(),
            dashboard_shortcut: Some(DEFAULT_DASHBOARD_SHORTCUT.into()),
            theme: Theme::System,
            retention_days: None,
            max_image_mb: 8,
            sensitive_ttl_minutes: 60 * 24,
            excluded_apps: Vec::new(),
            clean_links: false,
            sync_enabled: true,
            sync_port: clipd_sync::DEFAULT_PORT,
            auto_update_check: None,
            show_tray_icon: true,
            welcome_seen: false,
        }
    }
}

impl Settings {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("settings.json")
    }

    /// Load settings, or None when the file doesn't exist yet.
    pub fn load(data_dir: &Path) -> Result<Option<Settings>> {
        let path = Self::path(data_dir);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(
                serde_json::from_slice(&bytes).context("settings.json is not valid")?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("reading settings.json"),
        }
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(data_dir).context("creating data dir")?;
        let path = Self::path(data_dir);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?).context("writing settings")?;
        std::fs::rename(&tmp, &path).context("replacing settings.json")?;
        Ok(())
    }

    /// Normalize user input: lowercase + dedupe excluded apps, clamp numbers.
    pub fn normalized(mut self) -> Settings {
        let mut apps: Vec<String> = self
            .excluded_apps
            .iter()
            .map(|a| a.trim().to_lowercase())
            .filter(|a| !a.is_empty())
            .collect();
        apps.sort();
        apps.dedup();
        self.excluded_apps = apps;
        self.sensitive_ttl_minutes = self.sensitive_ttl_minutes.clamp(1, 60 * 24 * 30);
        self.retention_days = self.retention_days.filter(|d| *d > 0);
        self.max_image_mb = self.max_image_mb.min(2048);
        if self.sync_port < 1024 {
            self.sync_port = clipd_sync::DEFAULT_PORT;
        }
        self.picker_shortcut = self.picker_shortcut.trim().to_string();
        self.dashboard_shortcut = self
            .dashboard_shortcut
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_from_older_versions_get_new_defaults() {
        // v0.1.2 wrote neither field: keep the tray icon, show the welcome once.
        let s: Settings = serde_json::from_str(r#"{"picker_shortcut":"Ctrl+Shift+V"}"#).unwrap();
        assert_eq!(s.picker_shortcut, "Ctrl+Shift+V");
        assert!(s.show_tray_icon);
        assert!(!s.welcome_seen);
    }
}
