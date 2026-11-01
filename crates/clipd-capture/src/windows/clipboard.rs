//! Reading from and writing to the Win32 clipboard.
//!
//! Clip bodies are never logged; only format names and sizes at debug level.

use std::ffi::c_void;

use anyhow::{Context, Result, bail};
use clipd_core::{RawClip, RawContent};
use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{
    EmptyClipboard, GetClipboardData, GetClipboardOwner, IsClipboardFormatAvailable,
    SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use super::{
    CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT, OpenClipboardGuard, formats, owner_window,
    process_name_of_window,
};
use crate::{WriteContent, cfhtml, dib, dropfiles};

/// Source app of the current clipboard content: the clipboard owner's
/// executable, or the foreground window's when the owner is unknown.
pub(super) fn source_app_now() -> Option<String> {
    unsafe {
        process_name_of_window(GetClipboardOwner())
            .or_else(|| process_name_of_window(GetForegroundWindow()))
    }
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Copy the bytes of a clipboard HGLOBAL.
unsafe fn global_bytes(handle: *mut c_void) -> Option<Vec<u8>> {
    if handle.is_null() {
        return None;
    }
    unsafe {
        let size = GlobalSize(handle);
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr as *const u8, size).to_vec();
        GlobalUnlock(handle);
        Some(bytes)
    }
}

/// Bytes of clipboard format `fmt`, if present. Clipboard must be open.
pub(super) fn format_bytes(fmt: u32) -> Option<Vec<u8>> {
    unsafe {
        if IsClipboardFormatAvailable(fmt) == 0 {
            return None;
        }
        global_bytes(GetClipboardData(fmt))
    }
}

fn format_dword(fmt: u32) -> Option<u32> {
    let b = format_bytes(fmt)?;
    Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
}

/// True if the content producer asked not to be recorded (password managers,
/// remote-desktop sync, Windows' own clipboard-history opt-out). Clipboard
/// must be open.
fn is_excluded() -> bool {
    let f = formats();
    unsafe {
        if IsClipboardFormatAvailable(f.exclude_from_monitor) != 0
            || IsClipboardFormatAvailable(f.viewer_ignore) != 0
        {
            return true;
        }
    }
    [f.can_include_in_history, f.can_upload_to_cloud]
        .into_iter()
        .any(|fmt| format_dword(fmt) == Some(0))
}

fn read_files() -> Option<Vec<String>> {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP) == 0 {
            return None;
        }
        let drop = GetClipboardData(CF_HDROP) as HDROP;
        if drop.is_null() {
            return None;
        }
        let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
        let mut files = Vec::with_capacity(count as usize);
        for i in 0..count {
            let len = DragQueryFileW(drop, i, std::ptr::null_mut(), 0) as usize;
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(drop, i, buf.as_mut_ptr(), buf.len() as u32) as usize;
            files.push(String::from_utf16_lossy(&buf[..got.min(len)]));
        }
        (!files.is_empty()).then_some(files)
    }
}

fn read_text() -> Option<String> {
    let bytes = format_bytes(CF_UNICODETEXT)?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    let text = String::from_utf16_lossy(&units);
    (!text.is_empty()).then_some(text)
}

fn read_html() -> Option<String> {
    cfhtml::parse(&format_bytes(formats().html)?)
}

fn read_image() -> Option<RawContent> {
    let f = formats();
    if let Some(png) = format_bytes(f.png) {
        match image::load_from_memory_with_format(&png, image::ImageFormat::Png) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (width, height) = rgba.dimensions();
                tracing::debug!(format = "PNG", width, height, "clipboard image");
                return Some(RawContent::Image {
                    width,
                    height,
                    rgba: rgba.into_raw(),
                });
            }
            Err(e) => tracing::debug!(error = %e, "PNG clipboard format undecodable"),
        }
    }
    for (fmt, name) in [(CF_DIBV5, "CF_DIBV5"), (CF_DIB, "CF_DIB")] {
        let Some(data) = format_bytes(fmt) else {
            continue;
        };
        match dib::dib_to_rgba(&data) {
            Ok((width, height, rgba)) => {
                tracing::debug!(format = name, width, height, "clipboard image");
                return Some(RawContent::Image {
                    width,
                    height,
                    rgba,
                });
            }
            Err(e) => tracing::debug!(format = name, error = %e, "DIB undecodable"),
        }
    }
    None
}

/// Read the clipboard content (priority: files, text(+html), image).
/// `Err` only when the clipboard could not be opened. The clipboard is
/// closed again before this returns.
pub(super) fn read_content(owner: HWND) -> Result<Option<RawContent>> {
    let _open = OpenClipboardGuard::open(owner)?;
    if is_excluded() {
        tracing::debug!("clipboard change excluded by producer opt-out format");
        return Ok(None);
    }
    if let Some(files) = read_files() {
        tracing::debug!(format = "CF_HDROP", count = files.len(), "clipboard files");
        return Ok(Some(RawContent::Files(files)));
    }
    if let Some(text) = read_text() {
        let html = read_html();
        tracing::debug!(
            format = "CF_UNICODETEXT",
            chars = text.chars().count(),
            html_bytes = html.as_ref().map(String::len),
            "clipboard text"
        );
        return Ok(Some(RawContent::Text { text, html }));
    }
    Ok(read_image())
}

/// See [`crate::read_current`].
pub fn read_current() -> Result<Option<RawClip>> {
    let source_app = source_app_now();
    let content = read_content(std::ptr::null_mut())?;
    Ok(content.map(|content| RawClip {
        content,
        source_app,
        captured_at: now_ms(),
    }))
}

// ---- writing -------------------------------------------------------------

/// Hand `data` to the clipboard as format `fmt`. Clipboard must be open and
/// emptied. On success the system owns the memory.
pub(super) unsafe fn set_data(fmt: u32, data: &[u8]) -> Result<()> {
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1));
        if handle.is_null() {
            bail!("GlobalAlloc({} bytes) failed", data.len());
        }
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            GlobalFree(handle);
            bail!("GlobalLock failed");
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
        GlobalUnlock(handle);
        if SetClipboardData(fmt, handle).is_null() {
            let err = GetLastError();
            GlobalFree(handle);
            bail!("SetClipboardData(format {fmt}) failed (error {err})");
        }
        Ok(())
    }
}

fn utf16_nul_bytes(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// See [`crate::write_clip`].
pub fn write_clip(content: WriteContent<'_>) -> Result<()> {
    let f = formats();
    // Build every payload before touching the clipboard.
    let mut items: Vec<(u32, Vec<u8>)> = Vec::new();
    match content {
        WriteContent::Text { text, html } => {
            items.push((CF_UNICODETEXT, utf16_nul_bytes(text)));
            if let Some(html) = html {
                let mut data = cfhtml::build(html);
                data.push(0);
                items.push((f.html, data));
            }
        }
        WriteContent::ImagePng(path) => {
            let png = std::fs::read(path).with_context(|| format!("reading {path}"))?;
            let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                .with_context(|| format!("decoding PNG {path}"))?
                .to_rgba8();
            let (w, h) = img.dimensions();
            items.push((f.png, png));
            items.push((CF_DIB, dib::rgba_to_dib32(w, h, img.as_raw())?));
        }
        WriteContent::Files(files) => {
            if files.is_empty() {
                bail!("no files to put on the clipboard");
            }
            items.push((CF_HDROP, dropfiles::encode(files)));
        }
    }

    let owner = owner_window()?;
    let _open = OpenClipboardGuard::open(owner)?;
    unsafe {
        if EmptyClipboard() == 0 {
            bail!("EmptyClipboard failed (error {})", GetLastError());
        }
        for (fmt, data) in &items {
            tracing::debug!(
                format = *fmt,
                bytes = data.len(),
                "writing clipboard format"
            );
            set_data(*fmt, data)?;
        }
    }
    Ok(())
}
