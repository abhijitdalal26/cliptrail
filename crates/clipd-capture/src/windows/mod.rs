//! Windows backend: Win32 clipboard listener, clipboard read/write, and
//! paste-back via SendInput.

mod clipboard;
mod paste;
mod watcher;

#[cfg(test)]
mod live_tests;

pub use clipboard::{read_current, write_clip};
pub use paste::{paste_into, remember_foreground};
pub use watcher::Watcher;

use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, OpenClipboard, RegisterClipboardFormatW,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowThreadProcessId,
    HWND_MESSAGE, MSG, RegisterClassW, TranslateMessage, WNDCLASSW,
};

/// Standard clipboard format ids (winuser.h).
pub(crate) const CF_DIB: u32 = 8;
pub(crate) const CF_UNICODETEXT: u32 = 13;
pub(crate) const CF_HDROP: u32 = 15;
pub(crate) const CF_DIBV5: u32 = 17;

const OPEN_TRIES: u32 = 10;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(20);

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Registered (non-standard) clipboard format ids.
pub(crate) struct Formats {
    pub html: u32,
    pub png: u32,
    pub exclude_from_monitor: u32,
    pub viewer_ignore: u32,
    pub can_include_in_history: u32,
    pub can_upload_to_cloud: u32,
}

pub(crate) fn formats() -> &'static Formats {
    static F: OnceLock<Formats> = OnceLock::new();
    F.get_or_init(|| {
        let reg = |name: &str| unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) };
        Formats {
            html: reg("HTML Format"),
            png: reg("PNG"),
            exclude_from_monitor: reg("ExcludeClipboardContentFromMonitorProcessing"),
            viewer_ignore: reg("Clipboard Viewer Ignore"),
            can_include_in_history: reg("CanIncludeInClipboardHistory"),
            can_upload_to_cloud: reg("CanUploadToCloudClipboard"),
        }
    })
}

/// RAII guard: the clipboard is open while this lives.
pub(crate) struct OpenClipboardGuard;

impl OpenClipboardGuard {
    /// OpenClipboard with retries (other apps hold it briefly).
    pub fn open(owner: HWND) -> Result<Self> {
        for attempt in 0..OPEN_TRIES {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(Self);
            }
            if attempt + 1 < OPEN_TRIES {
                std::thread::sleep(OPEN_RETRY_DELAY);
            }
        }
        bail!(
            "OpenClipboard failed after {OPEN_TRIES} tries (error {})",
            unsafe { GetLastError() }
        )
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

/// Executable file name ("chrome.exe") of the process owning `hwnd`.
pub(crate) fn process_name_of_window(hwnd: HWND) -> Option<String> {
    if hwnd.is_null() {
        return None;
    }
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        (!name.is_empty()).then(|| name.to_owned())
    }
}

const CLASS_NAME: &str = "ClipTrailMessageWindow";

/// Register the shared message-window class (idempotent) and create a
/// message-only window with it.
pub(crate) fn create_message_window() -> Result<HWND> {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(watcher::wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            bail!("RegisterClassW failed (error {})", GetLastError());
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            bail!("CreateWindowExW failed (error {})", GetLastError());
        }
        Ok(hwnd)
    }
}

/// Pump messages for the calling thread until WM_QUIT.
pub(crate) fn run_message_loop() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Long-lived hidden window used as the clipboard *owner* for
/// `write_clip`, so that `GetClipboardOwner` (and thus the "source app" the
/// watcher reports) is this process. It lives on its own pumping thread.
pub(crate) fn owner_window() -> Result<HWND> {
    static OWNER: OnceLock<std::result::Result<isize, String>> = OnceLock::new();
    let r = OWNER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("clipd-clipboard-owner".into())
            .spawn(move || match create_message_window() {
                Ok(hwnd) => {
                    let _ = tx.send(Ok(hwnd as isize));
                    run_message_loop();
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            });
        match spawned {
            Ok(_) => rx
                .recv()
                .unwrap_or_else(|_| Err("owner thread died".into())),
            Err(e) => Err(e.to_string()),
        }
    });
    match r {
        Ok(h) => Ok(*h as HWND),
        Err(e) => bail!("clipboard owner window: {e}"),
    }
}

/// Default handling for everything the shared wnd_proc doesn't care about.
pub(crate) unsafe fn default_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
