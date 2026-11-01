//! Polling clipboard watcher: `changeCount` ticks on the general
//! pasteboard every POLL_MS; a move means someone copied. A short settle
//! delay coalesces apps that write the clipboard in bursts.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result};
use clipd_core::RawClip;
use objc2_app_kit::NSPasteboard;

const POLL_MS: u64 = 250;
/// One changeCount move can be a burst of several writes (one per UTI);
/// wait for it to settle so we read the final state once.
const SETTLE_MS: u64 = 75;

pub struct Watcher {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    pub fn spawn<F>(on_clip: F) -> Result<Self>
    where
        F: Fn(RawClip) + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("clipd-clipboard-watcher".into())
            .spawn(move || run(Box::new(on_clip), stop2))
            .context("spawning clipboard watcher thread")?;
        Ok(Watcher {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(on_clip: Box<dyn Fn(RawClip) + Send>, stop: Arc<AtomicBool>) {
    let pb = NSPasteboard::generalPasteboard();
    let mut last = pb.changeCount();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(POLL_MS));
        let cc = pb.changeCount();
        if cc == last {
            continue;
        }
        last = cc;
        // Owner/foreground are most accurate right at the change.
        let source_app = super::paste::source_app_now();
        std::thread::sleep(Duration::from_millis(SETTLE_MS));
        match super::pasteboard::read_content() {
            Some(content) => {
                let clip = RawClip {
                    content,
                    source_app,
                    captured_at: super::pasteboard::now_ms(),
                };
                if catch_unwind(AssertUnwindSafe(|| on_clip(clip))).is_err() {
                    tracing::warn!("on_clip callback panicked; watcher continues");
                }
            }
            None => tracing::debug!("pasteboard change read as empty/excluded"),
        }
    }
}
