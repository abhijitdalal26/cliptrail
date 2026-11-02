//! Desktop platform glue.
//!
//! - [`spawn_watcher`]: background clipboard watcher -> `RawClip`s
//! - [`write_clip`]: put a stored clip back on the OS clipboard
//! - [`remember_foreground`] / [`paste_into`]: paste-back into the app the
//!   user was in before our picker opened
//!
//! This file is the public API plus thin dispatch. Each OS backend lives in
//! its own module (`windows/`, `linux/`). Backends are `cfg`-gated; on OSes
//! without a backend every function returns an "unsupported platform" error.
//! Pure, OS-independent helpers (`cfhtml`, `dib`, `dropfiles`, `targets`,
//! `urilist`) are plain public modules.

pub mod cfhtml;
pub mod dib;
pub mod dropfiles;
pub mod targets;
pub mod urilist;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use anyhow::Result;
use clipd_core::RawClip;

/// Running watcher. Dropping it (or calling `stop`) stops the thread.
pub struct WatcherHandle {
    #[cfg(windows)]
    _inner: windows::Watcher,
    #[cfg(target_os = "linux")]
    _inner: linux::Watcher,
    #[cfg(target_os = "macos")]
    _inner: macos::Watcher,
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    _private: (),
}

impl WatcherHandle {
    /// Stop the watcher and wait for its thread to exit.
    pub fn stop(self) {}
}

/// Start watching the system clipboard on a background thread. `on_clip`
/// is called once per clipboard change (never for changes made by a
/// password manager that sets the Windows exclusion formats).
pub fn spawn_watcher<F>(on_clip: F) -> Result<WatcherHandle>
where
    F: Fn(RawClip) + Send + 'static,
{
    #[cfg(windows)]
    {
        Ok(WatcherHandle {
            _inner: windows::Watcher::spawn(on_clip)?,
        })
    }
    #[cfg(target_os = "linux")]
    {
        Ok(WatcherHandle {
            _inner: linux::Watcher::spawn(on_clip)?,
        })
    }
    #[cfg(target_os = "macos")]
    {
        Ok(WatcherHandle {
            _inner: macos::Watcher::spawn(on_clip)?,
        })
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = on_clip;
        anyhow::bail!("unsupported platform")
    }
}

/// Read whatever is on the clipboard right now, using the same rules as the
/// watcher (priority order, exclusion formats). `Ok(None)` if the clipboard
/// is empty, unsupported or excluded.
pub fn read_current() -> Result<Option<RawClip>> {
    #[cfg(windows)]
    {
        windows::read_current()
    }
    #[cfg(target_os = "linux")]
    {
        linux::read_current()
    }
    #[cfg(target_os = "macos")]
    {
        macos::read_current()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        anyhow::bail!("unsupported platform")
    }
}

/// What to put back on the clipboard.
pub enum WriteContent<'a> {
    /// `html` is optional rich flavor; pass None for "paste as plain text".
    Text {
        text: &'a str,
        html: Option<&'a str>,
    },
    /// Path to a PNG file on disk.
    ImagePng(&'a str),
    Files(&'a [String]),
}

pub fn write_clip(content: WriteContent<'_>) -> Result<()> {
    #[cfg(windows)]
    {
        windows::write_clip(content)
    }
    #[cfg(target_os = "linux")]
    {
        linux::write_clip(content)
    }
    #[cfg(target_os = "macos")]
    {
        macos::write_clip(content)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = content;
        anyhow::bail!("unsupported platform")
    }
}

/// Opaque handle to a native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignWindow(pub isize);

/// Record the currently focused window (call right before showing the
/// picker). None if it's one of our own windows or unknown.
pub fn remember_foreground() -> Option<ForeignWindow> {
    #[cfg(windows)]
    {
        windows::remember_foreground()
    }
    #[cfg(target_os = "linux")]
    {
        linux::remember_foreground()
    }
    #[cfg(target_os = "macos")]
    {
        macos::remember_foreground()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Focus `target`, wait for focus to settle, then send Ctrl+V.
pub fn paste_into(target: ForeignWindow) -> Result<()> {
    #[cfg(windows)]
    {
        windows::paste_into(target)
    }
    #[cfg(target_os = "linux")]
    {
        linux::paste_into(target)
    }
    #[cfg(target_os = "macos")]
    {
        macos::paste_into(target)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = target;
        anyhow::bail!("unsupported platform")
    }
}

/// Whether this process may post synthetic input events — required for
/// [`paste_into`]. Always true outside macOS (other OSes gate elsewhere).
/// The desktop UI polls this to show the Accessibility explainer.
pub fn accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::accessibility_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}
