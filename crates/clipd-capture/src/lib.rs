//! Desktop platform glue. STUB: signatures are the contract.
//!
//! - [`spawn_watcher`]: background clipboard watcher → `RawClip`s
//! - [`write_clip`]: put a stored clip back on the OS clipboard
//! - [`remember_foreground`] / [`paste_into`]: paste-back into the app the
//!   user was in before our picker opened
//!
//! Windows is the first-class target; other OSes may return
//! "unsupported" errors for now.

use anyhow::Result;
use clipd_core::RawClip;

/// Running watcher. Dropping it (or calling `stop`) stops the thread.
pub struct WatcherHandle {
    _private: (),
}

impl WatcherHandle {
    pub fn stop(self) {}
}

/// Start watching the system clipboard on a background thread. `on_clip`
/// is called once per clipboard change (never for changes made by a
/// password manager that sets the Windows exclusion formats).
pub fn spawn_watcher<F>(on_clip: F) -> Result<WatcherHandle>
where
    F: Fn(RawClip) + Send + 'static,
{
    let _ = on_clip;
    todo!()
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
    let _ = content;
    todo!()
}

/// Opaque handle to a native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignWindow(pub isize);

/// Record the currently focused window (call right before showing the
/// picker). None if it's one of our own windows or unknown.
pub fn remember_foreground() -> Option<ForeignWindow> {
    todo!()
}

/// Focus `target`, wait for focus to settle, then send Ctrl+V.
pub fn paste_into(target: ForeignWindow) -> Result<()> {
    let _ = target;
    todo!()
}
