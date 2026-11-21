//! `clipfile://` lets the webview preview a file the user copied (an image
//! or a video) straight from where it lives; nothing is copied.
//!
//! Narrow on purpose: a request is served only when the exact path is listed
//! by a clip in the history (a copied file, or a linked screenshot) and its
//! extension is a previewable image or video type. Videos are served in byte
//! ranges so the player streams them instead of reading the whole file.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use tauri::http::{Response, StatusCode, header};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

use crate::imageproto::percent_decode;
use crate::state::AppState;

pub const SCHEME: &str = "clipfile";

/// Largest image served whole; bigger ones show the file icon instead.
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
/// Largest slice of a video sent per request.
const MAX_CHUNK: u64 = 4 * 1024 * 1024;

/// MIME type of a previewable file, and whether it's a video.
pub fn preview_type(path: &Path) -> Option<(&'static str, bool)> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => ("image/png", false),
        "jpg" | "jpeg" => ("image/jpeg", false),
        "gif" => ("image/gif", false),
        "webp" => ("image/webp", false),
        "bmp" => ("image/bmp", false),
        "avif" => ("image/avif", false),
        "mp4" | "m4v" => ("video/mp4", true),
        "webm" => ("video/webm", true),
        "mov" => ("video/quicktime", true),
        _ => return None,
    })
}

/// `Range: bytes=a-b` / `bytes=a-` (one range) against a file of `len` bytes.
fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
    let spec = h.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), len.checked_sub(1)?)
        }
        (a, "") => (a.parse().ok()?, len.checked_sub(1)?),
        (a, b) => (
            a.parse().ok()?,
            b.parse::<u64>().ok()?.min(len.checked_sub(1)?),
        ),
    };
    (start <= end && end < len).then_some((start, end))
}

fn empty(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Vec::new())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn serve(path: &Path, range: Option<&str>) -> std::io::Result<Response<Vec<u8>>> {
    let Some((mime, video)) = preview_type(path) else {
        return Ok(empty(StatusCode::FORBIDDEN));
    };
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let base = |status: StatusCode| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, mime)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CACHE_CONTROL, "no-store")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
    };
    if !video {
        if len > MAX_IMAGE_BYTES {
            return Ok(empty(StatusCode::PAYLOAD_TOO_LARGE));
        }
        let mut body = Vec::with_capacity(len as usize);
        f.read_to_end(&mut body)?;
        return Ok(base(StatusCode::OK)
            .body(body)
            .unwrap_or_else(|_| Response::new(Vec::new())));
    }
    if len == 0 {
        return Ok(empty(StatusCode::NOT_FOUND));
    }
    // Players always send a Range; treat a missing one as "from the start".
    let Some((start, end)) = parse_range(range.unwrap_or("bytes=0-"), len) else {
        return Ok(base(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{len}"))
            .body(Vec::new())
            .unwrap_or_else(|_| Response::new(Vec::new())));
    };
    let end = end.min(start + MAX_CHUNK - 1);
    f.seek(SeekFrom::Start(start))?;
    let mut body = vec![0; (end - start + 1) as usize];
    f.read_exact(&mut body)?;
    Ok(base(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
        .body(body)
        .unwrap_or_else(|_| Response::new(Vec::new())))
}

pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    let url_path = request.uri().path().to_owned();
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    std::thread::spawn(move || {
        let Some(path) = percent_decode(url_path.trim_start_matches('/')) else {
            return responder.respond(empty(StatusCode::BAD_REQUEST));
        };
        let Ok(p) = app.state::<AppState>().pipeline() else {
            return responder.respond(empty(StatusCode::SERVICE_UNAVAILABLE));
        };
        if !p.store().has_file(&path).unwrap_or(false) {
            return responder.respond(empty(StatusCode::FORBIDDEN));
        }
        let res = serve(Path::new(&path), range.as_deref()).unwrap_or_else(|e| {
            tracing::debug!(error = %e, "clipfile: file unavailable");
            empty(StatusCode::NOT_FOUND)
        });
        responder.respond(res);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-", 10), Some((0, 9)));
        assert_eq!(parse_range("bytes=2-5", 10), Some((2, 5)));
        assert_eq!(parse_range("bytes=2-50", 10), Some((2, 9)));
        assert_eq!(parse_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(parse_range("bytes=10-", 10), None);
        assert_eq!(parse_range("bytes=5-2", 10), None);
        assert_eq!(parse_range("bytes=0-1,4-5", 10), None);
        assert_eq!(parse_range("items=0-1", 10), None);
    }

    #[test]
    fn only_media_types_are_previewable() {
        assert_eq!(
            preview_type(Path::new("a/B.JPG")),
            Some(("image/jpeg", false))
        );
        assert_eq!(
            preview_type(Path::new("clip.mp4")),
            Some(("video/mp4", true))
        );
        assert_eq!(preview_type(Path::new("notes.txt")), None);
        assert_eq!(preview_type(Path::new("page.svg")), None);
        assert_eq!(preview_type(Path::new("noext")), None);
    }

    #[test]
    fn video_is_served_in_capped_ranges() {
        let dir = std::env::temp_dir().join(format!("clipfile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("v.mp4");
        std::fs::write(&f, (0..=255u8).collect::<Vec<_>>()).unwrap();
        let r = serve(&f, Some("bytes=10-19")).unwrap();
        assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(r.body(), &(10..=19u8).collect::<Vec<_>>());
        assert_eq!(r.headers()[header::CONTENT_RANGE], "bytes 10-19/256");
        let txt = dir.join("t.txt");
        std::fs::write(&txt, b"secret").unwrap();
        assert_eq!(serve(&txt, None).unwrap().status(), StatusCode::FORBIDDEN);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
