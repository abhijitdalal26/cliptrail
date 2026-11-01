//! Clipboard target (MIME / X11 atom name) selection and text decoding for
//! the Linux backends (X11 and Wayland share the same logic). Pure and
//! OS-independent so it is unit-tested on every platform.
//!
//! Priority mirrors the Windows backend: files, then text (+ HTML flavor),
//! then image. Producers that mark the data as secret are excluded.

use crate::urilist;

/// Targets that mark clipboard content as secret / transient. Presence alone
/// is enough; we never fetch the (tiny) hint payload.
const EXCLUDE_TARGETS: &[&str] = &[
    // KeePassXC, KDE Klipper, wl-copy --sensitive (value is "secret").
    "x-kde-passwordManagerHint",
    // nspasteboard.org conventions, also set by some cross-platform apps.
    "application/x-nspasteboard-concealed-type",
    "org.nspasteboard.ConcealedType",
    "application/x-nspasteboard-transient-type",
    "org.nspasteboard.TransientType",
];

const URI_LIST: &str = "text/uri-list";
const GNOME_FILES: &str = "x-special/gnome-copied-files";
/// Plain-text targets, best first. `STRING` is Latin-1; the rest are UTF-8.
const TEXT_TARGETS: &[&str] = &[
    "UTF8_STRING",
    "text/plain;charset=utf-8",
    "text/plain",
    "STRING",
];
const HTML_TARGET: &str = "text/html";
const PNG_TARGET: &str = "image/png";

/// Which targets to fetch for one clipboard change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// The producer asked not to be recorded.
    pub excluded: bool,
    pub files: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub image: Option<String>,
}

impl Plan {
    /// Nothing we can read is on offer.
    pub fn is_empty(&self) -> bool {
        self.files.is_none() && self.text.is_none() && self.image.is_none()
    }
}

/// First entry of `wanted` (in priority order) that `offered` contains
/// (ASCII case-insensitive); returns the offered spelling.
fn pick(offered: &[String], wanted: &[&str]) -> Option<String> {
    wanted
        .iter()
        .find_map(|w| offered.iter().find(|o| o.eq_ignore_ascii_case(w)).cloned())
}

/// Decide what to read given the targets the owner offers.
pub fn plan(offered: &[String]) -> Plan {
    let excluded = offered
        .iter()
        .any(|o| EXCLUDE_TARGETS.iter().any(|e| o.eq_ignore_ascii_case(e)));
    if excluded {
        return Plan {
            excluded: true,
            ..Plan::default()
        };
    }
    Plan {
        excluded: false,
        files: pick(offered, &[URI_LIST, GNOME_FILES]),
        text: pick(offered, TEXT_TARGETS),
        html: pick(offered, &[HTML_TARGET]),
        image: pick(offered, &[PNG_TARGET]),
    }
}

/// Paths from a file-list target's payload (`target` as returned in `Plan`).
pub fn parse_files(target: &str, data: &[u8]) -> Vec<String> {
    if target.eq_ignore_ascii_case(GNOME_FILES) {
        urilist::parse_gnome_copied_files(data).1
    } else {
        urilist::parse_uri_list(data)
    }
}

/// Decode a plain-text payload. `STRING` is ISO-8859-1; everything else is
/// treated as UTF-8 (lossy). Trailing NULs are dropped; empty -> None.
pub fn decode_text(target: &str, data: &[u8]) -> Option<String> {
    let data = trim_nuls(data);
    let text = if target.eq_ignore_ascii_case("STRING") {
        data.iter().map(|&b| b as char).collect()
    } else {
        String::from_utf8_lossy(data).into_owned()
    };
    (!text.is_empty()).then_some(text)
}

/// Decode an HTML payload: UTF-8, or UTF-16 (BOM, or the NUL-interleaved
/// pattern some toolkits send without one). A leading BOM is removed.
pub fn decode_html(data: &[u8]) -> Option<String> {
    let html = if let Some(rest) = data.strip_prefix(&[0xFF, 0xFE]) {
        utf16(rest, u16::from_le_bytes)
    } else if let Some(rest) = data.strip_prefix(&[0xFE, 0xFF]) {
        utf16(rest, u16::from_be_bytes)
    } else if data.strip_prefix(&[0xEF, 0xBB, 0xBF]).is_some() {
        String::from_utf8_lossy(&data[3..]).into_owned()
    } else if looks_utf16le(data) {
        utf16(data, u16::from_le_bytes)
    } else {
        String::from_utf8_lossy(data).into_owned()
    };
    let html = html.trim_end_matches('\0').to_string();
    (!html.trim().is_empty()).then_some(html)
}

fn trim_nuls(data: &[u8]) -> &[u8] {
    let end = data.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    &data[..end]
}

fn utf16(data: &[u8], conv: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = data.chunks_exact(2).map(|c| conv([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

/// ASCII-heavy UTF-16LE without BOM: every odd byte of the first bytes is 0.
fn looks_utf16le(data: &[u8]) -> bool {
    let probe = &data[..data.len().min(32) & !1];
    probe.len() >= 4 && probe.chunks_exact(2).all(|c| c[1] == 0 && c[0] != 0)
}

/// Targets (name, bytes) to serve when we own the selection with text.
pub fn text_offers(text: &str, html: Option<&str>) -> Vec<(&'static str, Vec<u8>)> {
    let utf8 = text.as_bytes().to_vec();
    let mut out = vec![
        ("UTF8_STRING", utf8.clone()),
        ("text/plain;charset=utf-8", utf8.clone()),
        ("text/plain", utf8),
    ];
    if text.chars().all(|c| (c as u32) < 0x100) {
        out.push(("STRING", text.chars().map(|c| c as u32 as u8).collect()));
    }
    if let Some(html) = html {
        out.push((HTML_TARGET, html.as_bytes().to_vec()));
    }
    out
}

/// Targets to serve for a PNG image.
pub fn image_offers(png: Vec<u8>) -> Vec<(&'static str, Vec<u8>)> {
    vec![(PNG_TARGET, png)]
}

/// Targets to serve for a file list; None if no path is usable (absolute).
pub fn file_offers(paths: &[String]) -> Option<Vec<(&'static str, Vec<u8>)>> {
    let list = urilist::encode_uri_list(paths);
    if list.is_empty() {
        return None;
    }
    Some(vec![
        (URI_LIST, list),
        (GNOME_FILES, urilist::encode_gnome_copied_files(paths)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn text_priority_prefers_utf8() {
        let p = plan(&v(&["STRING", "text/plain", "UTF8_STRING", "TARGETS"]));
        assert_eq!(p.text.as_deref(), Some("UTF8_STRING"));
        let p = plan(&v(&["STRING", "text/plain"]));
        assert_eq!(p.text.as_deref(), Some("text/plain"));
        let p = plan(&v(&["STRING"]));
        assert_eq!(p.text.as_deref(), Some("STRING"));
    }

    #[test]
    fn charset_target_matches_case_insensitively() {
        let p = plan(&v(&["text/plain;charset=UTF-8", "text/html"]));
        assert_eq!(p.text.as_deref(), Some("text/plain;charset=UTF-8"));
        assert_eq!(p.html.as_deref(), Some("text/html"));
        assert!(!p.is_empty());
    }

    #[test]
    fn files_and_image_are_found() {
        let p = plan(&v(&[
            "x-special/gnome-copied-files",
            "text/uri-list",
            "UTF8_STRING",
            "image/png",
        ]));
        // uri-list wins over gnome-copied-files
        assert_eq!(p.files.as_deref(), Some("text/uri-list"));
        assert_eq!(p.text.as_deref(), Some("UTF8_STRING"));
        assert_eq!(p.image.as_deref(), Some("image/png"));
        let p = plan(&v(&["x-special/gnome-copied-files"]));
        assert_eq!(p.files.as_deref(), Some("x-special/gnome-copied-files"));
    }

    #[test]
    fn password_manager_hints_exclude() {
        let p = plan(&v(&["UTF8_STRING", "x-kde-passwordManagerHint"]));
        assert!(p.excluded);
        assert!(p.text.is_none());
        let p = plan(&v(&[
            "text/plain",
            "application/x-nspasteboard-concealed-type",
        ]));
        assert!(p.excluded);
        assert!(!plan(&v(&["text/plain"])).excluded);
    }

    #[test]
    fn nothing_readable_is_empty() {
        assert!(plan(&v(&["TARGETS", "TIMESTAMP", "image/jpeg"])).is_empty());
        assert!(plan(&[]).is_empty());
        // html alone is not enough (no plain-text flavor)
        assert!(plan(&v(&["text/html"])).is_empty());
    }

    #[test]
    fn file_payload_parsing_by_target() {
        let uri = b"file:///a%20b\r\nfile:///c\r\n";
        assert_eq!(parse_files("text/uri-list", uri), vec!["/a b", "/c"]);
        let gnome = b"copy\nfile:///a%20b\nfile:///c";
        assert_eq!(
            parse_files("x-special/gnome-copied-files", gnome),
            vec!["/a b", "/c"]
        );
        assert!(parse_files("text/uri-list", b"https://x.y").is_empty());
    }

    #[test]
    fn text_decoding() {
        assert_eq!(
            decode_text("UTF8_STRING", "héllo\0".as_bytes()).as_deref(),
            Some("héllo")
        );
        // STRING is Latin-1: 0xE9 is e-acute
        assert_eq!(decode_text("STRING", &[b'h', 0xE9]).as_deref(), Some("hé"));
        assert_eq!(decode_text("text/plain", b""), None);
        assert_eq!(decode_text("text/plain", b"\0\0"), None);
    }

    #[test]
    fn html_decoding() {
        assert_eq!(decode_html(b"<b>hi</b>").as_deref(), Some("<b>hi</b>"));
        assert_eq!(
            decode_html(b"\xEF\xBB\xBF<b>\xC3\xA9</b>").as_deref(),
            Some("<b>é</b>")
        );
        let mut le = vec![0xFF, 0xFE];
        le.extend("<p>é</p>".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_html(&le).as_deref(), Some("<p>é</p>"));
        let mut be = vec![0xFE, 0xFF];
        be.extend("<p>x</p>".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode_html(&be).as_deref(), Some("<p>x</p>"));
        // BOM-less UTF-16LE
        let bomless: Vec<u8> = "<i>a</i>"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode_html(&bomless).as_deref(), Some("<i>a</i>"));
        assert_eq!(decode_html(b"  \n"), None);
        assert_eq!(decode_html(b""), None);
    }

    #[test]
    fn text_offers_cover_common_targets() {
        let o = text_offers("héllo", Some("<b>x</b>"));
        let names: Vec<_> = o.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            [
                "UTF8_STRING",
                "text/plain;charset=utf-8",
                "text/plain",
                "STRING",
                "text/html"
            ]
        );
        // STRING is Latin-1 encoded
        assert_eq!(o[3].1, vec![b'h', 0xE9, b'l', b'l', b'o']);
        // non-Latin-1 text has no STRING flavor
        let o = text_offers("日本", None);
        assert!(o.iter().all(|(n, _)| *n != "STRING" && *n != "text/html"));
    }

    #[test]
    fn file_offers_need_an_absolute_path() {
        let o = file_offers(&v(&["/tmp/a b"])).unwrap();
        assert_eq!(o[0].0, "text/uri-list");
        assert_eq!(o[1].0, "x-special/gnome-copied-files");
        assert!(file_offers(&v(&[r"C:\only\windows"])).is_none());
        assert!(file_offers(&[]).is_none());
    }
}
