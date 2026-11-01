//! Tests that touch the real system clipboard. All `#[ignore]`d; run with
//! `cargo test -p clipd-capture -- --ignored --test-threads=1`.
//! The user's clipboard (text / files / image) is restored afterwards.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use clipd_core::{RawClip, RawContent};

use super::clipboard::{format_bytes, set_data};
use super::{CF_DIB, CF_UNICODETEXT, OpenClipboardGuard, formats, owner_window};
use crate::{WriteContent, read_current, spawn_watcher, write_clip};

/// Restores whatever was on the clipboard when dropped.
struct Restore(Option<RawContent>);

impl Restore {
    fn capture() -> Self {
        Restore(read_current().ok().flatten().map(|c| c.content))
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        match self.0.take() {
            Some(RawContent::Text { text, html }) => {
                let _ = write_clip(WriteContent::Text {
                    text: &text,
                    html: html.as_deref(),
                });
            }
            Some(RawContent::Files(files)) => {
                let _ = write_clip(WriteContent::Files(&files));
            }
            Some(RawContent::Image {
                width,
                height,
                rgba,
            }) => {
                let path = std::env::temp_dir().join("cliptrail-restore.png");
                if image::save_buffer(&path, &rgba, width, height, image::ColorType::Rgba8).is_ok()
                {
                    let _ = write_clip(WriteContent::ImagePng(path.to_str().unwrap()));
                    let _ = std::fs::remove_file(path);
                }
            }
            None => {}
        }
    }
}

fn this_exe_name() -> String {
    std::env::current_exe()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn unique(tag: &str) -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("cliptrail-test-{tag}-{n}")
}

fn watch() -> (crate::WatcherHandle, mpsc::Receiver<RawClip>) {
    let (tx, rx) = mpsc::channel();
    let w = spawn_watcher(move |c| {
        let _ = tx.send(c);
    })
    .unwrap();
    (w, rx)
}

/// Wait for a clip satisfying `pred` (other clips are ignored).
fn wait_for(rx: &mpsc::Receiver<RawClip>, pred: impl Fn(&RawClip) -> bool) -> Option<RawClip> {
    let deadline = Instant::now() + Duration::from_secs(4);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(c) if pred(&c) => return Some(c),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

fn is_text(c: &RawClip, want: &str) -> bool {
    matches!(&c.content, RawContent::Text { text, .. } if text == want)
}

#[test]
#[ignore = "touches the real clipboard"]
fn watcher_sees_written_text_with_source_app() {
    let _restore = Restore::capture();
    let (watcher, rx) = watch();

    let marker = unique("text");
    write_clip(WriteContent::Text {
        text: &marker,
        html: None,
    })
    .unwrap();
    let clip =
        wait_for(&rx, |c| is_text(c, &marker)).expect("watcher did not report the written text");
    assert!(clip.captured_at > 1_700_000_000_000);
    assert_eq!(
        clip.source_app.as_deref().map(str::to_lowercase),
        Some(this_exe_name().to_lowercase())
    );
    let RawContent::Text { html, .. } = clip.content else {
        unreachable!()
    };
    assert!(html.is_none());
    watcher.stop();
}

#[test]
#[ignore = "touches the real clipboard"]
fn text_with_html_round_trips() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();

    let marker = unique("html");
    let html = format!("<p>h\u{e9}llo <b>{marker}</b> \u{65e5}\u{672c}</p>");
    write_clip(WriteContent::Text {
        text: &marker,
        html: Some(&html),
    })
    .unwrap();
    let clip = wait_for(&rx, |c| is_text(c, &marker)).expect("no clip");
    let RawContent::Text { html: got, .. } = clip.content else {
        unreachable!()
    };
    assert_eq!(got.as_deref(), Some(html.as_str()));
}

#[test]
#[ignore = "touches the real clipboard"]
fn burst_of_writes_is_debounced_to_latest() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();
    let (a, b) = (unique("burst-a"), unique("burst-b"));
    write_clip(WriteContent::Text {
        text: &a,
        html: None,
    })
    .unwrap();
    write_clip(WriteContent::Text {
        text: &b,
        html: None,
    })
    .unwrap();
    let clip = wait_for(&rx, |c| matches!(&c.content, RawContent::Text { .. })).expect("no clip");
    assert!(
        is_text(&clip, &b),
        "expected only the latest write of the burst"
    );
    assert!(rx.recv_timeout(Duration::from_millis(400)).is_err());
}

#[test]
#[ignore = "touches the real clipboard"]
fn png_image_round_trips_with_alpha() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();

    let (w, h) = (3u32, 2u32);
    let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i * 11 + 5) as u8).collect();
    let path = std::env::temp_dir().join(format!("{}.png", unique("img")));
    image::save_buffer(&path, &rgba, w, h, image::ColorType::Rgba8).unwrap();
    write_clip(WriteContent::ImagePng(path.to_str().unwrap())).unwrap();
    let clip =
        wait_for(&rx, |c| matches!(c.content, RawContent::Image { .. })).expect("no image clip");
    let RawContent::Image {
        width,
        height,
        rgba: got,
    } = clip.content
    else {
        unreachable!()
    };
    assert_eq!((width, height), (w, h));
    assert_eq!(got, rgba);

    // The CF_DIB flavor must decode to the same colours too.
    {
        let _open = OpenClipboardGuard::open(std::ptr::null_mut()).unwrap();
        let dib = format_bytes(CF_DIB).expect("CF_DIB present");
        let (dw, dh, drgba) = crate::dib::dib_to_rgba(&dib).unwrap();
        assert_eq!((dw, dh), (w, h));
        for (a, b) in drgba.chunks(4).zip(rgba.chunks(4)) {
            assert_eq!(a[..3], b[..3]);
        }
    }
    let _ = std::fs::remove_file(path);
}

#[test]
#[ignore = "touches the real clipboard"]
fn files_round_trip() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();
    let files = vec![
        format!("{}\\Cargo.toml", env!("CARGO_MANIFEST_DIR")),
        r"C:\Windows\notepad.exe".to_string(),
    ];
    write_clip(WriteContent::Files(&files)).unwrap();
    let clip = wait_for(&rx, |c| matches!(c.content, RawContent::Files(_))).expect("no files clip");
    let RawContent::Files(got) = clip.content else {
        unreachable!()
    };
    assert_eq!(got, files);
}

#[test]
#[ignore = "touches the real clipboard"]
fn excluded_formats_are_skipped() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();
    let f = formats();

    let write_with = |text: &str, extra: Option<(u32, u32)>| {
        let _open = OpenClipboardGuard::open(owner_window().unwrap()).unwrap();
        unsafe {
            windows_sys::Win32::System::DataExchange::EmptyClipboard();
            let bytes: Vec<u8> = text
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            set_data(CF_UNICODETEXT, &bytes).unwrap();
            if let Some((fmt, val)) = extra {
                set_data(fmt, &val.to_le_bytes()).unwrap();
            }
        }
    };

    for (name, extra) in [
        ("exclude", (f.exclude_from_monitor, 1)),
        ("viewer-ignore", (f.viewer_ignore, 1)),
        ("history-0", (f.can_include_in_history, 0)),
        ("cloud-0", (f.can_upload_to_cloud, 0)),
    ] {
        let secret = unique(name);
        write_with(&secret, Some(extra));
        assert!(
            wait_for(&rx, |c| is_text(c, &secret)).is_none(),
            "{name}: excluded clip was reported"
        );
    }
    // DWORD 1 means "allowed": must be captured.
    let ok = unique("allowed");
    write_with(&ok, Some((f.can_include_in_history, 1)));
    assert!(wait_for(&rx, |c| is_text(c, &ok)).is_some());
}

#[test]
#[ignore = "touches the real clipboard"]
fn watcher_stops_cleanly_and_can_restart() {
    for _ in 0..3 {
        let w = spawn_watcher(|_| {}).unwrap();
        w.stop();
    }
}

/// End to end: a real WinForms text box in another process, focus stolen
/// away on purpose, then `paste_into` must refocus it and paste.
#[test]
#[ignore = "touches the real clipboard and steals window focus"]
fn paste_into_other_process_text_box() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow};

    let _restore = Restore::capture();
    let title = unique("target");
    let out = std::env::temp_dir().join(format!("{title}.txt"));
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         $f = New-Object Windows.Forms.Form; $f.Text = '{title}'; \
         $t = New-Object Windows.Forms.TextBox; $t.Multiline = $true; $t.Dock = 'Fill'; \
         $f.Controls.Add($t); $f.Add_Shown({{ $f.Activate(); $t.Focus() }}); \
         $tm = New-Object Windows.Forms.Timer; $tm.Interval = 9000; \
         $tm.Add_Tick({{ [IO.File]::WriteAllText('{}', $t.Text); $f.Close() }}); $tm.Start(); \
         [Windows.Forms.Application]::Run($f)",
        out.display()
    );
    let mut child = std::process::Command::new("powershell")
        .args(["-NoProfile", "-STA", "-Command", &script])
        .spawn()
        .unwrap();

    let wtitle = super::wide(&title);
    let mut hwnd = std::ptr::null_mut();
    for _ in 0..200 {
        hwnd = unsafe { FindWindowW(std::ptr::null(), wtitle.as_ptr()) };
        if !hwnd.is_null() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!hwnd.is_null(), "target window never appeared");
    std::thread::sleep(Duration::from_millis(800));

    // Push focus elsewhere (the taskbar) to make the refocus non-trivial.
    unsafe {
        let tray = FindWindowW(super::wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
        SetForegroundWindow(tray);
    }
    std::thread::sleep(Duration::from_millis(300));

    let marker = unique("pasted");
    write_clip(WriteContent::Text {
        text: &marker,
        html: None,
    })
    .unwrap();
    let t0 = Instant::now();
    crate::paste_into(crate::ForeignWindow(hwnd as isize)).unwrap();
    eprintln!("paste_into took {:?}", t0.elapsed());

    let status = child.wait().unwrap();
    assert!(status.success());
    let got = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    assert_eq!(got.trim(), marker);
}
