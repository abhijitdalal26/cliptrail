//! The Windows "HTML Format" clipboard flavor (CF_HTML).
//!
//! Layout: an ASCII header with byte offsets (`StartHTML`, `EndHTML`,
//! `StartFragment`, `EndFragment`) followed by UTF-8 HTML. Pure functions, no
//! OS calls, so they are testable (and reusable by other backends).

const PREFIX: &str = "<html><body>\r\n<!--StartFragment-->";
const SUFFIX: &str = "<!--EndFragment-->\r\n</body></html>";

fn header(start_html: usize, end_html: usize, start_frag: usize, end_frag: usize) -> String {
    format!(
        "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\n\
         StartFragment:{start_frag:010}\r\nEndFragment:{end_frag:010}\r\n"
    )
}

/// Wrap an HTML fragment into a CF_HTML payload with correct byte offsets.
/// The result is not NUL-terminated; callers adding it to the clipboard
/// append a trailing NUL.
pub fn build(fragment: &str) -> Vec<u8> {
    let header_len = header(0, 0, 0, 0).len();
    let start_frag = header_len + PREFIX.len();
    let end_frag = start_frag + fragment.len();
    let end_html = end_frag + SUFFIX.len();
    let mut out = header(header_len, end_html, start_frag, end_frag).into_bytes();
    out.extend_from_slice(PREFIX.as_bytes());
    out.extend_from_slice(fragment.as_bytes());
    out.extend_from_slice(SUFFIX.as_bytes());
    out
}

/// Value of `key:` in the header (first 1 KiB), if it is a plain
/// non-negative integer.
fn header_value(data: &[u8], key: &str) -> Option<usize> {
    let head = &data[..data.len().min(1024)];
    let key = format!("{key}:");
    let key = key.as_bytes();
    let mut i = 0;
    while i + key.len() < head.len() {
        let at_line_start = i == 0 || head[i - 1] == b'\n';
        if at_line_start && head[i..].starts_with(key) {
            let rest = &head[i + key.len()..];
            let end = rest
                .iter()
                .position(|b| !b.is_ascii_digit())
                .unwrap_or(rest.len());
            if end == 0 {
                return None;
            }
            return std::str::from_utf8(&rest[..end]).ok()?.parse().ok();
        }
        i += 1;
    }
    None
}

/// Extract the HTML from a CF_HTML payload: the fragment between
/// `StartFragment`/`EndFragment` if the header is sane, else the
/// `StartHTML..EndHTML` span, else the body after the header (or the whole
/// payload when there is no header). Returns None if the result is empty.
/// Input is treated as UTF-8 (lossy).
pub fn parse(data: &[u8]) -> Option<String> {
    // Windows allocations are often NUL padded.
    let end = data.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
    let data = &data[..end];
    let has_header = data.starts_with(b"Version:");
    let span = |s: &str, e: &str| {
        let (s, e) = (header_value(data, s)?, header_value(data, e)?);
        (s <= e && e <= data.len()).then_some(s..e)
    };
    let bytes = if has_header {
        match span("StartFragment", "EndFragment")
            .filter(|r| !r.is_empty())
            .or_else(|| span("StartHTML", "EndHTML"))
        {
            Some(r) => &data[r],
            None => {
                // Header present but unusable: skip the header lines.
                let body = data.iter().position(|&b| b == b'<').unwrap_or(0);
                &data[body..]
            }
        }
    } else {
        data
    };
    let s = String::from_utf8_lossy(bytes).into_owned();
    (!s.is_empty()).then_some(s)
}

/// The optional `SourceURL:` header line: the page the HTML was copied
/// from (browsers set it). Only http(s) addresses are returned.
pub fn source_url(data: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&data[..data.len().min(4096)]);
    let line = head
        .lines()
        .take_while(|l| !l.starts_with('<'))
        .find_map(|l| l.strip_prefix("SourceURL:"))?;
    web_url(line.trim())
}

/// `src` of the first `<img>` in an HTML fragment, if it is an http(s) URL.
pub fn img_src(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let tag = lower.find("<img")?;
    let end = lower[tag..].find('>').map_or(lower.len(), |e| tag + e);
    let attrs = &lower[tag..end];
    let mut from = 0;
    let at = loop {
        let i = from + attrs[from..].find("src")?;
        // A whole attribute name, not "srcset" or "data-src".
        let before = attrs[..i].chars().last();
        let after = attrs[i + 3..].trim_start();
        if before.is_some_and(char::is_whitespace) && after.starts_with('=') {
            break tag + i + 3 + (attrs[i + 3..].len() - after.len()) + 1;
        }
        from = i + 3;
    };
    let rest = html[at..end].trim_start();
    let value = match rest.chars().next()? {
        q @ ('"' | '\'') => rest[1..].split(q).next()?,
        _ => rest.split(|c: char| c.is_whitespace() || c == '>').next()?,
    };
    web_url(&value.replace("&amp;", "&"))
}

fn web_url(u: &str) -> Option<String> {
    let ok = u.len() <= 2048 && (u.starts_with("https://") || u.starts_with("http://"));
    ok.then(|| u.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_url_from_the_header() {
        let mut data = b"Version:0.9\r\nStartHTML:0000000105\r\nEndHTML:0000000200\r\n\
            StartFragment:0000000140\r\nEndFragment:0000000180\r\n\
            SourceURL:https://www.reddit.com/r/pics/comments/abc/\r\n<html>"
            .to_vec();
        assert_eq!(
            source_url(&data).as_deref(),
            Some("https://www.reddit.com/r/pics/comments/abc/")
        );
        data = build("<img src=\"https://i.redd.it/x.jpg\">");
        assert_eq!(source_url(&data), None, "no SourceURL line");
        assert_eq!(
            source_url(b"Version:0.9\r\nSourceURL:file:///C:/x.html\r\n"),
            None
        );
    }

    #[test]
    fn img_src_takes_the_first_image() {
        assert_eq!(
            img_src(r#"<img alt="cat" src="https://i.redd.it/abc.jpg?w=1&amp;s=2" srcset="x 2x">"#)
                .as_deref(),
            Some("https://i.redd.it/abc.jpg?w=1&s=2")
        );
        assert_eq!(
            img_src("<IMG data-src='nope' SRC='https://e.com/a.png'>").as_deref(),
            Some("https://e.com/a.png")
        );
        assert_eq!(
            img_src("<img src=https://e.com/b.gif>").as_deref(),
            Some("https://e.com/b.gif")
        );
        assert_eq!(img_src(r#"<img src="data:image/png;base64,AAAA">"#), None);
        assert_eq!(img_src("<p>no image</p>"), None);
    }

    #[test]
    fn round_trip_ascii_and_unicode() {
        for frag in ["<b>hi</b>", "<p>héllo — 日本語 😀</p>"] {
            assert_eq!(parse(&build(frag)).as_deref(), Some(frag));
        }
    }

    #[test]
    fn offsets_are_byte_exact() {
        let frag = "<i>日本語</i>";
        let data = build(frag);
        let sh = header_value(&data, "StartHTML").unwrap();
        let eh = header_value(&data, "EndHTML").unwrap();
        let sf = header_value(&data, "StartFragment").unwrap();
        let ef = header_value(&data, "EndFragment").unwrap();
        assert!(data[sh..].starts_with(b"<html>"));
        assert_eq!(eh, data.len());
        assert_eq!(&data[sf..ef], frag.as_bytes());
        assert!(data[..sf].ends_with(b"<!--StartFragment-->"));
        assert!(data[ef..].starts_with(b"<!--EndFragment-->"));
    }

    #[test]
    fn parses_payload_with_nul_padding() {
        let mut data = build("<span>x</span>");
        data.extend_from_slice(&[0, 0, 0]);
        assert_eq!(parse(&data).as_deref(), Some("<span>x</span>"));
    }

    #[test]
    fn falls_back_to_whole_html_without_header() {
        assert_eq!(parse(b"<b>raw</b>\0").as_deref(), Some("<b>raw</b>"));
    }

    #[test]
    fn falls_back_to_html_span_when_fragment_offsets_missing() {
        let body = "<b>abcdefg</b>";
        let hdr_len = 89;
        let head = format!(
            "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:-1\r\nEndFragment:-1\r\n",
            hdr_len,
            hdr_len + body.len()
        );
        assert_eq!(head.len(), hdr_len);
        let data = head + body;
        assert_eq!(parse(data.as_bytes()).as_deref(), Some(body));
    }

    #[test]
    fn unusable_header_falls_back_to_body() {
        let data = b"Version:0.9\r\nStartHTML:0000000040\r\nEndHTML:0000009999\r\nStartFragment:-1\r\nEndFragment:-1\r\n<b>abc</b>";
        assert_eq!(parse(data).as_deref(), Some("<b>abc</b>"));
    }

    #[test]
    fn garbage_offsets_do_not_panic() {
        let data = b"Version:0.9\r\nStartFragment:0000000900\r\nEndFragment:0000000100\r\nx";
        let _ = parse(data);
        assert_eq!(parse(b""), None);
        assert_eq!(parse(b"\0\0"), None);
    }
}
