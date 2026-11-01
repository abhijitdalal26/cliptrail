//! Wayland clipboard through `wl-clipboard-rs` (wlr / ext data-control).
//!
//! Works on KDE Plasma, Sway, Hyprland and other wlroots compositors. GNOME
//! (Mutter) does not implement a data-control protocol, so `probe` fails
//! there and the caller falls back to XWayland.
//!
//! Wayland exposes no information about which app owns the selection, so
//! `source_app` is always None.

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use clipd_core::{RawClip, RawContent};
use wl_clipboard_rs::copy::{self, MimeSource, MimeType as CopyMime, Source};
use wl_clipboard_rs::paste::{self, MimeType as PasteMime, Seat};
use wl_clipboard_rs::watch::{CancelHandle, ClipboardEvent, ClipboardType, Watcher as WlWatcher};

use super::common::{MAX_BYTES, now_ms, read_by_plan};

/// A source that never closes its pipe must not hang the watcher forever.
const READ_TIMEOUT: Duration = Duration::from_secs(3);

/// Is a data-control protocol available (and is there a seat)?
pub(super) fn probe() -> Result<()> {
    WlWatcher::new(ClipboardType::Regular, Seat::Unspecified)
        .map(drop)
        .map_err(|e| anyhow!(e))
}

/// Read at most `MAX_BYTES` from `pipe`, giving up after `READ_TIMEOUT`.
fn read_limited<R: Read + Send + 'static>(mut pipe: R) -> Option<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("clipd-wl-read".into())
        .spawn(move || {
            let mut buf = Vec::new();
            let res = (&mut pipe)
                .take(MAX_BYTES as u64 + 1)
                .read_to_end(&mut buf)
                .map(|_| buf);
            let _ = tx.send(res);
        })
        .ok()?;
    match rx.recv_timeout(READ_TIMEOUT) {
        Ok(Ok(buf)) if buf.len() <= MAX_BYTES => Some(buf),
        Ok(Ok(_)) => {
            tracing::debug!("clipboard payload over the size cap; skipped");
            None
        }
        _ => None,
    }
}

pub(super) struct Watcher {
    cancel: CancelHandle,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    pub fn spawn<F>(on_clip: F) -> Result<Self>
    where
        F: Fn(RawClip) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<std::result::Result<CancelHandle, String>>();
        let thread = std::thread::Builder::new()
            .name("clipd-clipboard-watcher".into())
            .spawn(move || run(on_clip, tx))
            .context("spawning clipboard watcher thread")?;
        match rx.recv() {
            Ok(Ok(cancel)) => Ok(Watcher {
                cancel,
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
        if let Some(thread) = self.thread.take() {
            self.cancel.cancel();
            let _ = thread.join();
        }
    }
}

/// Mirror of the X11 watcher's debounce window: several copies inside it
/// collapse to the latest.
const DEBOUNCE: Duration = Duration::from_millis(75);

fn run<F>(on_clip: F, ready: mpsc::Sender<std::result::Result<CancelHandle, String>>)
where
    F: Fn(RawClip) + Send + 'static,
{
    // The compositor replays the current selection right after we bind the
    // data device. Snapshot it first and drop the first event whose content
    // is identical — anything else is a genuine copy and must be reported,
    // even if it lands microseconds after startup.
    let baseline = read_current().ok().flatten().map(|c| c.content);
    let mut pending_replay = baseline.is_some();
    let mut watcher = match WlWatcher::new(ClipboardType::Regular, Seat::Unspecified) {
        Ok(w) => w,
        Err(e) => {
            let _ = ready.send(Err(format!("Wayland clipboard watcher: {e}")));
            return;
        }
    };
    let _ = ready.send(Ok(watcher.cancel_handle()));

    // Pump data-control events on an inner thread; this thread debounces.
    let (tx, rx) = mpsc::channel::<RawContent>();
    std::thread::Builder::new()
        .name("clipd-wl-events".into())
        .spawn(move || {
            loop {
                match watcher.next_event() {
                    Ok(None) => break, // cancelled
                    Ok(Some(ClipboardEvent::Changed {
                        mime_types,
                        mut offer,
                        ..
                    })) => {
                        let content = read_by_plan(&mime_types, &mut |target| {
                            offer.receive(target).ok().and_then(read_limited)
                        });
                        if pending_replay && content.as_ref() == baseline.as_ref() {
                            pending_replay = false;
                            continue;
                        }
                        pending_replay = false;
                        if let Some(content) = content
                            && tx.send(content).is_err()
                        {
                            break;
                        }
                    }
                    Ok(Some(ClipboardEvent::Cleared { .. })) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "Wayland clipboard watcher stopped");
                        break;
                    }
                }
            }
        })
        .ok();

    // Debounce: report only the latest content after DEBOUNCE of quiet.
    // `recv` delivers queued items even after the sender is gone, so a dead
    // pump drains first and only then reports Disconnected.
    let mut latest: Option<RawContent> = None;
    loop {
        let got = match latest {
            None => rx.recv().ok(),
            Some(_) => rx.recv_timeout(DEBOUNCE).ok(),
        };
        match got {
            Some(content) => latest = Some(content),
            None => match latest.take() {
                Some(content) => {
                    let clip = RawClip {
                        content,
                        source_app: None,
                        captured_at: now_ms(),
                    };
                    if catch_unwind(AssertUnwindSafe(|| on_clip(clip))).is_err() {
                        tracing::warn!("on_clip callback panicked; watcher continues");
                    }
                }
                None => break, // channel closed and drained
            },
        }
    }
}

pub(super) fn read_current() -> Result<Option<RawClip>> {
    let types = match paste::get_mime_types(paste::ClipboardType::Regular, Seat::Unspecified) {
        Ok(t) => t,
        Err(paste::Error::ClipboardEmpty | paste::Error::NoSeats | paste::Error::NoMimeType) => {
            return Ok(None);
        }
        Err(e) => return Err(anyhow!(e).context("reading Wayland clipboard")),
    };
    let names: Vec<String> = types.into_iter().collect();
    let content = read_by_plan(&names, &mut |target| {
        let (pipe, _) = paste::get_contents(
            paste::ClipboardType::Regular,
            Seat::Unspecified,
            PasteMime::Specific(target),
        )
        .ok()?;
        read_limited(pipe)
    });
    Ok(content.map(|content| RawClip {
        content,
        source_app: None,
        captured_at: now_ms(),
    }))
}

/// Take over the clipboard and serve `offers` from a background thread for
/// as long as nobody else copies something.
pub(super) fn write(offers: Vec<(&'static str, Vec<u8>)>) -> Result<()> {
    let (tx, rx) = mpsc::channel::<std::result::Result<(), String>>();
    std::thread::Builder::new()
        .name("clipd-clipboard-owner".into())
        .spawn(move || {
            let sources = offers
                .into_iter()
                .map(|(name, bytes)| MimeSource {
                    source: Source::Bytes(bytes.into_boxed_slice()),
                    mime_type: CopyMime::Specific(name.to_string()),
                })
                .collect();
            let mut opts = copy::Options::new();
            // `prepare_copy_multi` requires foreground; we are on our own thread.
            opts.foreground(true).omit_additional_text_mime_types(true);
            match opts.prepare_copy_multi(sources) {
                Ok(prepared) => {
                    let _ = tx.send(Ok(()));
                    if let Err(e) = prepared.serve() {
                        tracing::debug!(error = %e, "Wayland clipboard source ended with error");
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
        })
        .context("spawning clipboard owner thread")?;
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => bail!("Wayland clipboard write failed: {e}"),
        Err(_) => bail!("Wayland clipboard write timed out"),
    }
}
