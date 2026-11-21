//! Find the file the OS saved a screenshot as.
//!
//! On Windows (PrtScn, Win+Shift+S) and GNOME (PrtSc) a screenshot is put
//! on the clipboard *and* written to the screenshots folder. The clipboard
//! only carries pixels, so we look for a PNG in that folder written around
//! the same moment whose pixels are the same, and link the clip to it.
//! Pixel equality (not names or times alone) keeps a wrong file from ever
//! being linked. macOS isn't covered: its save and copy shortcuts are
//! separate, so a clipboard screenshot has no file.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How far a screenshot file's modified time may be from the capture.
const WINDOW: Duration = Duration::from_secs(10);
/// Only the newest few PNGs in the window are decoded.
const MAX_CANDIDATES: usize = 4;
/// The file can land after the clipboard changes; total wait about 6 s.
const RETRY_DELAYS_MS: [u64; 6] = [0, 250, 500, 1_000, 1_500, 3_000];

/// The folders screenshots are saved to on this OS (existing ones only).
pub fn screenshot_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    if let Some(d) = windows_known_folder() {
        dirs.push(d);
    }
    if let Some(p) = dirs::picture_dir() {
        let d = p.join("Screenshots");
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

/// Wait briefly for the screenshot file of an image captured at `captured`,
/// retrying while it may still be being written. Blocks; call it off the
/// capture thread.
pub fn wait_for_screenshot(
    width: u32,
    height: u32,
    rgba: &[u8],
    captured: SystemTime,
) -> Option<PathBuf> {
    let dirs = screenshot_dirs();
    if dirs.is_empty() {
        return None;
    }
    for delay in RETRY_DELAYS_MS {
        std::thread::sleep(Duration::from_millis(delay));
        if let Some(p) = find_screenshot(&dirs, width, height, rgba, captured) {
            return Some(p);
        }
    }
    None
}

/// One look through `dirs` for a PNG near `captured` with exactly these
/// pixels. Alpha is ignored: the saved file may be RGB while the clipboard
/// bitmap is RGBA.
pub fn find_screenshot(
    dirs: &[PathBuf],
    width: u32,
    height: u32,
    rgba: &[u8],
    captured: SystemTime,
) -> Option<PathBuf> {
    let mut candidates: Vec<(SystemTime, PathBuf)> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| is_png(&e.path()))
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            near(modified, captured).then(|| (modified, e.path()))
        })
        .collect();
    candidates.sort_by_key(|c| std::cmp::Reverse(c.0));
    candidates
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(_, p)| p)
        .find(|p| same_pixels(p, width, height, rgba))
}

fn is_png(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))
}

fn near(a: SystemTime, b: SystemTime) -> bool {
    let d = a.duration_since(b).or_else(|_| b.duration_since(a));
    d.is_ok_and(|d| d <= WINDOW)
}

fn same_pixels(path: &Path, width: u32, height: u32, rgba: &[u8]) -> bool {
    // Cheap header check before decoding the whole file.
    if image::image_dimensions(path).ok() != Some((width, height)) {
        return false;
    }
    let Ok(img) = image::open(path) else {
        return false;
    };
    let file = img.to_rgba8();
    file.as_raw().len() == rgba.len()
        && file
            .as_raw()
            .chunks_exact(4)
            .zip(rgba.chunks_exact(4))
            .all(|(a, b)| a[..3] == b[..3])
}

#[cfg(windows)]
fn windows_known_folder() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Screenshots, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };

    let mut raw: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: valid GUID pointer and out-pointer; the returned buffer is
    // freed with CoTaskMemFree as documented, even on failure.
    let hr = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_Screenshots,
            KF_FLAG_DEFAULT as u32,
            std::ptr::null_mut(),
            &mut raw,
        )
    };
    let path = (hr >= 0 && !raw.is_null()).then(|| {
        // SAFETY: on success `raw` is a NUL-terminated wide string.
        let len = (0..).take_while(|&i| unsafe { *raw.add(i) } != 0).count();
        let wide = unsafe { std::slice::from_raw_parts(raw, len) };
        PathBuf::from(std::ffi::OsString::from_wide(wide))
    });
    // SAFETY: `raw` came from SHGetKnownFolderPath (null is allowed).
    unsafe { CoTaskMemFree(raw as *const _) };
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let d = std::env::temp_dir().join(format!(
                "clipd-shot-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&d).unwrap();
            TempDir(d)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn pixels(w: u32, h: u32, seed: u8) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([(x as u8).wrapping_add(seed), y as u8, seed, 255])
        })
    }

    #[test]
    fn finds_the_file_with_the_same_pixels_only() {
        let dir = TempDir::new();
        let shot = pixels(32, 20, 3);
        // Same size, different pixels: must not match.
        pixels(32, 20, 9)
            .save(dir.0.join("Screenshot (1).png"))
            .unwrap();
        // Saved as RGB (no alpha), like many screenshot tools do.
        let rgb = RgbImage::from_fn(32, 20, |x, y| {
            let p = shot.get_pixel(x, y);
            Rgb([p[0], p[1], p[2]])
        });
        rgb.save(dir.0.join("Screenshot (2).png")).unwrap();

        let found = find_screenshot(
            std::slice::from_ref(&dir.0),
            32,
            20,
            shot.as_raw(),
            SystemTime::now(),
        );
        assert_eq!(found, Some(dir.0.join("Screenshot (2).png")));
    }

    #[test]
    fn ignores_files_far_from_the_capture_time() {
        let dir = TempDir::new();
        let shot = pixels(16, 16, 1);
        shot.save(dir.0.join("old.png")).unwrap();
        let later = SystemTime::now() + Duration::from_secs(120);
        let found = find_screenshot(std::slice::from_ref(&dir.0), 16, 16, shot.as_raw(), later);
        assert_eq!(found, None);
    }

    #[test]
    fn other_sizes_and_non_png_files_are_skipped() {
        let dir = TempDir::new();
        let shot = pixels(16, 16, 1);
        pixels(17, 16, 1).save(dir.0.join("wide.png")).unwrap();
        std::fs::write(dir.0.join("notes.txt"), b"hello").unwrap();
        let found = find_screenshot(
            std::slice::from_ref(&dir.0),
            16,
            16,
            shot.as_raw(),
            SystemTime::now(),
        );
        assert_eq!(found, None);
    }
}
