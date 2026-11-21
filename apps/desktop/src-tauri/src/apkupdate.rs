//! Android update check. A sideloaded app can't update itself, so this only
//! finds out whether a newer release exists; the UI then offers the APK
//! download in the browser and Android installs it over the old one (same
//! signing key, history kept).
//!
//! Privacy: like the desktop updater, a plain HTTPS GET of the static
//! `latest.json` on GitHub Releases. Nothing about the user, the device or
//! any clip is sent, and the app only checks when the user turned automatic
//! checks on (`Settings::auto_update_check`) or taps "Check now".

use serde::Serialize;

const REPO: &str = "https://github.com/abhijitdalal26/cliptrail";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkUpdate {
    pub available: bool,
    pub version: String,
    pub current_version: String,
    pub apk_url: String,
    pub notes_url: String,
}

/// True when `candidate` ("0.1.10") is a newer release than `current`.
/// Numeric parts are compared in order; a missing part counts as 0.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split(['.', '-', '+'])
            .map_while(|p| p.parse().ok())
            .collect()
    }
    let (a, b) = (parts(candidate), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn android_update_check() -> Result<ApkUpdate, String> {
    let fail = |_| "Couldn't reach GitHub to check for updates".to_string();
    let client = clipd_sync::relay_link::http_builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(fail)?;
    let body = client
        .get(format!("{REPO}/releases/latest/download/latest.json"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(fail)?
        .text()
        .await
        .map_err(fail)?;
    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| "The update information was unreadable".to_string())?;
    let version = json["version"]
        .as_str()
        .ok_or("The update information was unreadable")?
        .trim_start_matches('v')
        .to_owned();
    let current = env!("CARGO_PKG_VERSION");
    Ok(ApkUpdate {
        available: is_newer(&version, current),
        apk_url: format!(
            "{REPO}/releases/download/v{version}/ClipTrail_{version}_android-arm64.apk"
        ),
        notes_url: format!("{REPO}/releases/tag/v{version}"),
        current_version: current.to_owned(),
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("0.1.5", "0.1.4"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("v0.2.0", "0.1.99"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.4", "0.1.4"));
        assert!(!is_newer("0.1.3", "0.1.4"));
        assert!(!is_newer("0.1.4", "0.1.4.0"));
        assert!(!is_newer("garbage", "0.1.4"));
    }
}
