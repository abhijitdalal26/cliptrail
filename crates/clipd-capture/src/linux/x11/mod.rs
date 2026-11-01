//! X11 backend on the pure-Rust `x11rb` crate (no libxcb at build time).
//!
//! - `clip`: a connection + hidden window that can read the CLIPBOARD
//!   selection (including INCR transfers) and resolve the source app
//! - `watcher`: XFixes SelectionNotify driven change detection, debounced
//! - `owner`: owning the selection for `write_clip` (one serving thread per
//!   write; it exits when another client takes the selection)
//! - `paste`: remember the active window; refocus it and fake Ctrl+V (XTest)
//!
//! The whole backend works over XWayland too, but there it only sees
//! clipboard changes made while an X11 window has focus.

mod clip;
mod owner;
mod paste;
mod proc;
mod watcher;

use anyhow::{Context, Result};
use clipd_core::RawClip;
use x11rb::atom_manager;
use x11rb::rust_connection::RustConnection;

pub(super) use owner::write;
pub(super) use paste::{paste_into, remember_foreground};
pub(super) use watcher::Watcher;

use super::common::now_ms;

atom_manager! {
    pub Atoms: AtomsCookie {
        CLIPBOARD,
        TARGETS,
        INCR,
        _NET_WM_PID,
        _NET_ACTIVE_WINDOW,
        WM_CLIENT_LEADER,
        CLIPD_SEL,
        CLIPD_STOP,
    }
}

pub(crate) fn connect() -> Result<(RustConnection, usize)> {
    x11rb::connect(None).context("cannot connect to the X server (is DISPLAY set and reachable?)")
}

/// See [`crate::read_current`].
pub(super) fn read_current() -> Result<Option<RawClip>> {
    use x11rb::protocol::xproto::ConnectionExt;

    let mut clip = clip::X11Clip::connect()?;
    let owner = clip
        .conn
        .get_selection_owner(clip.atoms.CLIPBOARD)?
        .reply()?
        .owner;
    if owner == x11rb::NONE {
        return Ok(None);
    }
    let source_app = clip.source_app(owner);
    let content = clip.read_content(x11rb::CURRENT_TIME)?;
    Ok(content.map(|content| RawClip {
        content,
        source_app,
        captured_at: now_ms(),
    }))
}
