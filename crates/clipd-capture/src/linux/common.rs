//! Display-server independent parts of the Linux backend: turn "the targets
//! on offer + a way to fetch one" into a `RawContent`.

use anyhow::{Context, Result};
use clipd_core::RawContent;

use crate::targets;

/// Largest single clipboard payload we are willing to read (bytes).
pub(super) const MAX_BYTES: usize = 64 << 20;

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Read the clipboard content (priority: files, text(+html), image) given
/// the targets the owner offers. `fetch(target)` returns that target's bytes.
/// Clip bodies are never logged; only target names and sizes at debug level.
pub(super) fn read_by_plan(
    offered: &[String],
    mut fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Option<RawContent> {
    let plan = targets::plan(offered);
    if plan.excluded {
        tracing::debug!("clipboard change excluded by producer opt-out target");
        return None;
    }
    if let Some(target) = &plan.files
        && let Some(data) = fetch(target)
    {
        let files = targets::parse_files(target, &data);
        if !files.is_empty() {
            tracing::debug!(format = %target, count = files.len(), "clipboard files");
            return Some(RawContent::Files(files));
        }
    }
    if let Some(target) = &plan.text
        && let Some(data) = fetch(target)
        && let Some(text) = targets::decode_text(target, &data)
    {
        let html = plan
            .html
            .as_deref()
            .and_then(&mut fetch)
            .and_then(|d| targets::decode_html(&d));
        tracing::debug!(
            format = %target,
            chars = text.chars().count(),
            html_bytes = html.as_ref().map(String::len),
            "clipboard text"
        );
        return Some(RawContent::Text { text, html });
    }
    if let Some(target) = &plan.image
        && let Some(data) = fetch(target)
    {
        match decode_png(&data) {
            Ok((width, height, rgba)) => {
                tracing::debug!(format = %target, width, height, "clipboard image");
                return Some(RawContent::Image {
                    width,
                    height,
                    rgba,
                });
            }
            Err(e) => tracing::debug!(format = %target, error = %e, "image undecodable"),
        }
    }
    None
}

fn decode_png(png: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)?.to_rgba8();
    let (w, h) = img.dimensions();
    Ok((w, h, img.into_raw()))
}

/// Validate PNG bytes for `WriteContent::ImagePng`.
pub(super) fn checked_png(png: &[u8]) -> Result<Vec<u8>> {
    decode_png(png).context("decoding PNG")?;
    Ok(png.to_vec())
}

/// Payloads (target name, bytes) to publish for a `WriteContent`.
pub(super) fn offers_for(content: crate::WriteContent<'_>) -> Result<Vec<(&'static str, Vec<u8>)>> {
    use crate::WriteContent;
    Ok(match content {
        WriteContent::Text { text, html } => targets::text_offers(text, html),
        WriteContent::ImagePng(png) => targets::image_offers(checked_png(png)?),
        WriteContent::Files(files) => {
            targets::file_offers(files).context("no absolute file paths to put on the clipboard")?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn offered(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn run(names: &[&str], data: &[(&str, &[u8])]) -> Option<RawContent> {
        let map: HashMap<String, Vec<u8>> = data
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_vec()))
            .collect();
        read_by_plan(&offered(names), &mut |t| map.get(t).cloned())
    }

    #[test]
    fn files_win_over_text() {
        let got = run(
            &["text/uri-list", "UTF8_STRING"],
            &[
                ("text/uri-list", b"file:///tmp/a%20b\r\n"),
                ("UTF8_STRING", b"/tmp/a b"),
            ],
        );
        match got {
            Some(RawContent::Files(f)) => assert_eq!(f, vec!["/tmp/a b"]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn link_in_uri_list_falls_back_to_text() {
        let got = run(
            &["text/uri-list", "UTF8_STRING"],
            &[
                ("text/uri-list", b"https://example.com\r\n"),
                ("UTF8_STRING", b"https://example.com"),
            ],
        );
        assert!(matches!(got, Some(RawContent::Text { .. })));
    }

    #[test]
    fn text_with_html() {
        let got = run(
            &["UTF8_STRING", "text/html"],
            &[("UTF8_STRING", b"hi"), ("text/html", b"<b>hi</b>")],
        );
        match got {
            Some(RawContent::Text { text, html }) => {
                assert_eq!(text, "hi");
                assert_eq!(html.as_deref(), Some("<b>hi</b>"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn image_is_decoded() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(3, 2)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let got = run(&["image/png"], &[("image/png", &png)]);
        match got {
            Some(RawContent::Image {
                width,
                height,
                rgba,
            }) => {
                assert_eq!((width, height, rgba.len()), (3, 2, 24));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn excluded_and_unreadable_give_none() {
        assert!(
            run(
                &["UTF8_STRING", "x-kde-passwordManagerHint"],
                &[("UTF8_STRING", b"secret")]
            )
            .is_none()
        );
        assert!(run(&["UTF8_STRING"], &[]).is_none());
        assert!(run(&["image/png"], &[("image/png", b"not a png")]).is_none());
    }
}
