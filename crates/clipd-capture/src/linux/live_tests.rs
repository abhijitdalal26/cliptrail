//! Live clipboard tests — need a real X server (or Wayland compositor).
//!
//! Opt-in: `cargo test -p clipd-capture -- --ignored` (CI runs them under
//! Xvfb). Each test writes via [`write_clip`] and expects the watcher to
//! deliver the same content back on [`spawn_watcher`]. `write_clip` serves
//! the selection from a background owner thread inside this process, so
//! writes stay live for the duration of the test.

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use clipd_core::{RawClip, RawContent};

use crate::{WriteContent, read_current, spawn_watcher};

const TIMEOUT: Duration = Duration::from_secs(10);

fn wait_for(
    rx: &std::sync::mpsc::Receiver<RawClip>,
    pred: impl Fn(&RawContent) -> bool,
) -> RawClip {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let now = Instant::now();
        if now >= deadline {
            panic!("timed out waiting for a clipboard change");
        }
        match rx.recv_timeout(deadline - now) {
            Ok(clip) if pred(&clip.content) => return clip,
            Ok(_) => continue, // unrelated change (e.g. an older write)
            Err(RecvTimeoutError::Timeout) => panic!("timed out waiting for a clipboard change"),
            Err(RecvTimeoutError::Disconnected) => panic!("watcher channel closed"),
        }
    }
}

#[test]
#[ignore]
fn text_write_watch_read() {
    let (tx, rx) = channel();
    let _watcher = spawn_watcher(move |clip| {
        let _ = tx.send(clip);
    })
    .expect("watcher");

    let text = format!("cliptrail-linux-live-{:?}", Instant::now());
    crate::write_clip(WriteContent::Text {
        text: &text,
        html: None,
    })
    .expect("write");

    let clip = wait_for(&rx, |c| matches!(c, RawContent::Text { .. }));
    match clip.content {
        RawContent::Text { text: got, .. } => assert_eq!(got, text),
        other => panic!("expected text, got {other:?}"),
    }

    match read_current().expect("read").map(|c| c.content) {
        Some(RawContent::Text { text: got, .. }) => assert_eq!(got, text),
        other => panic!("read_current: expected text, got {other:?}"),
    }
}

#[test]
#[ignore]
fn image_write_watch_read() {
    let (tx, rx) = channel();
    let _watcher = spawn_watcher(move |clip| {
        let _ = tx.send(clip);
    })
    .expect("watcher");

    // Tiny valid PNG (1x1 red pixel).
    let dir = std::env::temp_dir().join(format!("cliptrail-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("img.png");
    let red = image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 30, 30, 255]));
    red.save_with_format(&png, image::ImageFormat::Png).unwrap();
    let path = png.to_string_lossy().into_owned();

    crate::write_clip(WriteContent::ImagePng(&path)).expect("write");

    let clip = wait_for(&rx, |c| matches!(c, RawContent::Image { .. }));
    match clip.content {
        RawContent::Image { width, height, .. } => {
            assert_eq!((width, height), (4, 4));
        }
        other => panic!("expected image, got {other:?}"),
    }

    match read_current().expect("read").map(|c| c.content) {
        Some(RawContent::Image { .. }) => {}
        other => panic!("read_current: expected image, got {other:?}"),
    }
}

#[test]
#[ignore]
fn files_write_watch_read() {
    let (tx, rx) = channel();
    let _watcher = spawn_watcher(move |clip| {
        let _ = tx.send(clip);
    })
    .expect("watcher");

    let dir = std::env::temp_dir().join(format!("cliptrail-live-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("cliptrail-test-file-1.txt");
    let f2 = dir.join("cliptrail-test-file-2.txt");
    std::fs::write(&f1, "one").unwrap();
    std::fs::write(&f2, "two").unwrap();
    let paths = [
        f1.to_string_lossy().into_owned(),
        f2.to_string_lossy().into_owned(),
    ];

    crate::write_clip(WriteContent::Files(&paths)).expect("write");

    let clip = wait_for(&rx, |c| matches!(c, RawContent::Files(_)));
    match clip.content {
        RawContent::Files(got) => {
            assert!(got.iter().any(|p| p.contains("cliptrail-test-file-1")));
            assert!(got.iter().any(|p| p.contains("cliptrail-test-file-2")));
        }
        other => panic!("expected files, got {other:?}"),
    }
}
