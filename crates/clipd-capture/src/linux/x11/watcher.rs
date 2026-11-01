//! Event-driven X11 clipboard watcher: XFixes `SelectionNotify` on the
//! CLIPBOARD selection, debounced like the Windows backend.
//!
//! Shutdown: the watcher thread blocks in `wait_for_event`, so `Drop` wakes
//! it with a client message sent from a second connection.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clipd_core::RawClip;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt as _, EventMask, Window};
use x11rb::rust_connection::RustConnection;

use super::clip::X11Clip;
use super::connect;
use crate::linux::common::now_ms;

/// Bursts of events from one copy are coalesced into a single read.
const DEBOUNCE: Duration = Duration::from_millis(75);
const IDLE_STEP: Duration = Duration::from_millis(5);

pub(in crate::linux) struct Watcher {
    /// Second connection, only used to wake the watcher thread on drop.
    waker: RustConnection,
    win: Window,
    stop_atom: u32,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    pub fn spawn<F>(on_clip: F) -> Result<Self>
    where
        F: Fn(RawClip) + Send + 'static,
    {
        let (waker, _) = connect()?;
        let stop_atom = waker
            .intern_atom(false, b"CLIPD_STOP")?
            .reply()
            .context("interning CLIPD_STOP")?
            .atom;

        let (tx, rx) = mpsc::channel::<std::result::Result<Window, String>>();
        let thread = std::thread::Builder::new()
            .name("clipd-clipboard-watcher".into())
            .spawn(move || run(on_clip, tx))
            .context("spawning clipboard watcher thread")?;
        match rx.recv() {
            Ok(Ok(win)) => Ok(Watcher {
                waker,
                win,
                stop_atom,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(anyhow!(e))
            }
            Err(_) => {
                let _ = thread.join();
                bail!("clipboard watcher thread exited during startup")
            }
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        let msg = ClientMessageEvent::new(32, self.win, self.stop_atom, [0u32; 5]);
        let sent = self
            .waker
            .send_event(false, self.win, EventMask::NO_EVENT, msg)
            .and_then(|_| self.waker.flush());
        match sent {
            Ok(()) => {
                let _ = thread.join();
            }
            // Cannot wake it; do not hang the caller. The thread ends when
            // the process does or the X connection drops.
            Err(e) => tracing::warn!(error = %e, "could not stop clipboard watcher thread"),
        }
    }
}

struct Pending {
    deadline: Instant,
    /// App name of the selection owner, resolved at the change itself (helper
    /// windows are often destroyed right after serving the data).
    source_app: Option<String>,
}

fn setup(clip: &X11Clip) -> Result<()> {
    clip.conn
        .xfixes_query_version(5, 0)?
        .reply()
        .context("XFixes extension unavailable")?;
    clip.conn.xfixes_select_selection_input(
        clip.win,
        clip.atoms.CLIPBOARD,
        SelectionEventMask::SET_SELECTION_OWNER
            | SelectionEventMask::SELECTION_WINDOW_DESTROY
            | SelectionEventMask::SELECTION_CLIENT_CLOSE,
    )?;
    clip.conn.flush()?;
    Ok(())
}

fn run<F>(on_clip: F, ready: mpsc::Sender<std::result::Result<Window, String>>)
where
    F: Fn(RawClip) + Send + 'static,
{
    let mut clip = match X11Clip::connect().and_then(|c| setup(&c).map(|()| c)) {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(format!("{e:#}")));
            return;
        }
    };
    let _ = ready.send(Ok(clip.win));
    if let Err(e) = event_loop(&mut clip, &on_clip) {
        tracing::warn!(error = %e, "X11 clipboard watcher stopped");
    }
}

fn event_loop(clip: &mut X11Clip, on_clip: &dyn Fn(RawClip)) -> Result<()> {
    let mut pending: Option<Pending> = None;
    loop {
        let event = if let Some(ev) = clip.take_deferred() {
            Some(ev)
        } else if pending.is_some() {
            clip.conn.poll_for_event()?
        } else {
            Some(clip.conn.wait_for_event()?)
        };
        let Some(event) = event else {
            // Only reachable while a change is pending: wait out the debounce.
            if pending
                .as_ref()
                .is_some_and(|p| Instant::now() >= p.deadline)
            {
                let p = pending.take().expect("checked above");
                handle_change(clip, on_clip, p);
            } else {
                std::thread::sleep(IDLE_STEP);
            }
            continue;
        };
        match event {
            Event::XfixesSelectionNotify(e) if e.selection == clip.atoms.CLIPBOARD => {
                if e.owner == x11rb::NONE {
                    pending = None; // selection cleared, nothing to read
                } else {
                    pending = Some(Pending {
                        deadline: Instant::now() + DEBOUNCE,
                        source_app: clip.source_app(e.owner),
                    });
                }
            }
            Event::ClientMessage(m) if m.type_ == clip.atoms.CLIPD_STOP && m.window == clip.win => {
                return Ok(());
            }
            _ => {}
        }
    }
}

fn handle_change(clip: &mut X11Clip, on_clip: &dyn Fn(RawClip), pending: Pending) {
    match clip.read_content(x11rb::CURRENT_TIME) {
        Ok(Some(content)) => {
            let raw = RawClip {
                content,
                source_app: pending.source_app,
                captured_at: now_ms(),
            };
            if catch_unwind(AssertUnwindSafe(|| on_clip(raw))).is_err() {
                tracing::warn!("on_clip callback panicked; watcher continues");
            }
        }
        Ok(None) => {}
        Err(e) => tracing::debug!(error = %e, "reading clipboard failed"),
    }
}
