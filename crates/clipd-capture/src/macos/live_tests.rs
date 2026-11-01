//! Tests that touch the real pasteboard. All `#[ignore]`d; run with
//! `cargo test -p clipd-capture -- --ignored --test-threads=1`.
//! The previous clipboard content (text / files / image) is restored.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use clipd_core::{RawClip, RawContent};
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeString};
use objc2_foundation::{NSArray, NSString};

use super::pasteboard::{ItemData, write_item};
use crate::{WriteContent, read_current, spawn_watcher, write_clip};

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
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(c) if pred(&c) => return Some(c),
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
    None
}

#[test]
#[ignore]
fn text_write_watch_read() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();
    let text = unique("text");
    write_clip(WriteContent::Text {
        text: &text,
        html: None,
    })
    .unwrap();
    let clip = wait_for(
        &rx,
        |c| matches!(&c.content, RawContent::Text { text: t, .. } if *t == text),
    )
    .expect("watcher didn't see the text write");
    assert!(matches!(clip.content, RawContent::Text { .. }));
    let current = read_current().unwrap().unwrap();
    assert!(matches!(current.content, RawContent::Text { text: ref t, .. } if *t == text));
}

#[test]
#[ignore]
fn html_flavor_comes_along() {
    let _restore = Restore::capture();
    let text = unique("html");
    write_clip(WriteContent::Text {
        text: &text,
        html: Some("<b>hi</b>"),
    })
    .unwrap();
    let current = read_current().unwrap().unwrap();
    match current.content {
        RawContent::Text { text: t, html } => {
            assert_eq!(t, text);
            assert_eq!(html.as_deref(), Some("<b>hi</b>"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
#[ignore]
fn image_round_trip() {
    let _restore = Restore::capture();
    let (_w, rx) = watch();
    let path = std::env::temp_dir().join("cliptrail-test.png");
    let rgba = [255_u8, 0, 0, 255].repeat(16);
    image::save_buffer(&path, &rgba, 4, 4, image::ColorType::Rgba8).unwrap();
    write_clip(WriteContent::ImagePng(path.to_str().unwrap())).unwrap();
    let clip = wait_for(&rx, |c| matches!(c.content, RawContent::Image { .. }))
        .expect("watcher didn't see the image write");
    let RawContent::Image { width, height, .. } = clip.content else {
        panic!("not an image");
    };
    assert_eq!((width, height), (4, 4));
    let _ = std::fs::remove_file(path);
}

#[test]
#[ignore]
fn files_round_trip() {
    let _restore = Restore::capture();
    let f = std::env::temp_dir().join(unique("file"));
    std::fs::write(&f, b"x").unwrap();
    write_clip(WriteContent::Files(&[f.to_string_lossy().into_owned()])).unwrap();
    let current = read_current().unwrap().unwrap();
    match current.content {
        RawContent::Files(files) => assert!(files.iter().any(|p| p.ends_with("cliptrail-test"))),
        other => panic!("unexpected {other:?}"),
    }
    let _ = std::fs::remove_file(f);
}

/// A ConcealedType marker on the change must skip the whole copy.
#[test]
#[ignore]
fn concealed_type_is_skipped() {
    let _restore = Restore::capture();
    let text = unique("concealed");
    // Same trick 1Password uses: secret marker type alongside the payload.
    let marker = NSString::from_str("org.nspasteboard.ConcealedType");
    write_item(&[
        (
            unsafe { &NSPasteboardTypeString },
            ItemData::Text(text.clone()),
        ),
        (&marker, ItemData::Text(String::new())),
    ]);
    assert!(read_current().unwrap().is_none());
    // A separate write of just the marker changes changeCount; the watcher
    // must not emit a clip for it. (Full watcher check is racy across the
    // restore write, so we only assert the read side here.)
}

#[test]
#[ignore]
fn transient_type_is_skipped() {
    let _restore = Restore::capture();
    let marker = NSString::from_str("org.nspasteboard.TransientType");
    write_item(&[
        (
            unsafe { &NSPasteboardTypeString },
            ItemData::Text(unique("transient")),
        ),
        (&marker, ItemData::Text(String::new())),
    ]);
    assert!(read_current().unwrap().is_none());
}

/// Unused import guard: keep NSPasteboard referenced so feature combos
/// still compile even if tests are trimmed.
#[allow(dead_code)]
fn _touch(_p: &NSPasteboard, _i: &NSPasteboardItem, _a: &NSArray<NSString>) {}
