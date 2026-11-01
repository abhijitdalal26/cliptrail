//! Linux backend.
//!
//! Display server selection (cached after the first successful probe):
//!
//! 1. `WAYLAND_DISPLAY` set and the compositor implements a data-control
//!    protocol (`ext-data-control` / `wlr-data-control`: KDE Plasma, Sway,
//!    Hyprland, ...): native Wayland through `wl-clipboard-rs`.
//! 2. Otherwise `DISPLAY` set: X11 through `x11rb`. On GNOME/Mutter (no
//!    data-control) this means XWayland, which only sees clipboard changes
//!    while an X11 window has focus. That is a compositor limitation.
//! 3. Neither: every call fails with a clear error.
//!
//! Paste-back (focus + synthetic Ctrl+V) exists on real X11 only. On Wayland
//! `remember_foreground` is None and `paste_into` fails, so the UI falls back
//! to "Copied, press Ctrl+V".

mod common;
mod wayland;
mod x11;

use std::sync::OnceLock;

use anyhow::{Result, bail};
use clipd_core::RawClip;

use crate::{ForeignWindow, WriteContent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    /// A real X11 session.
    X11,
    /// A Wayland session with a data-control protocol.
    Wayland,
    /// A Wayland session without data-control; X11 clients only (XWayland).
    XWayland,
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

fn backend() -> Result<Backend> {
    static BACKEND: OnceLock<Backend> = OnceLock::new();
    if let Some(b) = BACKEND.get() {
        return Ok(*b);
    }
    let b = detect()?;
    tracing::debug!(backend = ?b, "clipboard backend selected");
    Ok(*BACKEND.get_or_init(|| b))
}

fn detect() -> Result<Backend> {
    let has_x11 = env_set("DISPLAY");
    if env_set("WAYLAND_DISPLAY") {
        match wayland::probe() {
            Ok(()) => return Ok(Backend::Wayland),
            Err(e) if has_x11 => {
                tracing::warn!(
                    error = %e,
                    "Wayland data-control unavailable; falling back to XWayland \
                     (clipboard changes are only seen while an X11 window has focus)"
                );
                return Ok(Backend::XWayland);
            }
            Err(e) => bail!(
                "this Wayland compositor cannot be used for clipboard history ({e:#}); \
                 GNOME/Mutter has no data-control protocol and DISPLAY (XWayland) is not set"
            ),
        }
    }
    if has_x11 {
        Ok(Backend::X11)
    } else {
        bail!("no display server: neither WAYLAND_DISPLAY nor DISPLAY is set")
    }
}

/// Variants are held only for their `Drop` (stop + join the thread).
#[allow(dead_code)]
enum Inner {
    X11(Box<x11::Watcher>),
    Wayland(wayland::Watcher),
}

/// Running watcher; stops its thread when dropped.
pub struct Watcher {
    _inner: Inner,
}

impl Watcher {
    pub fn spawn<F>(on_clip: F) -> Result<Self>
    where
        F: Fn(RawClip) + Send + 'static,
    {
        let inner = match backend()? {
            Backend::Wayland => Inner::Wayland(wayland::Watcher::spawn(on_clip)?),
            Backend::X11 | Backend::XWayland => Inner::X11(Box::new(x11::Watcher::spawn(on_clip)?)),
        };
        Ok(Watcher { _inner: inner })
    }
}

pub fn read_current() -> Result<Option<RawClip>> {
    match backend()? {
        Backend::Wayland => wayland::read_current(),
        Backend::X11 | Backend::XWayland => x11::read_current(),
    }
}

pub fn write_clip(content: WriteContent<'_>) -> Result<()> {
    let offers = common::offers_for(content)?;
    match backend()? {
        Backend::Wayland => wayland::write(offers),
        Backend::X11 | Backend::XWayland => x11::write(offers),
    }
}

pub fn remember_foreground() -> Option<ForeignWindow> {
    match backend() {
        Ok(Backend::X11) => x11::remember_foreground(),
        _ => None,
    }
}

pub fn paste_into(target: ForeignWindow) -> Result<()> {
    match backend()? {
        Backend::X11 => x11::paste_into(target),
        Backend::Wayland | Backend::XWayland => {
            bail!("paste-back not supported on Wayland")
        }
    }
}
