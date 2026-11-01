//! Freedesktop file-list clipboard formats (`text/uri-list` and
//! `x-special/gnome-copied-files`) plus percent-encoding. Pure and
//! OS-independent; used by the Linux backend.
//!
//! Only local `file://` URIs are understood; everything else (http links,
//! remote hosts) is dropped so a copied link is treated as text, not a file.

/// Percent-decode `%XX` escapes. Malformed escapes are kept literally.
pub fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Percent-encode a filesystem path for use in a `file://` URI. Unreserved
/// characters and `/` are kept; every other byte becomes `%XX`.
pub fn percent_encode_path(path: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(path.len());
    for &b in path.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Convert one `file:` URI to an absolute local path, or None if it is not a
/// local file URI.
pub fn file_uri_to_path(uri: &str) -> Option<String> {
    let uri = uri.trim();
    if !uri.get(..5)?.eq_ignore_ascii_case("file:") {
        return None;
    }
    let after = uri[5..].strip_prefix("//")?;
    // file:///path  (empty host)  or  file://localhost/path
    let path_part = if after.starts_with('/') {
        after
    } else {
        let (host, path) = after.split_at(after.find('/')?);
        if !host.eq_ignore_ascii_case("localhost") {
            return None;
        }
        path
    };
    // Query / fragment do not belong to a local path.
    let path_part = path_part.split(['?', '#']).next().unwrap_or(path_part);
    let decoded = String::from_utf8_lossy(&percent_decode(path_part)).into_owned();
    decoded.starts_with('/').then_some(decoded)
}

/// Parse a `text/uri-list` body (RFC 2483): one URI per line, `#` comments.
pub fn parse_uri_list(data: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(data)
        .split(['\r', '\n', '\0'])
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(file_uri_to_path)
        .collect()
}

/// What a `x-special/gnome-copied-files` payload asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    Copy,
    Cut,
}

/// Parse `x-special/gnome-copied-files`: first line is `copy` or `cut`,
/// followed by one URI per line. Tolerates a missing action line.
pub fn parse_gnome_copied_files(data: &[u8]) -> (FileAction, Vec<String>) {
    let text = String::from_utf8_lossy(data);
    let mut action = FileAction::Copy;
    let mut paths = Vec::new();
    let lines = text
        .split(['\r', '\n', '\0'])
        .map(str::trim)
        .filter(|l| !l.is_empty());
    for (i, line) in lines.enumerate() {
        if i == 0 && !line.contains(':') {
            if line.eq_ignore_ascii_case("cut") {
                action = FileAction::Cut;
            }
            continue;
        }
        if let Some(p) = file_uri_to_path(line) {
            paths.push(p);
        }
    }
    (action, paths)
}

fn absolute(paths: &[String]) -> impl Iterator<Item = &String> {
    paths.iter().filter(|p| p.starts_with('/'))
}

/// Encode absolute paths as a `text/uri-list` body (CRLF-terminated lines).
/// Relative paths (e.g. a Windows path from a synced clip) are skipped.
pub fn encode_uri_list(paths: &[String]) -> Vec<u8> {
    let mut out = String::new();
    for p in absolute(paths) {
        out.push_str("file://");
        out.push_str(&percent_encode_path(p));
        out.push_str("\r\n");
    }
    out.into_bytes()
}

/// Encode absolute paths as `x-special/gnome-copied-files` (`copy` action;
/// lines separated by `\n`, no trailing newline, as Nautilus writes it).
pub fn encode_gnome_copied_files(paths: &[String]) -> Vec<u8> {
    let mut out = String::from("copy");
    for p in absolute(paths) {
        out.push('\n');
        out.push_str("file://");
        out.push_str(&percent_encode_path(p));
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b%2Fc"), b"a b/c");
        assert_eq!(percent_decode("%E6%97%A5"), "日".as_bytes());
        // malformed escapes stay literal
        assert_eq!(percent_decode("100%"), b"100%");
        assert_eq!(percent_decode("%zz%4"), b"%zz%4");
        assert_eq!(percent_decode("%41"), b"A");
    }

    #[test]
    fn file_uris() {
        assert_eq!(
            file_uri_to_path("file:///home/me/a%20b.txt").as_deref(),
            Some("/home/me/a b.txt")
        );
        assert_eq!(
            file_uri_to_path("file://localhost/tmp/x").as_deref(),
            Some("/tmp/x")
        );
        assert_eq!(file_uri_to_path("FILE:///tmp/x").as_deref(), Some("/tmp/x"));
        assert_eq!(file_uri_to_path("file://otherhost/tmp/x"), None);
        assert_eq!(file_uri_to_path("https://example.com/a"), None);
        assert_eq!(file_uri_to_path("file://"), None);
        assert_eq!(file_uri_to_path("not a uri"), None);
        assert_eq!(file_uri_to_path("file:///").as_deref(), Some("/"));
        assert_eq!(
            file_uri_to_path("file:///a/b%23c?x=1#frag").as_deref(),
            Some("/a/b#c")
        );
    }

    #[test]
    fn uri_list_parsing() {
        let data = b"# comment\r\nfile:///a/b%20c\r\nhttps://x.y/z\r\n\r\nfile:///d/\xC3\xA9\n";
        assert_eq!(parse_uri_list(data), vec!["/a/b c", "/d/é"]);
        assert!(parse_uri_list(b"https://example.com\r\n").is_empty());
        assert!(parse_uri_list(b"").is_empty());
    }

    #[test]
    fn gnome_copied_files_parsing() {
        let (a, p) = parse_gnome_copied_files(b"copy\nfile:///a%20b\nfile:///c");
        assert_eq!(a, FileAction::Copy);
        assert_eq!(p, vec!["/a b", "/c"]);
        let (a, p) = parse_gnome_copied_files(b"cut\r\nfile:///x\r\n");
        assert_eq!(a, FileAction::Cut);
        assert_eq!(p, vec!["/x"]);
        // no action line
        let (a, p) = parse_gnome_copied_files(b"file:///only");
        assert_eq!(a, FileAction::Copy);
        assert_eq!(p, vec!["/only"]);
    }

    #[test]
    fn encoding_roundtrips() {
        let paths = vec![
            "/home/me/a b.txt".to_string(),
            "/tmp/日本語/é#1%.png".to_string(),
            r"C:\windows\skipped.txt".to_string(),
        ];
        let list = encode_uri_list(&paths);
        let text = String::from_utf8(list.clone()).unwrap();
        assert!(text.starts_with("file:///home/me/a%20b.txt\r\n"));
        assert!(text.ends_with("\r\n"));
        assert_eq!(parse_uri_list(&list), &paths[..2]);

        let gnome = encode_gnome_copied_files(&paths);
        let text = String::from_utf8(gnome.clone()).unwrap();
        assert!(text.starts_with("copy\nfile:///home/me/a%20b.txt\nfile:///tmp/"));
        assert!(!text.ends_with('\n'));
        assert_eq!(
            parse_gnome_copied_files(&gnome),
            (FileAction::Copy, paths[..2].to_vec())
        );
    }

    #[test]
    fn encode_keeps_unreserved() {
        assert_eq!(percent_encode_path("/a-b_c.d~e/F9"), "/a-b_c.d~e/F9");
        assert_eq!(percent_encode_path("/a b"), "/a%20b");
    }
}
