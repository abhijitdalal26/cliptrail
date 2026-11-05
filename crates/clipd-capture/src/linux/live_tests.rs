//! Tests that touch the real session clipboard. All `#[ignore]`d; run with
//! `cargo test -p clipd-capture -- --ignored --test-threads=1`.
//!
//! They run against whatever backend the environment selects:
//!
//! - X11: any X session, or headless under Xvfb (`xvfb-run -a ...`).
//! - Wayland: a compositor with a data-control protocol (sway headless
//!   works: `WLR_BACKENDS=headless sway -c /dev/null`, then
//!   `WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=... cargo test ...`).
//!
//! X11-only pieces (INCR via xclip, XTest paste-back) skip themselves under
//! Wayland. The user's clipboard is restored afterwards.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use clipd_core::{RawClip, RawContent};

use super::x11;
use crate::{WriteContent, read_current, spawn_watcher, write_clip};

/// True on Wayland (native or XWayland fallback): no paste-back, no xclip.
fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

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
                    let _ = write_clip(WriteContent::ImagePng(&std::fs::read(&path).unwrap()));
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
    let deadline = Instant::now() + Duration::from_secs(6);
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
    if wayland() {
        // The Wayland protocol exposes no owner info.
        assert_eq!(clip.source_app, None);
    } else {
        assert_eq!(
            clip.source_app.as_deref().map(str::to_lowercase),
            Some(this_exe_name().to_lowercase())
        );
    }
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
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());
}

#[test]
#[ignore = "touches the real clipboard"]
fn png_image_round_trips() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();

    let (w, h) = (3u32, 2u32);
    let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i * 11 + 5) as u8).collect();
    let path = std::env::temp_dir().join(format!("{}.png", unique("img")));
    image::save_buffer(&path, &rgba, w, h, image::ColorType::Rgba8).unwrap();
    write_clip(WriteContent::ImagePng(&std::fs::read(&path).unwrap())).unwrap();
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
    let _ = std::fs::remove_file(path);
}

#[test]
#[ignore = "touches the real clipboard"]
fn files_round_trip() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();
    let files = vec![
        format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")),
        "/tmp/a b.txt".to_string(),
    ];
    write_clip(WriteContent::Files(&files)).unwrap();
    let clip = wait_for(&rx, |c| matches!(c.content, RawContent::Files(_))).expect("no files clip");
    let RawContent::Files(got) = clip.content else {
        unreachable!()
    };
    assert_eq!(got, files);
}

/// Every exclusion target a password manager is known to set must suppress
/// capture (KeePassXC's x-kde hint plus the nspasteboard conventions).
#[test]
#[ignore = "touches the real clipboard"]
fn excluded_targets_are_skipped() {
    // Password-manager hint atoms are an X11 convention; x11::write needs an
    // X server. On a Wayland session with XWayland the compositor bridges the
    // write into the Wayland clipboard, which is what this exercises there.
    if std::env::var_os("DISPLAY").is_none() {
        return;
    }
    let _restore = Restore::capture();
    let (_w, rx) = watch();
    for hint in [
        "x-kde-passwordManagerHint",
        "application/x-nspasteboard-concealed-type",
        "org.nspasteboard.ConcealedType",
        "application/x-nspasteboard-transient-type",
        "org.nspasteboard.TransientType",
    ] {
        let secret = unique("secret");
        // Serve UTF8_STRING + the hint like a password manager does.
        x11::write(vec![
            ("UTF8_STRING", secret.as_bytes().to_vec()),
            (hint, b"1".to_vec()),
        ])
        .unwrap();
        assert!(
            wait_for(&rx, |c| is_text(c, &secret)).is_none(),
            "{hint}: excluded clip was reported"
        );
    }
    // A plain write right after must still be captured.
    let ok = unique("allowed");
    write_clip(WriteContent::Text {
        text: &ok,
        html: None,
    })
    .unwrap();
    assert!(wait_for(&rx, |c| is_text(c, &ok)).is_some());
}

/// INCR read: a payload bigger than a single property transfer, served by
/// xclip (which does INCR when the data exceeds the server's max request
/// size). X11 only — Wayland has no INCR.
#[test]
#[ignore = "touches the real clipboard"]
fn large_clip_incr_via_xclip() {
    if wayland() {
        eprintln!("skipped: xclip/INCR is X11-only");
        return;
    }
    if which_xclip().is_none() {
        eprintln!("skipped: xclip not installed");
        return;
    }
    let _restore = Restore::capture();
    let (_w, rx) = watch();

    let marker = unique("incr");
    // >256 KiB of repeated text ends in the marker.
    let mut big = marker.clone();
    big.push_str(&"x".repeat(300 * 1024));
    big.push_str(&marker);
    let file = std::env::temp_dir().join(format!("{}.txt", unique("big")));
    std::fs::write(&file, &big).unwrap();
    // xclip serves the selection until another owner takes it.
    let mut child = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-i"])
        .arg(&file)
        .spawn()
        .unwrap();
    let clip = wait_for(&rx, |c| match &c.content {
        RawContent::Text { text, .. } => text.len() > 300 * 1024 && text.ends_with(&marker),
        _ => false,
    })
    .expect("no large clip via INCR");
    let RawContent::Text { text, .. } = clip.content else {
        unreachable!()
    };
    assert_eq!(text, big);
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&file);
}

fn which_xclip() -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("xclip"))
            .find(|p| p.is_file())
    })
}

/// A write from a different client must be captured. Uses `wl-copy` on
/// Wayland and `xclip` on X11 — the same tools users' terminals run.
#[test]
#[ignore = "touches the real clipboard"]
fn external_tool_copy_is_captured() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();
    let marker = unique("external");
    let file = std::env::temp_dir().join(format!("{}.txt", unique("ext")));
    std::fs::write(&file, &marker).unwrap();
    let mut child = if wayland() {
        std::process::Command::new("wl-copy")
            .arg("--")
            .arg(&marker)
            .spawn()
            .expect("wl-copy not available")
    } else {
        std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-i"])
            .arg(&file)
            .spawn()
            .expect("xclip not available")
    };
    let got = wait_for(&rx, |c| is_text(c, &marker));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&file);
    assert!(got.is_some(), "external clipboard write was not captured");
}

#[test]
#[ignore = "touches the real clipboard"]
fn watcher_stops_cleanly_and_can_restart() {
    for _ in 0..3 {
        let w = spawn_watcher(|_| {}).unwrap();
        w.stop();
    }
}

/// A minimal X11 window in another role: receives Ctrl+V (or Ctrl+Shift+V
/// when `need_shift`, a terminal), reads the CLIPBOARD selection like a
/// real app and reports the pasted text. Returns (window id, receiver).
mod target {
    use std::sync::mpsc;

    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt, CreateWindowAux, EventMask, KeyButMask, KeyPressEvent, PropMode,
        WindowClass,
    };
    use x11rb::wrapper::ConnectionExt as _;

    use crate::linux::x11::{Atoms, connect};

    const XK_V: u32 = 0x0076;

    /// `need_shift`: the fake app only honors Ctrl+Shift+V (a terminal).
    pub fn spawn(name: &str, class: &str, need_shift: bool) -> (u32, mpsc::Receiver<String>) {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (text_tx, text_rx) = mpsc::channel::<String>();
        let wm_class = format!("{name}\0{class}\0");
        std::thread::spawn(move || {
            let (conn, screen) = connect().unwrap();
            let atoms = Atoms::new(&conn).unwrap().reply().unwrap();
            let scr = &conn.setup().roots[screen];
            let win = conn.generate_id().unwrap();
            conn.create_window(
                scr.root_depth,
                win,
                scr.root,
                10,
                10,
                100,
                100,
                0,
                WindowClass::INPUT_OUTPUT,
                scr.root_visual,
                &CreateWindowAux::new().event_mask(EventMask::KEY_PRESS),
            )
            .unwrap();
            conn.change_property8(
                PropMode::REPLACE,
                win,
                atoms.WM_CLASS,
                AtomEnum::STRING,
                wm_class.as_bytes(),
            )
            .unwrap();
            conn.map_window(win).unwrap();
            let min = conn.setup().min_keycode;
            let max = conn.setup().max_keycode;
            let map = conn
                .get_keyboard_mapping(min, max - min + 1)
                .unwrap()
                .reply()
                .unwrap();
            let per = usize::from(map.keysyms_per_keycode);
            let v = map
                .keysyms
                .chunks(per)
                .position(|row| row.contains(&XK_V))
                .map(|i| (i + usize::from(min)) as u8)
                .expect("no 'v' keycode");
            ready_tx.send(win).unwrap();
            // On a paste keypress we request the selection; the answer lands
            // as a SelectionNotify in the same loop, like a real app's.
            loop {
                match conn.wait_for_event().unwrap() {
                    Event::KeyPress(k) => {
                        if on_v(&k, v, need_shift) {
                            conn.convert_selection(
                                win,
                                atoms.CLIPBOARD,
                                atoms.UTF8_STRING,
                                atoms.CLIPD_SEL,
                                k.time,
                            )
                            .unwrap();
                            conn.flush().unwrap();
                        }
                    }
                    Event::SelectionNotify(n)
                        if n.requestor == win && n.property != x11rb::NONE =>
                    {
                        let reply = conn
                            .get_property(true, win, atoms.CLIPD_SEL, AtomEnum::ANY, 0, u32::MAX)
                            .unwrap()
                            .reply()
                            .unwrap();
                        let _ = text_tx.send(String::from_utf8_lossy(&reply.value).into_owned());
                    }
                    _ => {}
                }
            }
        });
        (ready_rx.recv().unwrap(), text_rx)
    }

    fn on_v(k: &KeyPressEvent, v: u8, need_shift: bool) -> bool {
        k.detail == v
            && k.state.contains(KeyButMask::CONTROL)
            && k.state.contains(KeyButMask::SHIFT) == need_shift
    }
}

/// End to end: a real foreign-looking X11 window, focus moved elsewhere,
/// then `paste_into` must refocus it and send the paste keys.
#[test]
#[ignore = "touches the real clipboard and steals window focus"]
fn paste_into_text_window() {
    if wayland() {
        eprintln!("skipped: paste-back is X11-only");
        return;
    }
    let _restore = Restore::capture();
    let (win, rx) = target::spawn("cliptrail-target", "KWrite", false);
    std::thread::sleep(Duration::from_millis(400));

    let marker = unique("pasted");
    write_clip(WriteContent::Text {
        text: &marker,
        html: None,
    })
    .unwrap();
    crate::paste_into(crate::ForeignWindow(win as isize)).unwrap();
    let got = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("no paste received by the target window");
    assert_eq!(got.trim_end_matches('\0'), marker);
}

/// Same, but the target wears a terminal WM_CLASS: the backend must send
/// Ctrl+Shift+V, and our fake terminal ignores plain Ctrl+V.
#[test]
#[ignore = "touches the real clipboard and steals window focus"]
fn paste_into_terminal_sends_ctrl_shift_v() {
    if wayland() {
        eprintln!("skipped: paste-back is X11-only");
        return;
    }
    let _restore = Restore::capture();
    let (win, rx) = target::spawn("konsole", "konsole", true);
    std::thread::sleep(Duration::from_millis(400));

    let marker = unique("pasted-term");
    write_clip(WriteContent::Text {
        text: &marker,
        html: None,
    })
    .unwrap();
    crate::paste_into(crate::ForeignWindow(win as isize)).unwrap();
    let got = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("terminal target received no Ctrl+Shift+V");
    assert_eq!(got.trim_end_matches('\0'), marker);
}
