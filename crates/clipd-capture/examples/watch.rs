//! Prints metadata (never content) for each clipboard change.
//!
//! `cargo run -p clipd-capture --example watch [-- <seconds>]`
//! Runs until Ctrl+C, or for <seconds> if given.

use std::time::{Duration, Instant};

use clipd_core::RawContent;

fn main() -> anyhow::Result<()> {
    let seconds: Option<u64> = std::env::args().nth(1).and_then(|s| s.parse().ok());
    let started = Instant::now();
    let _watcher = clipd_capture::spawn_watcher(move |clip| {
        let what = match &clip.content {
            RawContent::Text { text, html } => format!(
                "text chars={} bytes={} html={}",
                text.chars().count(),
                text.len(),
                html.as_ref()
                    .map_or("no".to_string(), |h| format!("{} bytes", h.len())),
            ),
            RawContent::Image {
                width,
                height,
                rgba,
            } => format!("image {width}x{height} rgba_bytes={}", rgba.len()),
            RawContent::Files(files) => format!("files count={}", files.len()),
        };
        println!(
            "[+{:>6.1}s] {what} | source={} | captured_at={}",
            started.elapsed().as_secs_f32(),
            clip.source_app.as_deref().unwrap_or("?"),
            clip.captured_at,
        );
    })?;
    println!("watching the clipboard (metadata only); copy something...");
    match seconds {
        Some(s) => std::thread::sleep(Duration::from_secs(s)),
        None => loop {
            std::thread::sleep(Duration::from_secs(3600));
        },
    }
    Ok(())
}
