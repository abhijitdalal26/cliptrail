//! Bridge to `ClipTrailPlugin.kt` (gen/android/app/src/main/java/dev/
//! cliptrail/app): Android APIs the UI needs while it's open — clipboard
//! write, settings screens, permission state, QR scan, bar colors.
//! The UI never talks to the Kotlin plugin directly; it calls our Rust
//! commands (see `commands.rs`), which call [`call`].

use serde::Serialize;
use serde::de::DeserializeOwned;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime, Wry};

pub struct Bridge<R: Runtime>(#[allow(dead_code)] PluginHandle<R>);

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("cliptrail")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                let handle = api.register_android_plugin("dev.cliptrail.app", "ClipTrailPlugin")?;
                app.manage(Bridge(handle));
            }
            #[cfg(not(target_os = "android"))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}

/// Run a Kotlin plugin command and wait for its result.
pub async fn call<T: DeserializeOwned>(
    app: &AppHandle,
    command: &str,
    payload: impl Serialize,
) -> Result<T, String> {
    let bridge = app
        .try_state::<Bridge<Wry>>()
        .ok_or_else(|| "Android bridge not available".to_string())?;
    bridge
        .0
        .run_mobile_plugin_async::<T>(command, payload)
        .await
        .map_err(|e| e.to_string())
}
