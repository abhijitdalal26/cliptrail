//! Paste-back: remember the user's foreground window, later refocus it and
//! synthesize Ctrl+V.
//!
//! Focus strategy (escalating, each step given a short settle window):
//! 1. `SetForegroundWindow` alone. Windows allows it when our process is the
//!    foreground process or just received input, which is the normal case
//!    right after the picker was used.
//! 2. `AttachThreadInput` to the current foreground thread, then
//!    `BringWindowToTop` + `SetForegroundWindow`. Sharing the input queue of
//!    the foreground thread lifts the foreground lock.
//! 3. A synthetic Alt tap (counts as "last input event"), then
//!    `SetForegroundWindow` again. Last resort: Alt alone can activate a
//!    menu bar in the target.

use std::time::Duration;

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow,
    SW_RESTORE, SetForegroundWindow, ShowWindow,
};

use crate::ForeignWindow;

const POLL_STEP: Duration = Duration::from_millis(10);
/// Settle time per focus strategy: 5 polls x 10 ms (3 strategies ~ 150 ms).
const POLLS_PER_STRATEGY: u32 = 5;
const VK_V: u16 = 0x56;

pub fn remember_foreground() -> Option<crate::ForeignWindow> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 || pid == GetCurrentProcessId() {
            return None;
        }
        Some(ForeignWindow(hwnd as isize))
    }
}

fn key(vk: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<()> {
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent as usize != inputs.len() {
        bail!(
            "SendInput delivered {sent}/{} events (input blocked by a higher-integrity window?)",
            inputs.len()
        );
    }
    Ok(())
}

fn is_foreground(target: HWND) -> bool {
    unsafe { GetForegroundWindow() == target }
}

/// Poll until `target` is foreground; true if it got there in time.
fn settled(target: HWND) -> bool {
    for _ in 0..POLLS_PER_STRATEGY {
        if is_foreground(target) {
            return true;
        }
        std::thread::sleep(POLL_STEP);
    }
    is_foreground(target)
}

fn attach_and_focus(target: HWND) {
    unsafe {
        let fg = GetForegroundWindow();
        let fg_thread = if fg.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, std::ptr::null_mut())
        };
        let me = GetCurrentThreadId();
        let attached =
            fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, 1) != 0;
        BringWindowToTop(target);
        SetForegroundWindow(target);
        if attached {
            AttachThreadInput(me, fg_thread, 0);
        }
    }
}

fn alt_tap_and_focus(target: HWND) {
    let _ = send(&[key(VK_MENU, false), key(VK_MENU, true)]);
    unsafe {
        SetForegroundWindow(target);
    }
}

/// Key-up events for any modifier the user is still physically holding
/// (typically the picker hotkey), so they don't combine with our Ctrl+V.
fn release_held_modifiers() -> Vec<INPUT> {
    [
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ]
    .into_iter()
    .filter(|&vk| unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000 != 0)
    .map(|vk| key(vk, true))
    .collect()
}

pub fn paste_into(target: ForeignWindow) -> Result<()> {
    let hwnd = target.0 as HWND;
    unsafe {
        if IsWindow(hwnd) == 0 {
            bail!("target window no longer exists");
        }
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
    if !settled(hwnd) {
        tracing::debug!("SetForegroundWindow alone failed; trying AttachThreadInput");
        attach_and_focus(hwnd);
        if !settled(hwnd) {
            tracing::debug!("AttachThreadInput failed; trying Alt tap");
            alt_tap_and_focus(hwnd);
            if !settled(hwnd) {
                bail!("could not bring the target window to the foreground");
            }
        }
    }
    // Let the target's input queue pick up the activation.
    std::thread::sleep(Duration::from_millis(15));

    let mut inputs = release_held_modifiers();
    inputs.extend([
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ]);
    send(&inputs)
}
