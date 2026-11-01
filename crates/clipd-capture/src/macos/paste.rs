//! Paste-back: remember the frontmost app, re-activate it, post Cmd+V.
//!
//! `ForeignWindow` wraps the target's pid. Posting synthetic events needs
//! Accessibility permission — checked up front with `AXIsProcessTrusted`
//! so the UI gets a clear, actionable error instead of a silent no-op.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use objc2_core_graphics::{CGEvent, CGEventFlags};
use objc2_foundation::{NSDate, NSRunLoop};

use crate::ForeignWindow;

/// kVK_ANSI_V
const VK_V: u16 = 0x09;
const ACTIVATE_TIMEOUT: Duration = Duration::from_millis(750);
const POLL_STEP: Duration = Duration::from_millis(10);

/// The message the UI surfaces when paste-back is denied.
const ACCESSIBILITY_HINT: &str =
    "allow ClipTrail in System Settings → Privacy & Security → Accessibility";

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// HIServices: true when the process may post synthetic input events.
    fn AXIsProcessTrusted() -> bool;
}

pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Name of the frontmost app — recorded at copy time. The executable's
/// file name ("Safari") is what the rest of the app expects; bundle id is
/// the fallback.
pub(super) fn source_app_now() -> Option<String> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    app.executableURL()
        .and_then(|u| u.lastPathComponent())
        .or_else(|| app.bundleIdentifier())
        .or_else(|| app.localizedName())
        .map(|s| s.to_string())
}

pub fn remember_foreground() -> Option<ForeignWindow> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    // Never remember ourselves — the picker is about to take key status.
    if app.processIdentifier() == std::process::id() as i32 {
        return None;
    }
    Some(ForeignWindow(app.processIdentifier() as isize))
}

fn pump_runloop_for(d: Duration) {
    // Activation is delivered on the app's run loop; give it a moment.
    let limit = NSDate::dateWithTimeIntervalSinceNow(d.as_secs_f64());
    let _ = NSRunLoop::currentRunLoop()
        .runMode_beforeDate(unsafe { &objc2_foundation::NSDefaultRunLoopMode }, &limit);
}

pub fn paste_into(target: ForeignWindow) -> Result<()> {
    if !trusted() {
        bail!("Accessibility permission needed for paste-back: {ACCESSIBILITY_HINT}");
    }
    let pid = target.0 as i32;
    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
        bail!("the target app is no longer running")
    };

    let options = NSApplicationActivationOptions::ActivateAllWindows;
    if !app.activateWithOptions(options) {
        bail!("the target app refused to come to the front");
    }
    let deadline = Instant::now() + ACTIVATE_TIMEOUT;
    while !app.isActive() && Instant::now() < deadline {
        pump_runloop_for(POLL_STEP);
    }
    if !app.isActive() {
        bail!("the target app did not come to the front in time");
    }

    // Cmd+V straight at the app (tap location Session also works, but
    // post_to_pid can't land on the wrong app if focus shifted mid-wait).
    let Some(down) = CGEvent::new_keyboard_event(None, VK_V, true) else {
        bail!("couldn't create the Cmd+V key event")
    };
    CGEvent::set_flags(Some(&down), CGEventFlags::MaskCommand);
    let Some(up) = CGEvent::new_keyboard_event(None, VK_V, false) else {
        bail!("couldn't create the Cmd+V key event")
    };
    CGEvent::set_flags(Some(&up), CGEventFlags::MaskCommand);
    CGEvent::post_to_pid(pid, Some(&down));
    CGEvent::post_to_pid(pid, Some(&up));
    Ok(())
}
