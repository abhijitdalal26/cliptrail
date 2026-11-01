//! Reading the CLIPBOARD selection over a private connection and window.
//!
//! ICCCM transfer: `ConvertSelection` into a property on our window, wait for
//! `SelectionNotify`, read the property. Large data arrives as INCR: the
//! owner announces type INCR, then sends chunks (each signalled by a
//! `PropertyNotify`) until a zero-length chunk. We cap total size.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clipd_core::RawContent;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt, CreateWindowAux, EventMask, Property, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;

use super::proc::{active_window, pid_of_window_tree, process_name};
use super::{Atoms, connect};
use crate::linux::common::{MAX_BYTES, read_by_plan};

/// Per-step wait for a selection owner to answer.
const STEP_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// Data of one converted target.
pub(super) struct Fetched {
    pub type_: Atom,
    pub format: u8,
    pub data: Vec<u8>,
}

pub(super) struct X11Clip {
    pub conn: RustConnection,
    pub root: Window,
    pub win: Window,
    pub atoms: Atoms,
    /// XFixes events that arrived while we waited for a transfer; the
    /// watcher still has to see them.
    deferred: VecDeque<Event>,
    names: HashMap<Atom, String>,
}

impl X11Clip {
    pub fn connect() -> Result<Self> {
        let (conn, screen_num) = connect()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let win = conn.generate_id()?;
        conn.create_window(
            screen.root_depth,
            win,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        let atoms = Atoms::new(&conn)?.reply().context("interning atoms")?;
        Ok(X11Clip {
            conn,
            root,
            win,
            atoms,
            deferred: VecDeque::new(),
            names: HashMap::new(),
        })
    }

    pub fn take_deferred(&mut self) -> Option<Event> {
        self.deferred.pop_front()
    }

    /// Poll for an event satisfying `pick` until `timeout`. Other events
    /// are dropped, except XFixes selection changes which are kept.
    fn wait_event(&mut self, pick: impl Fn(&Event) -> bool) -> Result<Option<Event>> {
        self.conn.flush()?;
        let deadline = Instant::now() + STEP_TIMEOUT;
        loop {
            match self.conn.poll_for_event()? {
                Some(ev) if pick(&ev) => return Ok(Some(ev)),
                Some(ev @ Event::XfixesSelectionNotify(_)) => self.deferred.push_back(ev),
                Some(_) => {}
                None if Instant::now() >= deadline => return Ok(None),
                None => std::thread::sleep(POLL_INTERVAL),
            }
        }
    }

    /// One `GetProperty(delete)` of our transfer property. `None` if the
    /// payload is larger than the cap (the property is then removed).
    fn read_prop(&self) -> Result<Option<x11rb::protocol::xproto::GetPropertyReply>> {
        let reply = self
            .conn
            .get_property(
                true,
                self.win,
                self.atoms.CLIPD_SEL,
                AtomEnum::ANY,
                0,
                (MAX_BYTES / 4 + 1) as u32,
            )?
            .reply()?;
        if reply.bytes_after > 0 {
            // Not deleted by the server when truncated.
            self.conn.delete_property(self.win, self.atoms.CLIPD_SEL)?;
            tracing::debug!("clipboard payload over the size cap; skipped");
            return Ok(None);
        }
        Ok(Some(reply))
    }

    /// Convert CLIPBOARD to `target` and return the bytes, or None if the
    /// owner refused, timed out or the data is over the size cap.
    pub fn fetch(&mut self, target: Atom, time: u32) -> Result<Option<Fetched>> {
        let (win, prop, selection) = (self.win, self.atoms.CLIPD_SEL, self.atoms.CLIPBOARD);
        self.conn.delete_property(win, prop)?;
        self.conn
            .convert_selection(win, selection, target, prop, time)?;
        let Some(Event::SelectionNotify(n)) = self.wait_event(
            |e| matches!(e, Event::SelectionNotify(n) if n.requestor == win && n.selection == selection),
        )?
        else {
            tracing::debug!("selection owner did not answer in time");
            return Ok(None);
        };
        if n.property == x11rb::NONE {
            return Ok(None);
        }
        let Some(first) = self.read_prop()? else {
            return Ok(None);
        };
        if first.type_ != self.atoms.INCR {
            if first.type_ == x11rb::NONE {
                return Ok(None);
            }
            return Ok(Some(Fetched {
                type_: first.type_,
                format: first.format,
                data: first.value,
            }));
        }
        self.fetch_incr()
    }

    /// Remaining part of an INCR transfer (the INCR marker was just deleted,
    /// which tells the owner to start sending).
    fn fetch_incr(&mut self) -> Result<Option<Fetched>> {
        let (win, prop) = (self.win, self.atoms.CLIPD_SEL);
        let mut out = Fetched {
            type_: x11rb::NONE,
            format: 8,
            data: Vec::new(),
        };
        loop {
            let got = self.wait_event(|e| {
                matches!(e, Event::PropertyNotify(p)
                    if p.window == win && p.atom == prop && p.state == Property::NEW_VALUE)
            })?;
            if got.is_none() {
                tracing::debug!("INCR transfer stalled");
                return Ok(None);
            }
            let Some(chunk) = self.read_prop()? else {
                return Ok(None);
            };
            if chunk.type_ == x11rb::NONE {
                continue; // stale notification, property already gone
            }
            if chunk.value.is_empty() {
                return Ok(Some(out));
            }
            out.type_ = chunk.type_;
            out.format = chunk.format;
            out.data.extend_from_slice(&chunk.value);
            if out.data.len() > MAX_BYTES {
                tracing::debug!("INCR payload over the size cap; abandoned");
                return Ok(None);
            }
        }
    }

    /// Offered targets as (atom, name) pairs.
    fn target_list(&mut self, time: u32) -> Result<Vec<(Atom, String)>> {
        let Some(f) = self.fetch(self.atoms.TARGETS, time)? else {
            return Ok(Vec::new());
        };
        if f.format != 32 {
            return Ok(Vec::new());
        }
        let atoms: Vec<Atom> = f
            .data
            .chunks_exact(4)
            .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .filter(|&a| a != x11rb::NONE)
            .collect();
        let unknown: Vec<Atom> = atoms
            .iter()
            .copied()
            .filter(|a| !self.names.contains_key(a))
            .collect();
        let cookies = unknown
            .iter()
            .map(|&a| self.conn.get_atom_name(a))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (a, cookie) in unknown.into_iter().zip(cookies) {
            if let Ok(reply) = cookie.reply() {
                self.names
                    .insert(a, String::from_utf8_lossy(&reply.name).into_owned());
            }
        }
        Ok(atoms
            .into_iter()
            .filter_map(|a| self.names.get(&a).map(|n| (a, n.clone())))
            .collect())
    }

    /// Read the clipboard content (priority: files, text(+html), image).
    pub fn read_content(&mut self, time: u32) -> Result<Option<RawContent>> {
        let targets = self.target_list(time)?;
        let names: Vec<String> = targets.iter().map(|(_, n)| n.clone()).collect();
        Ok(read_by_plan(&names, &mut |name| {
            let atom = targets.iter().find(|(_, n)| n == name)?.0;
            match self.fetch(atom, time) {
                Ok(f) => f.map(|f| f.data),
                Err(e) => {
                    tracing::debug!(target = name, error = %e, "fetching clipboard target failed");
                    None
                }
            }
        }))
    }

    /// Executable name of the app that owns the selection. Falls back to the
    /// active window's process when the owner window has no pid.
    pub fn source_app(&self, owner: Window) -> Option<String> {
        let pid = pid_of_window_tree(&self.conn, &self.atoms, self.root, owner).or_else(|| {
            let active = active_window(&self.conn, &self.atoms, self.root)?;
            pid_of_window_tree(&self.conn, &self.atoms, self.root, active)
        })?;
        process_name(pid)
    }
}
