//! Event-driven clipboard watcher: a message-only window registered with
//! `AddClipboardFormatListener`, debounced with a window timer.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::thread::JoinHandle;

use anyhow::{Context, Result, anyhow};
use clipd_core::RawClip;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, KillTimer, PostMessageW, PostQuitMessage,
    SetTimer, SetWindowLongPtrW, WM_CLIPBOARDUPDATE, WM_CLOSE, WM_DESTROY, WM_TIMER,
};

use super::clipboard::{now_ms, read_content, source_app_now};
use super::{create_message_window, default_proc, run_message_loop};

/// Bursts of formats from one copy are coalesced into a single read.
const DEBOUNCE_MS: u32 = 75;
/// If the clipboard is locked by someone else for a long time, try again.
const REOPEN_DELAY_MS: u32 = 250;
const MAX_REOPENS: u8 = 3;
const TIMER_ID: usize = 1;

struct State {
    on_clip: Box<dyn Fn(RawClip) + Send>,
    /// Source app captured at the latest WM_CLIPBOARDUPDATE.
    source_app: RefCell<Option<String>>,
    reopens: Cell<u8>,
}

pub struct Watcher {
    hwnd: isize,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    pub fn spawn<F>(on_clip: F) -> Result<Self>
    where
        F: Fn(RawClip) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<std::result::Result<isize, String>>();
        let thread = std::thread::Builder::new()
            .name("clipd-clipboard-watcher".into())
            .spawn(move || run(Box::new(on_clip), tx))
            .context("spawning clipboard watcher thread")?;
        match rx.recv() {
            Ok(Ok(hwnd)) => Ok(Watcher {
                hwnd,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(anyhow!(e))
            }
            Err(_) => {
                let _ = thread.join();
                Err(anyhow!("clipboard watcher thread exited during startup"))
            }
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // WM_CLOSE -> DestroyWindow -> WM_DESTROY -> PostQuitMessage.
            unsafe { PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0) };
            let _ = thread.join();
        }
    }
}

fn run(
    on_clip: Box<dyn Fn(RawClip) + Send>,
    ready: mpsc::Sender<std::result::Result<isize, String>>,
) {
    let state = Box::new(State {
        on_clip,
        source_app: RefCell::new(None),
        reopens: Cell::new(0),
    });
    let hwnd = match create_message_window() {
        Ok(h) => h,
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    };
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*state as *const State as isize);
        if AddClipboardFormatListener(hwnd) == 0 {
            let err = GetLastError();
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DestroyWindow(hwnd);
            let _ = ready.send(Err(format!(
                "AddClipboardFormatListener failed (error {err})"
            )));
            return;
        }
    }
    let _ = ready.send(Ok(hwnd as isize));
    run_message_loop();
    // Window is gone; stop wnd_proc from seeing a dangling pointer.
    drop(state);
}

pub(super) unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        match msg {
            WM_CLIPBOARDUPDATE if !state.is_null() => {
                let state = &*state;
                // Owner / foreground are most accurate right at the change.
                *state.source_app.borrow_mut() = source_app_now();
                state.reopens.set(0);
                SetTimer(hwnd, TIMER_ID, DEBOUNCE_MS, None);
                0
            }
            WM_TIMER if wparam == TIMER_ID && !state.is_null() => {
                KillTimer(hwnd, TIMER_ID);
                handle_update(hwnd, &*state);
                0
            }
            WM_CLOSE if !state.is_null() => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY if !state.is_null() => {
                RemoveClipboardFormatListener(hwnd);
                KillTimer(hwnd, TIMER_ID);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                PostQuitMessage(0);
                0
            }
            _ => default_proc(hwnd, msg, wparam, lparam),
        }
    }
}

fn handle_update(hwnd: HWND, state: &State) {
    // read_content closes the clipboard before returning, so the callback
    // below never runs while we hold the clipboard lock.
    match read_content(hwnd) {
        Ok(Some(content)) => {
            let clip = RawClip {
                content,
                source_app: state.source_app.borrow_mut().take(),
                captured_at: now_ms(),
            };
            if catch_unwind(AssertUnwindSafe(|| (state.on_clip)(clip))).is_err() {
                tracing::warn!("on_clip callback panicked; watcher continues");
            }
        }
        Ok(None) => {}
        Err(e) => {
            let tries = state.reopens.get();
            if tries < MAX_REOPENS {
                state.reopens.set(tries + 1);
                unsafe { SetTimer(hwnd, TIMER_ID, REOPEN_DELAY_MS, None) };
                tracing::debug!(error = %e, tries, "clipboard busy; will retry");
            } else {
                tracing::debug!(error = %e, "giving up on clipboard change");
            }
        }
    }
}
