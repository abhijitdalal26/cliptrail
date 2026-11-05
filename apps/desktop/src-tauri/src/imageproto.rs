//! `clipimg://` serves stored images and thumbnails to the webview.
//!
//! Image files are encrypted at rest (`clipd_core::imagefile`), so the
//! webview can't load them from disk; this protocol decrypts them in memory.
//! The frontend builds URLs with `convertFileSrc(path, "clipimg")`. Only
//! `<data dir>/images/*.png` and `<data dir>/thumbs/*.png` are served.

use std::path::{Path, PathBuf};

use tauri::http::{Response, StatusCode, header};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

use crate::state::AppState;

pub const SCHEME: &str = "clipimg";

/// Percent-decode a URL path segment (UTF-8).
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The image file a request may read, or None if it's outside the data dir's
/// `images/` / `thumbs/` folders.
fn allowed_path(data_dir: &Path, url_path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(percent_decode(url_path.trim_start_matches('/'))?);
    let name = path.file_name()?.to_str()?;
    let ok_name = name.len() <= 64
        && name.ends_with(".png")
        && name[..name.len() - 4]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok_name {
        return None;
    }
    let folder = path.parent()?;
    let kind = folder.file_name()?.to_str()?;
    if kind != "images" && kind != "thumbs" {
        return None;
    }
    let canon = |p: &Path| dunce_canonical(p);
    if canon(folder.parent()?)? != canon(data_dir)? {
        return None;
    }
    Some(data_dir.join(kind).join(name))
}

/// `canonicalize` without Windows' `\\?\` prefix, so paths compare equal.
fn dunce_canonical(p: &Path) -> Option<PathBuf> {
    let c = std::fs::canonicalize(p).ok()?;
    let s = c.to_string_lossy();
    Some(match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => c,
    })
}

fn respond(status: StatusCode, body: Vec<u8>, png: bool) -> Response<Vec<u8>> {
    let mut b = Response::builder().status(status);
    if png {
        b = b
            .header(header::CONTENT_TYPE, "image/png")
            // Ids are content-addressed per clip; a file never changes.
            .header(header::CACHE_CONTROL, "private, max-age=86400, immutable");
    }
    b.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    let url_path = request.uri().path().to_owned();
    std::thread::spawn(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return responder.respond(respond(StatusCode::SERVICE_UNAVAILABLE, Vec::new(), false));
        };
        let res = match allowed_path(&state.data_dir, &url_path) {
            None => respond(StatusCode::FORBIDDEN, Vec::new(), false),
            Some(path) => match clipd_core::read_image(&path) {
                Ok(png) => respond(StatusCode::OK, png, true),
                Err(e) => {
                    tracing::debug!(error = %e, "clipimg: image unavailable");
                    respond(StatusCode::NOT_FOUND, Vec::new(), false)
                }
            },
        };
        responder.respond(res);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_image_files_in_the_data_dir() {
        let dir = std::env::temp_dir().join(format!("clipimg-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("thumbs")).unwrap();
        std::fs::create_dir_all(dir.join("other")).unwrap();
        let enc = |p: &Path| {
            p.to_string_lossy()
                .bytes()
                .map(|b| match b {
                    b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect::<String>()
        };
        let good = dir.join("thumbs").join("01ABC.png");
        assert_eq!(
            allowed_path(&dir, &format!("/{}", enc(&good))),
            Some(dir.join("thumbs").join("01ABC.png"))
        );
        for bad in [
            dir.join("other").join("01ABC.png"),
            dir.join("thumbs").join("x.txt"),
            dir.join("thumbs").join("..").join("other").join("a.png"),
            std::env::temp_dir().join("thumbs").join("a.png"),
        ] {
            assert_eq!(
                allowed_path(&dir, &format!("/{}", enc(&bad))),
                None,
                "{bad:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
