//! Owning the CLIPBOARD selection for `write_clip`.
//!
//! X11 has no clipboard storage: the owner must answer `SelectionRequest`
//! events for as long as the data should stay pasteable. Each write spawns a
//! thread with its own connection and window that takes the selection and
//! serves it until another client takes over (`SelectionClear`), so the
//! content survives for as long as the process runs.
//!
//! Limitation: payloads are sent in a single property write, so anything
//! above the server's maximum request size (about 16 MiB with BIG-REQUESTS)
//! is rejected up front; outgoing INCR is not implemented.

use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt, CreateWindowAux, EventMask, PropMode, SELECTION_NOTIFY_EVENT,
    SelectionNotifyEvent, SelectionRequestEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use super::{Atoms, connect};

const READY_TIMEOUT: Duration = Duration::from_secs(3);
/// Headroom for the request header when checking payload sizes.
const REQUEST_OVERHEAD: usize = 128;

type Offers = Vec<(&'static str, Vec<u8>)>;

/// See [`crate::write_clip`]. Returns once we own the selection.
pub(in crate::linux) fn write(offers: Offers) -> Result<()> {
    let (tx, rx) = mpsc::channel::<std::result::Result<(), String>>();
    std::thread::Builder::new()
        .name("clipd-clipboard-owner".into())
        .spawn(move || {
            if let Err(e) = run(&offers, &tx) {
                // Before readiness this reports the failure to the caller;
                // afterwards nobody listens and it is only a debug note.
                tracing::debug!(error = %e, "clipboard owner thread ended");
                let _ = tx.send(Err(format!("{e:#}")));
            }
        })
        .context("spawning clipboard owner thread")?;
    match rx.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => bail!("X11 clipboard write failed: {e}"),
        Err(_) => bail!("X11 clipboard write timed out"),
    }
}

fn run(offers: &Offers, ready: &mpsc::Sender<std::result::Result<(), String>>) -> Result<()> {
    let (conn, screen_num) = connect()?;
    let atoms = Atoms::new(&conn)?.reply().context("interning atoms")?;

    let max = conn
        .maximum_request_bytes()
        .saturating_sub(REQUEST_OVERHEAD);
    if let Some((name, data)) = offers.iter().find(|(_, d)| d.len() > max) {
        bail!(
            "{name} payload is {} bytes; the X server accepts at most {max} per request \
             (outgoing INCR is not supported)",
            data.len()
        );
    }

    let cookies = offers
        .iter()
        .map(|(name, _)| conn.intern_atom(false, name.as_bytes()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let target_atoms = cookies
        .into_iter()
        .map(|c| c.reply().map(|r| r.atom))
        .collect::<std::result::Result<Vec<Atom>, _>>()?;

    let screen = &conn.setup().roots[screen_num];
    let win = conn.generate_id()?;
    conn.create_window(
        screen.root_depth,
        win,
        screen.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new(),
    )?;
    // So the watcher reports this process as the source of our own writes
    // (same as the Windows backend).
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms._NET_WM_PID,
        AtomEnum::CARDINAL,
        &[std::process::id()],
    )?;
    conn.set_selection_owner(win, atoms.CLIPBOARD, x11rb::CURRENT_TIME)?;
    let owner = conn.get_selection_owner(atoms.CLIPBOARD)?.reply()?.owner;
    if owner != win {
        bail!("could not acquire the CLIPBOARD selection");
    }
    let _ = ready.send(Ok(()));

    loop {
        match conn.wait_for_event()? {
            Event::SelectionRequest(req) => {
                let ctx = Serving {
                    conn: &conn,
                    atoms: &atoms,
                    offers,
                    target_atoms: &target_atoms,
                };
                ctx.serve(&req)?;
            }
            Event::SelectionClear(c) if c.selection == atoms.CLIPBOARD => return Ok(()),
            _ => {}
        }
    }
}

struct Serving<'a> {
    conn: &'a RustConnection,
    atoms: &'a Atoms,
    offers: &'a Offers,
    target_atoms: &'a [Atom],
}

impl Serving<'_> {
    fn serve(&self, req: &SelectionRequestEvent) -> Result<()> {
        // Pre-ICCCM clients leave the property unset: use the target.
        let property = if req.property == x11rb::NONE {
            req.target
        } else {
            req.property
        };
        let served = if req.selection != self.atoms.CLIPBOARD {
            false
        } else if req.target == self.atoms.TARGETS {
            let mut list = vec![self.atoms.TARGETS];
            list.extend_from_slice(self.target_atoms);
            self.conn.change_property32(
                PropMode::REPLACE,
                req.requestor,
                property,
                AtomEnum::ATOM,
                &list,
            )?;
            true
        } else if let Some(i) = self.target_atoms.iter().position(|&a| a == req.target) {
            let name = self.offers[i].0;
            // Property type: STRING for the Latin-1 flavor, else the target
            // itself (UTF8_STRING, image/png, ...).
            let type_: Atom = if name == "STRING" {
                AtomEnum::STRING.into()
            } else {
                req.target
            };
            tracing::debug!(
                format = name,
                bytes = self.offers[i].1.len(),
                "serving clipboard target"
            );
            self.conn.change_property8(
                PropMode::REPLACE,
                req.requestor,
                property,
                type_,
                &self.offers[i].1,
            )?;
            true
        } else {
            false
        };
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: req.time,
            requestor: req.requestor,
            selection: req.selection,
            target: req.target,
            property: if served { property } else { x11rb::NONE },
        };
        self.conn
            .send_event(false, req.requestor, EventMask::NO_EVENT, notify)?;
        self.conn.flush()?;
        Ok(())
    }
}
