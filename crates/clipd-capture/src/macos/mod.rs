//! macOS backend.
//!
//! NSPasteboard has no change notification, so the watcher polls
//! `changeCount` on a background thread (~250 ms, cheap) and reads the
//! pasteboard when it moves. Reading priority mirrors Windows/Linux:
//! file URLs, then text (+ HTML flavor), then image. Producers that mark
//! the data as secret/transient (`org.nspasteboard.*`, 1Password) are
//! skipped entirely.
//!
//! Paste-back activates the previously frontmost app and posts Cmd+V via
//! CGEvent — that needs Accessibility permission; `paste_into` fails with
//! instructions when `AXIsProcessTrusted` is false.

mod paste;
mod pasteboard;
mod watcher;

#[cfg(test)]
mod live_tests;

use anyhow::Result;
use clipd_core::RawClip;

use crate::{ForeignWindow, WriteContent};

/// Running watcher; stops its thread when dropped.
pub struct Watcher {
    _inner: watcher::Watcher,
}

impl Watcher {
    pub fn spawn<F>(on_clip: F) -> Result<Self>
    where
        F: Fn(RawClip) + Send + 'static,
    {
        Ok(Self {
            _inner: watcher::Watcher::spawn(on_clip)?,
        })
    }
}

pub fn read_current() -> Result<Option<RawClip>> {
    pasteboard::read_current()
}

pub fn write_clip(content: WriteContent<'_>) -> Result<()> {
    pasteboard::write_clip(content)
}

pub fn remember_foreground() -> Option<ForeignWindow> {
    paste::remember_foreground()
}

pub fn paste_into(target: ForeignWindow) -> Result<()> {
    paste::paste_into(target)
}

/// Whether the app may post synthetic events (Cmd+V paste-back).
/// The UI polls this to show the one-time permission explainer.
pub fn accessibility_trusted() -> bool {
    paste::trusted()
}
