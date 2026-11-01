//! Type-aware text transforms: "paste it the way you need it".
//!
//! Every clip already has a [`Kind`]; this module turns that knowledge into
//! actions (clean a tracking link, copy a color as RGB, prettify JSON, ...).
//! Pure functions over text, no OS access. [`available`] says which
//! transforms make sense for a clip *and* would change it; [`apply`] runs one.
//!
//! Rule: errors never contain clip content (only static messages).

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::model::Kind;

/// A text transform. Serialized kebab-case ("clean-url", "json-pretty", ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transform {
    // --- any text
    /// Identity for text. "Paste without formatting" is done at write time by
    /// dropping the HTML flavor; never listed by [`available`].
    PlainText,
    /// Trim, and strip trailing whitespace from every line.
    Trim,
    UpperCase,
    LowerCase,
    TitleCase,
    /// Join lines with single spaces.
    SingleLine,
    // --- link
    /// Remove tracking query parameters (utm_*, fbclid, gclid, ...).
    CleanUrl,
    /// `[host/path](url)`
    MarkdownLink,
    /// Percent-decode (`%20` -> space).
    UrlDecode,
    // --- color
    ColorHex,
    ColorRgb,
    ColorHsl,
    // --- code / JSON
    JsonPretty,
    JsonMinify,
    /// Fenced code block with the language from the clip's subkind.
    MarkdownCodeBlock,
    /// Remove common leading indentation.
    Dedent,
    // --- contact
    /// Keep only a leading `+` and the digits.
    PhoneDigits,
    /// `mailto:address`
    MailtoLink,
}

/// Run a transform. Fails (without echoing the text) when it doesn't apply,
/// e.g. [`Transform::JsonPretty`] on text that isn't valid JSON.
pub fn apply(t: Transform, text: &str) -> Result<String> {
    match t {
        Transform::PlainText => Ok(text.to_owned()),
        Transform::Trim => Ok(trim(text)),
        Transform::UpperCase => Ok(text.to_uppercase()),
        Transform::LowerCase => Ok(text.to_lowercase()),
        Transform::TitleCase => Ok(title_case(text)),
        Transform::SingleLine => Ok(single_line(text)),
        Transform::CleanUrl => clean_url(text),
        Transform::MarkdownLink => markdown_link(text),
        Transform::UrlDecode => url_decode(text),
        Transform::ColorHex => color_to(text, ColorFormat::Hex),
        Transform::ColorRgb => color_to(text, ColorFormat::Rgb),
        Transform::ColorHsl => color_to(text, ColorFormat::Hsl),
        Transform::JsonPretty => json_reformat(text, true),
        Transform::JsonMinify => json_reformat(text, false),
        Transform::MarkdownCodeBlock => Ok(markdown_code_block(text, None)),
        Transform::Dedent => Ok(dedent(text)),
        Transform::PhoneDigits => phone_digits(text),
        Transform::MailtoLink => mailto_link(text),
    }
}

/// Like [`apply`], but knows the clip's subkind (the language for
/// [`Transform::MarkdownCodeBlock`]).
pub fn apply_for(t: Transform, subkind: Option<&str>, text: &str) -> Result<String> {
    match t {
        Transform::MarkdownCodeBlock => Ok(markdown_code_block(text, subkind)),
        _ => apply(t, text),
    }
}

/// Transforms worth offering for a clip, best first: only those that make
/// sense for its kind and would actually change the text. Nothing for
/// sensitive, file and image clips.
pub fn available(kind: Kind, subkind: Option<&str>, text: &str) -> Vec<Transform> {
    use Transform::*;
    let t = text.trim();
    if t.is_empty() {
        return Vec::new();
    }
    let candidates: Vec<Transform> = match kind {
        Kind::Link => vec![CleanUrl, MarkdownLink, UrlDecode],
        Kind::Color => vec![ColorHex, ColorRgb, ColorHsl],
        Kind::Code => {
            let mut v = Vec::new();
            if subkind == Some("json") || t.starts_with('{') || t.starts_with('[') {
                v.extend([JsonPretty, JsonMinify]);
            }
            if !t.starts_with("```") {
                v.push(MarkdownCodeBlock);
            }
            if t.contains('\n') {
                v.push(Dedent);
            }
            v.push(Trim);
            v
        }
        Kind::Contact => match subkind {
            Some("email") => vec![MailtoLink],
            Some("phone") => vec![PhoneDigits],
            _ => vec![MailtoLink, PhoneDigits],
        },
        Kind::Text => {
            let mut v = vec![Trim, SingleLine];
            if t.contains('\n') {
                v.push(Dedent);
            }
            v.extend([UpperCase, LowerCase, TitleCase]);
            v
        }
        Kind::Sensitive | Kind::File | Kind::Image => return Vec::new(),
    };
    candidates
        .into_iter()
        .filter(|&c| offers(c, subkind, text))
        .collect()
}

/// True when `c` applies to `text` and the result differs from it.
fn offers(c: Transform, subkind: Option<&str>, text: &str) -> bool {
    // Don't offer "Copy as RGB" on something that already is rgb(); hex is
    // exempt because it also expands shorthand (#abc -> #aabbcc).
    if matches!(c, Transform::ColorRgb | Transform::ColorHsl) {
        let own = match parse_color(text.trim()) {
            Some((_, f)) => f,
            None => return false,
        };
        let target = if c == Transform::ColorRgb {
            ColorFormat::Rgb
        } else {
            ColorFormat::Hsl
        };
        if own == target {
            return false;
        }
    }
    match apply_for(c, subkind, text) {
        Ok(out) => {
            if matches!(c, Transform::JsonPretty | Transform::JsonMinify) {
                out.trim_end() != text.trim_end()
            } else {
                out != text
            }
        }
        Err(_) => false,
    }
}

// ------------------------------------------------------------------ helpers

/// Text split into lines, remembering the line ending style so a transform
/// can put it back (CRLF stays CRLF, a trailing newline stays).
struct Lines<'a> {
    lines: Vec<&'a str>,
    eol: &'static str,
    trailing: bool,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let trailing = text.ends_with('\n');
        let mut lines: Vec<&str> = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect();
        if trailing {
            lines.pop();
        }
        Lines {
            lines,
            eol,
            trailing,
        }
    }

    fn join(&self, lines: &[String]) -> String {
        let mut s = lines.join(self.eol);
        if self.trailing {
            s.push_str(self.eol);
        }
        s
    }
}

// --------------------------------------------------------------- plain text

fn trim(text: &str) -> String {
    let lines = Lines::new(text.trim());
    let out: Vec<String> = lines
        .lines
        .iter()
        .map(|l| l.trim_end().to_owned())
        .collect();
    out.join(lines.eol)
}

fn single_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Capitalize the first letter of every word, lowercase the rest. A word is
/// a run of letters/digits/apostrophes ("don't" stays one word).
fn title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_start = true;
    for c in text.chars() {
        if c.is_alphanumeric() || c == '\'' || c == '\u{2019}' {
            if at_start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            at_start = false;
        } else {
            out.push(c);
            at_start = true;
        }
    }
    out
}

// --------------------------------------------------------------------- code

fn dedent(text: &str) -> String {
    let lines = Lines::new(text);
    let mut common: Option<&str> = None;
    for l in lines.lines.iter().filter(|l| !l.trim().is_empty()) {
        let indent = &l[..l.len() - l.trim_start().len()];
        common = Some(match common {
            None => indent,
            Some(c) => common_prefix(c, indent),
        });
    }
    let prefix = common.unwrap_or("");
    let out: Vec<String> = lines
        .lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                l.strip_prefix(prefix).unwrap_or(l).to_owned()
            }
        })
        .collect();
    lines.join(&out)
}

fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    &a[..n]
}

fn markdown_code_block(text: &str, lang: Option<&str>) -> String {
    let body = text.trim_end_matches(['\n', '\r']);
    let longest_run = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat((longest_run + 1).max(3));
    let lang: String = lang
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '#' | '.' | '-'))
        .collect();
    format!("{fence}{lang}\n{body}\n{fence}")
}

/// Pretty-print (2 spaces) or minify JSON without touching key order,
/// number formatting or string escapes: validate with serde_json, then
/// re-space the original tokens.
fn json_reformat(text: &str, pretty: bool) -> Result<String> {
    let src = text.trim();
    if serde_json::from_str::<serde::de::IgnoredAny>(src).is_err() {
        bail!("not valid JSON");
    }
    let mut out = String::with_capacity(src.len() + src.len() / 4);
    let mut depth = 0usize;
    let mut chars = src.chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        for _ in 0..depth {
            out.push_str("  ");
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                let mut escaped = false;
                for s in chars.by_ref() {
                    out.push(s);
                    if escaped {
                        escaped = false;
                    } else if s == '\\' {
                        escaped = true;
                    } else if s == '"' {
                        break;
                    }
                }
            }
            '{' | '[' => {
                out.push(c);
                while chars.next_if(|w| w.is_whitespace()).is_some() {}
                let closer = if c == '{' { '}' } else { ']' };
                if chars.next_if_eq(&closer).is_some() {
                    out.push(closer); // empty container stays `{}` / `[]`
                } else {
                    depth += 1;
                    if pretty {
                        newline(&mut out, depth);
                    }
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                if pretty {
                    newline(&mut out, depth);
                }
                out.push(c);
            }
            ',' => {
                out.push(',');
                if pretty {
                    newline(&mut out, depth);
                }
            }
            ':' => out.push_str(if pretty { ": " } else { ":" }),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    Ok(out)
}

// --------------------------------------------------------------------- link

/// Parse a single-line link and return it trimmed, plus the parsed URL.
fn parse_link(text: &str) -> Result<(&str, Url)> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        bail!("not a single link");
    }
    match Url::parse(t) {
        Ok(u) if u.has_host() => Ok((t, u)),
        _ => bail!("not a link"),
    }
}

/// Parameters that only identify the click, never the page.
fn is_tracking_param(key: &str, host: &str) -> bool {
    let k = key.to_ascii_lowercase();
    if k.starts_with("utm_") || k.starts_with("wt_mc") || k.starts_with("pk_") {
        return true;
    }
    if matches!(
        k.as_str(),
        "fbclid"
            | "gclid"
            | "gclsrc"
            | "gbraid"
            | "wbraid"
            | "dclid"
            | "msclkid"
            | "mc_cid"
            | "mc_eid"
            | "igshid"
            | "ref_src"
            | "ref_url"
            | "spm"
            | "_hsenc"
            | "_hsmi"
            | "__hssc"
            | "__hstc"
            | "__hsfp"
            | "hsctatracking"
            | "yclid"
            | "twclid"
            | "ttclid"
            | "li_fat_id"
            | "mkt_tok"
            | "_ga"
            | "_gl"
            | "vero_id"
            | "oly_enc_id"
            | "oly_anon_id"
            | "s_cid"
    ) {
        return true;
    }
    let on = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    // `si` is YouTube's / Spotify's share id; `s`/`t` on X are share tokens.
    // Generic sites may use these names for real, so scope them by host.
    match k.as_str() {
        "si" => on("youtube.com") || on("youtu.be") || on("spotify.com") || on("spotify.link"),
        "s" | "t" => on("twitter.com") || on("x.com"),
        _ => false,
    }
}

/// Strip tracking parameters while leaving every other byte of the URL
/// (case, encoding, parameter order, fragment) exactly as copied.
fn clean_url(text: &str) -> Result<String> {
    let (t, parsed) = parse_link(text)?;
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let (rest, fragment) = match t.split_once('#') {
        Some((r, f)) => (r, Some(f)),
        None => (t, None),
    };
    let Some((base, query)) = rest.split_once('?') else {
        return Ok(t.to_owned());
    };
    let segments: Vec<&str> = query.split('&').collect();
    let kept: Vec<&str> = segments
        .iter()
        .copied()
        .filter(|seg| {
            let key = seg.split_once('=').map_or(*seg, |(k, _)| k);
            !is_tracking_param(&percent_decode_lossy(key), &host)
        })
        .collect();
    if kept.len() == segments.len() {
        return Ok(t.to_owned()); // nothing to strip: keep every byte
    }
    let mut out = base.to_owned();
    let q = kept.join("&");
    if !q.is_empty() {
        out.push('?');
        out.push_str(&q);
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    Ok(out)
}

fn markdown_link(text: &str) -> Result<String> {
    let (t, parsed) = parse_link(text)?;
    let host = parsed.host_str().unwrap_or("");
    let host = host.strip_prefix("www.").unwrap_or(host);
    let path = parsed.path().trim_end_matches('/');
    let label = format!("{host}{path}")
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    let dest = t.replace('(', "%28").replace(')', "%29");
    Ok(format!("[{label}]({dest})"))
}

fn url_decode(text: &str) -> Result<String> {
    let t = text.trim();
    let bytes = percent_decode(t);
    match String::from_utf8(bytes) {
        Ok(s) => Ok(s),
        Err(_) => bail!("not valid UTF-8 after decoding"),
    }
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2]))
        {
            out.push((h << 4) | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn percent_decode_lossy(s: &str) -> String {
    String::from_utf8_lossy(&percent_decode(s)).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

// ------------------------------------------------------------------ contact

fn phone_digits(text: &str) -> Result<String> {
    let t = text.trim();
    let mut out = String::new();
    if t.starts_with('+') {
        out.push('+');
    }
    out.extend(t.chars().filter(char::is_ascii_digit));
    if out.trim_start_matches('+').is_empty() {
        bail!("no digits");
    }
    Ok(out)
}

fn mailto_link(text: &str) -> Result<String> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) || !t.contains('@') {
        bail!("not an email address");
    }
    if t.len() >= 7 && t[..7].eq_ignore_ascii_case("mailto:") {
        return Ok(t.to_owned());
    }
    Ok(format!("mailto:{t}"))
}

// -------------------------------------------------------------------- color

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorFormat {
    Hex,
    Rgb,
    Hsl,
}

#[derive(Debug, Clone, Copy)]
struct Rgba {
    r: u8,
    g: u8,
    b: u8,
    /// 0.0 ..= 1.0
    a: f64,
}

fn color_to(text: &str, to: ColorFormat) -> Result<String> {
    let t = text.trim();
    let Some((c, _)) = parse_color(t) else {
        bail!("not a color");
    };
    let a3 = (c.a * 1000.0).round() / 1000.0;
    let opaque = a3 >= 1.0;
    Ok(match to {
        ColorFormat::Hex => {
            let upper = t.starts_with('#') && t.bytes().any(|b| matches!(b, b'A'..=b'F'));
            let alpha_byte = (c.a * 255.0).round() as u8;
            let mut s = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
            if !opaque && alpha_byte != 255 {
                s.push_str(&format!("{alpha_byte:02x}"));
            }
            if upper { s.to_uppercase() } else { s }
        }
        ColorFormat::Rgb => {
            if opaque {
                format!("rgb({}, {}, {})", c.r, c.g, c.b)
            } else {
                format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, fmt_num(a3, 3))
            }
        }
        ColorFormat::Hsl => {
            let (h, s, l) = rgb_to_hsl(c.r, c.g, c.b);
            let (h, s, l) = (fmt_num(h, 1), fmt_num(s, 1), fmt_num(l, 1));
            if opaque {
                format!("hsl({h}, {s}%, {l}%)")
            } else {
                format!("hsla({h}, {s}%, {l}%, {})", fmt_num(a3, 3))
            }
        }
    })
}

/// Fixed decimals with trailing zeros trimmed ("50.0" -> "50", "-0" -> "0").
fn fmt_num(x: f64, decimals: usize) -> String {
    let s = format!("{x:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    };
    if s == "-0" { "0".into() } else { s }
}

fn parse_color(t: &str) -> Option<(Rgba, ColorFormat)> {
    if let Some(hex) = t.strip_prefix('#') {
        return parse_hex(hex).map(|c| (c, ColorFormat::Hex));
    }
    let lower = t.to_ascii_lowercase();
    let (name, rest) = lower.split_once('(')?;
    let inner = rest.strip_suffix(')')?;
    let fmt = match name.trim() {
        "rgb" | "rgba" => ColorFormat::Rgb,
        "hsl" | "hsla" => ColorFormat::Hsl,
        _ => return None,
    };
    let parts: Vec<&str> = inner
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() != 3 && parts.len() != 4 {
        return None;
    }
    let a = match parts.get(3) {
        Some(p) => parse_alpha(p)?,
        None => 1.0,
    };
    let c = if fmt == ColorFormat::Rgb {
        let ch = |p: &str| -> Option<u8> {
            let v = match p.strip_suffix('%') {
                Some(n) => num(n)? * 2.55,
                None => num(p)?,
            };
            Some(v.round().clamp(0.0, 255.0) as u8)
        };
        Rgba {
            r: ch(parts[0])?,
            g: ch(parts[1])?,
            b: ch(parts[2])?,
            a,
        }
    } else {
        let h = parse_hue(parts[0])?;
        let s = num(parts[1].trim_end_matches('%'))?.clamp(0.0, 100.0) / 100.0;
        let l = num(parts[2].trim_end_matches('%'))?.clamp(0.0, 100.0) / 100.0;
        let (r, g, b) = hsl_to_rgb(h, s, l);
        Rgba { r, g, b, a }
    };
    Some((c, fmt))
}

fn num(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn parse_alpha(p: &str) -> Option<f64> {
    let v = match p.strip_suffix('%') {
        Some(n) => num(n)? / 100.0,
        None => num(p)?,
    };
    Some(v.clamp(0.0, 1.0))
}

fn parse_hue(p: &str) -> Option<f64> {
    let (n, factor) = if let Some(n) = p.strip_suffix("deg") {
        (n, 1.0)
    } else if let Some(n) = p.strip_suffix("grad") {
        (n, 0.9)
    } else if let Some(n) = p.strip_suffix("rad") {
        (n, 180.0 / std::f64::consts::PI)
    } else if let Some(n) = p.strip_suffix("turn") {
        (n, 360.0)
    } else {
        (p, 1.0)
    };
    Some((num(n)? * factor).rem_euclid(360.0))
}

fn parse_hex(h: &str) -> Option<Rgba> {
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let d: Vec<u8> = h.bytes().filter_map(hex_val).collect();
    let (r, g, b, a) = match d.len() {
        3 => (d[0] * 17, d[1] * 17, d[2] * 17, 255),
        4 => (d[0] * 17, d[1] * 17, d[2] * 17, d[3] * 17),
        6 => (
            (d[0] << 4) | d[1],
            (d[2] << 4) | d[3],
            (d[4] << 4) | d[5],
            255,
        ),
        8 => (
            (d[0] << 4) | d[1],
            (d[2] << 4) | d[3],
            (d[4] << 4) | d[5],
            (d[6] << 4) | d[7],
        ),
        _ => return None,
    };
    Some(Rgba {
        r,
        g,
        b,
        a: f64::from(a) / 255.0,
    })
}

/// `h` in degrees [0, 360), `s`/`l` in [0, 1].
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to8 = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to8(r1), to8(g1), to8(b1))
}

/// Returns (hue degrees, saturation %, lightness %).
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (r, g, b) = (
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l * 100.0);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    ((h * 60.0).rem_euclid(360.0), s * 100.0, l * 100.0)
}

#[cfg(test)]
mod tests {
    use super::Transform::*;
    use super::*;

    fn ok(t: Transform, s: &str) -> String {
        apply(t, s).unwrap()
    }

    // ----------------------------------------------------------- serde

    #[test]
    fn serde_is_kebab_case() {
        assert_eq!(serde_json::to_string(&CleanUrl).unwrap(), "\"clean-url\"");
        assert_eq!(
            serde_json::to_string(&MarkdownCodeBlock).unwrap(),
            "\"markdown-code-block\""
        );
        assert_eq!(serde_json::to_string(&ColorHsl).unwrap(), "\"color-hsl\"");
        let t: Transform = serde_json::from_str("\"json-pretty\"").unwrap();
        assert_eq!(t, JsonPretty);
    }

    // ------------------------------------------------------ plain text

    #[test]
    fn plain_text_is_identity() {
        assert_eq!(ok(PlainText, "  a\tb \n"), "  a\tb \n");
    }

    #[test]
    fn trim_strips_ends_and_trailing_line_space() {
        assert_eq!(ok(Trim, "  hello  \n  world \t\n\n"), "hello\n  world");
        assert_eq!(ok(Trim, "a  \r\nb \r\n"), "a\r\nb");
        assert_eq!(ok(Trim, ""), "");
    }

    #[test]
    fn case_transforms() {
        assert_eq!(ok(UpperCase, "Hello, wörld"), "HELLO, WÖRLD");
        assert_eq!(ok(LowerCase, "Hello, WÖRLD"), "hello, wörld");
        assert_eq!(ok(TitleCase, "the quick BROWN fox"), "The Quick Brown Fox");
        assert_eq!(ok(TitleCase, "don't stop-me now"), "Don't Stop-Me Now");
        assert_eq!(
            ok(TitleCase, "3rd place\nsecond line"),
            "3rd Place\nSecond Line"
        );
        assert_eq!(ok(TitleCase, "élan vital"), "Élan Vital");
    }

    #[test]
    fn single_line_joins_with_spaces() {
        assert_eq!(ok(SingleLine, "a\n  b  \r\n\n c\n"), "a b c");
        assert_eq!(ok(SingleLine, "one"), "one");
    }

    // ------------------------------------------------------------ link

    #[test]
    fn clean_url_strips_known_params() {
        assert_eq!(
            ok(
                CleanUrl,
                "https://example.com/p?utm_source=x&utm_medium=y&utm_campaign=z"
            ),
            "https://example.com/p"
        );
        assert_eq!(
            ok(
                CleanUrl,
                "https://example.com/?fbclid=AbC&gclid=1&dclid=2&msclkid=3"
            ),
            "https://example.com/"
        );
        for p in [
            "mc_cid", "mc_eid", "igshid", "ref_src", "spm", "_hsenc", "_hsmi", "yclid", "twclid",
            "wt_mc", "wt_mc_id",
        ] {
            let u = format!("https://example.com/a?{p}=1");
            assert_eq!(ok(CleanUrl, &u), "https://example.com/a", "{p}");
        }
    }

    #[test]
    fn clean_url_keeps_real_params_in_order() {
        assert_eq!(
            ok(
                CleanUrl,
                "https://shop.com/s?q=rust%20book&utm_source=nl&page=2&fbclid=x&sort=asc"
            ),
            "https://shop.com/s?q=rust%20book&page=2&sort=asc"
        );
    }

    #[test]
    fn clean_url_keeps_fragment_and_bytes() {
        assert_eq!(
            ok(
                CleanUrl,
                "https://Example.com/Docs/Page?utm_source=a&x=1#Section-2"
            ),
            "https://Example.com/Docs/Page?x=1#Section-2"
        );
        assert_eq!(
            ok(CleanUrl, "https://example.com/p?utm_source=a#frag"),
            "https://example.com/p#frag"
        );
        // A `?` inside the fragment (hash router) is not a query.
        assert_eq!(
            ok(CleanUrl, "https://app.io/#/page?utm_source=a"),
            "https://app.io/#/page?utm_source=a"
        );
    }

    #[test]
    fn clean_url_untouched_urls_are_byte_identical() {
        let u = "https://example.com/a%20b/C?x=1&y=%7Efoo";
        assert_eq!(ok(CleanUrl, u), u);
        let u = "https://example.com";
        assert_eq!(ok(CleanUrl, u), u);
        let u = "https://example.com/?";
        assert_eq!(ok(CleanUrl, u), u);
    }

    #[test]
    fn clean_url_param_names_case_and_encoding() {
        assert_eq!(
            ok(CleanUrl, "https://e.com/?UTM_Source=a&k=v"),
            "https://e.com/?k=v"
        );
        assert_eq!(
            ok(CleanUrl, "https://e.com/?utm%5Fsource=a&k=v"),
            "https://e.com/?k=v"
        );
        // Prefix only: "utmost" is a real parameter.
        assert_eq!(
            ok(CleanUrl, "https://e.com/?utmost=1"),
            "https://e.com/?utmost=1"
        );
        // Flag-style params without a value.
        assert_eq!(ok(CleanUrl, "https://e.com/?gclid&a"), "https://e.com/?a");
    }

    #[test]
    fn clean_url_host_scoped_params() {
        assert_eq!(
            ok(CleanUrl, "https://youtu.be/dQw4w9WgXcQ?si=abc123&t=42"),
            "https://youtu.be/dQw4w9WgXcQ?t=42"
        );
        assert_eq!(
            ok(
                CleanUrl,
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ&si=xyz"
            ),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        assert_eq!(
            ok(CleanUrl, "https://open.spotify.com/track/1?si=deadbeef"),
            "https://open.spotify.com/track/1"
        );
        assert_eq!(
            ok(CleanUrl, "https://x.com/user/status/1?s=20&t=abc"),
            "https://x.com/user/status/1"
        );
        // Elsewhere `si`, `s` and `t` are real parameters.
        assert_eq!(
            ok(CleanUrl, "https://example.com/search?s=rust&si=2&t=3"),
            "https://example.com/search?s=rust&si=2&t=3"
        );
        // Look-alike hosts don't match.
        assert_eq!(
            ok(CleanUrl, "https://notyoutube.com/?si=1"),
            "https://notyoutube.com/?si=1"
        );
    }

    #[test]
    fn clean_url_rejects_non_links() {
        assert!(apply(CleanUrl, "hello world").is_err());
        assert!(apply(CleanUrl, "https://a.com/x https://b.com").is_err());
        assert!(apply(CleanUrl, "").is_err());
    }

    #[test]
    fn markdown_link_uses_host_and_path() {
        assert_eq!(
            ok(
                MarkdownLink,
                "https://github.com/tauri-apps/tauri/pull/1?x=1"
            ),
            "[github.com/tauri-apps/tauri/pull/1](https://github.com/tauri-apps/tauri/pull/1?x=1)"
        );
        assert_eq!(
            ok(MarkdownLink, "https://www.rust-lang.org/"),
            "[rust-lang.org](https://www.rust-lang.org/)"
        );
        assert_eq!(
            ok(
                MarkdownLink,
                "https://en.wikipedia.org/wiki/Rust_(programming_language)"
            ),
            "[en.wikipedia.org/wiki/Rust_(programming_language)](https://en.wikipedia.org/wiki/Rust_%28programming_language%29)"
        );
        assert!(apply(MarkdownLink, "nope").is_err());
    }

    #[test]
    fn url_decode_decodes_percent_sequences() {
        assert_eq!(
            ok(UrlDecode, "https://e.com/a%20b?q=caf%C3%A9&r=100%25"),
            "https://e.com/a b?q=café&r=100%"
        );
        // Stray `%` and truncated escapes are left alone.
        assert_eq!(ok(UrlDecode, "50%"), "50%");
        assert_eq!(ok(UrlDecode, "a%2"), "a%2");
        assert_eq!(ok(UrlDecode, "a%zzb"), "a%zzb");
        assert!(apply(UrlDecode, "bad%FF").is_err());
    }

    // ----------------------------------------------------------- color

    #[test]
    fn color_conversions_known_values() {
        assert_eq!(ok(ColorRgb, "#ff0000"), "rgb(255, 0, 0)");
        assert_eq!(ok(ColorRgb, "#f00"), "rgb(255, 0, 0)");
        assert_eq!(ok(ColorHsl, "#ff0000"), "hsl(0, 100%, 50%)");
        assert_eq!(ok(ColorHsl, "#00ff00"), "hsl(120, 100%, 50%)");
        assert_eq!(ok(ColorHsl, "#0000ff"), "hsl(240, 100%, 50%)");
        assert_eq!(ok(ColorHsl, "#ffffff"), "hsl(0, 0%, 100%)");
        assert_eq!(ok(ColorHsl, "#000"), "hsl(0, 0%, 0%)");
        assert_eq!(ok(ColorHex, "rgb(255, 128, 0)"), "#ff8000");
        assert_eq!(ok(ColorHex, "hsl(120, 100%, 25%)"), "#008000");
        assert_eq!(ok(ColorHex, "#abc"), "#aabbcc");
        assert_eq!(ok(ColorHex, "#ABC"), "#AABBCC");
    }

    #[test]
    fn color_alpha_is_preserved() {
        assert_eq!(ok(ColorRgb, "#ff000080"), "rgba(255, 0, 0, 0.502)");
        assert_eq!(ok(ColorHsl, "#ff000080"), "hsla(0, 100%, 50%, 0.502)");
        assert_eq!(ok(ColorHex, "rgba(255, 0, 0, 0.5)"), "#ff000080");
        assert_eq!(ok(ColorHex, "rgb(255 0 0 / 50%)"), "#ff000080");
        assert_eq!(ok(ColorHex, "hsla(0, 100%, 50%, 0.5)"), "#ff000080");
        assert_eq!(ok(ColorHex, "#f008"), "#ff000088");
        // Fully opaque alpha is dropped.
        assert_eq!(ok(ColorRgb, "#ff0000ff"), "rgb(255, 0, 0)");
        assert_eq!(ok(ColorHex, "rgba(1, 2, 3, 1)"), "#010203");
    }

    #[test]
    fn color_parses_css_syntax_variants() {
        assert_eq!(ok(ColorHex, "rgb(100%, 0%, 0%)"), "#ff0000");
        assert_eq!(ok(ColorHex, "RGB(255 0 0)"), "#ff0000");
        assert_eq!(ok(ColorHex, "hsl(0.5turn 100% 50%)"), "#00ffff");
        assert_eq!(ok(ColorHex, "hsl(180deg, 100%, 50%)"), "#00ffff");
        assert_eq!(ok(ColorHex, "hsl(-120, 100%, 50%)"), "#0000ff");
        assert_eq!(ok(ColorHex, "rgb(300, -5, 0)"), "#ff0000");
        assert!(apply(ColorHex, "rgb(1, 2)").is_err());
        assert!(apply(ColorHex, "#12345").is_err());
        assert!(apply(ColorHex, "red").is_err());
        assert!(apply(ColorHex, "#ggg").is_err());
    }

    #[test]
    fn color_round_trips() {
        // hex -> rgb -> hex is exact; hex -> hsl -> hex is exact at the
        // 0.1 precision we print, including alpha.
        let mut n = 0u32;
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(51) {
                    for alpha in ["", "80", "1a", "fe"] {
                        let hex = format!("#{r:02x}{g:02x}{b:02x}{alpha}");
                        let via_rgb = ok(ColorHex, &ok(ColorRgb, &hex));
                        let via_hsl = ok(ColorHex, &ok(ColorHsl, &hex));
                        assert_eq!(via_rgb, hex, "rgb round trip");
                        assert_eq!(via_hsl, hex, "hsl round trip");
                        n += 1;
                    }
                }
            }
        }
        assert!(n > 1000);
    }

    // ------------------------------------------------------------ JSON

    #[test]
    fn json_pretty_basic_and_order_preserved() {
        let out = ok(
            JsonPretty,
            r#"{"b":1,"a":[1,2,{"z":null,"y":true}],"c":"x"}"#,
        );
        assert_eq!(
            out,
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2,\n    {\n      \"z\": null,\n      \"y\": true\n    }\n  ],\n  \"c\": \"x\"\n}"
        );
    }

    #[test]
    fn json_edge_cases() {
        assert_eq!(ok(JsonPretty, "{}"), "{}");
        assert_eq!(ok(JsonPretty, "[ ]"), "[]");
        assert_eq!(
            ok(JsonPretty, r#"{"a":{},"b":[]}"#),
            "{\n  \"a\": {},\n  \"b\": []\n}"
        );
        // Strings keep escapes and structural characters untouched.
        assert_eq!(
            ok(JsonMinify, r#"{ "k" : "a, b: {c} [d] \" \\ é" }"#),
            r#"{"k":"a, b: {c} [d] \" \\ é"}"#
        );
        // Numbers are not reformatted (no 1.0 -> 1, no precision loss).
        assert_eq!(
            ok(JsonMinify, r#"[1.0, 1e5, 12345678901234567890, -0.0]"#),
            "[1.0,1e5,12345678901234567890,-0.0]"
        );
        // Scalars are valid JSON too.
        assert_eq!(ok(JsonPretty, "  42  "), "42");
        // Unicode untouched.
        assert_eq!(ok(JsonMinify, "{ \"é\": \"日本\" }"), "{\"é\":\"日本\"}");
    }

    #[test]
    fn json_pretty_is_idempotent_and_round_trips() {
        let src = r#"{"a":[1,2,3],"b":{"c":"d, e","f":[]}}"#;
        let pretty = ok(JsonPretty, src);
        assert_eq!(ok(JsonPretty, &pretty), pretty);
        assert_eq!(ok(JsonMinify, &pretty), src);
    }

    #[test]
    fn json_invalid_is_rejected() {
        for bad in [
            "{",
            "{'a':1}",
            "{\"a\":1,}",
            "[1 2]",
            "hello",
            "",
            "{\"a\":}",
        ] {
            assert!(apply(JsonPretty, bad).is_err(), "{bad:?}");
            assert!(apply(JsonMinify, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn markdown_code_block_uses_language_and_safe_fence() {
        assert_eq!(
            apply_for(MarkdownCodeBlock, Some("rust"), "fn main() {}\n").unwrap(),
            "```rust\nfn main() {}\n```"
        );
        assert_eq!(
            apply_for(MarkdownCodeBlock, None, "x = 1").unwrap(),
            "```\nx = 1\n```"
        );
        // Content with a ``` fence gets a longer one.
        assert_eq!(
            apply_for(MarkdownCodeBlock, Some("markdown"), "```js\na\n```").unwrap(),
            "````markdown\n```js\na\n```\n````"
        );
        // Hostile subkind can't break the fence line.
        assert_eq!(
            apply_for(MarkdownCodeBlock, Some("a\nb`c"), "x").unwrap(),
            "```abc\nx\n```"
        );
    }

    #[test]
    fn dedent_spaces() {
        assert_eq!(ok(Dedent, "    a\n      b\n    c"), "a\n  b\nc");
        assert_eq!(ok(Dedent, "  a\n    b\n"), "a\n  b\n");
    }

    #[test]
    fn dedent_tabs_and_mixed() {
        assert_eq!(ok(Dedent, "\t\ta\n\t\t\tb"), "a\n\tb");
        // Different indent characters: only the shared prefix is removed.
        assert_eq!(ok(Dedent, "\ta\n    b"), "\ta\n    b");
        assert_eq!(ok(Dedent, "\t  a\n\t    b"), "a\n  b");
    }

    #[test]
    fn dedent_blank_lines_and_edges() {
        // Blank lines don't count toward the common indent and become empty.
        assert_eq!(ok(Dedent, "    a\n\n  \n    b"), "a\n\n\nb");
        // Nothing to remove.
        assert_eq!(ok(Dedent, "a\n  b"), "a\n  b");
        assert_eq!(ok(Dedent, ""), "");
        assert_eq!(ok(Dedent, "   \n  "), "\n");
        // CRLF is preserved.
        assert_eq!(ok(Dedent, "  a\r\n  b\r\n"), "a\r\nb\r\n");
    }

    // --------------------------------------------------------- contact

    #[test]
    fn phone_digits_keeps_plus_and_digits() {
        assert_eq!(ok(PhoneDigits, "+1 (555) 123-4567"), "+15551234567");
        assert_eq!(ok(PhoneDigits, "(555) 123-4567"), "5551234567");
        assert_eq!(ok(PhoneDigits, " +44 20 7946 0958 "), "+442079460958");
        assert_eq!(ok(PhoneDigits, "555.123.4567"), "5551234567");
        assert!(apply(PhoneDigits, "call me").is_err());
        assert!(apply(PhoneDigits, "+").is_err());
    }

    #[test]
    fn mailto_link_prefixes_address() {
        assert_eq!(
            ok(MailtoLink, " a.b@example.com "),
            "mailto:a.b@example.com"
        );
        assert_eq!(ok(MailtoLink, "mailto:a@b.co"), "mailto:a@b.co");
        assert!(apply(MailtoLink, "not an email").is_err());
        assert!(apply(MailtoLink, "").is_err());
    }

    // ------------------------------------------------------- available

    #[test]
    fn available_by_kind() {
        let l = available(
            Kind::Link,
            Some("example.com"),
            "https://example.com/a%20b?utm_source=x",
        );
        assert_eq!(l, vec![CleanUrl, MarkdownLink, UrlDecode]);

        let l = available(Kind::Link, None, "https://example.com/");
        assert_eq!(l, vec![MarkdownLink]);

        assert_eq!(
            available(Kind::Color, Some("hex"), "#ff0000"),
            vec![ColorRgb, ColorHsl]
        );
        assert_eq!(
            available(Kind::Color, Some("rgb"), "rgb(255, 0, 0)"),
            vec![ColorHex, ColorHsl]
        );
        assert_eq!(
            available(Kind::Color, Some("hex"), "#f00"),
            vec![ColorHex, ColorRgb, ColorHsl]
        );

        assert_eq!(
            available(Kind::Code, Some("json"), r#"{"a":1}"#),
            vec![JsonPretty, MarkdownCodeBlock]
        );
        assert_eq!(
            available(Kind::Code, Some("json"), "{\n  \"a\": 1\n}"),
            vec![JsonMinify, MarkdownCodeBlock]
        );
        assert_eq!(
            available(Kind::Code, Some("rust"), "    fn a() {}\n    fn b() {}"),
            vec![MarkdownCodeBlock, Dedent, Trim]
        );

        assert_eq!(
            available(Kind::Contact, Some("phone"), "+1 (555) 123-4567"),
            vec![PhoneDigits]
        );
        assert_eq!(
            available(Kind::Contact, Some("phone"), "+15551234567"),
            Vec::<Transform>::new()
        );
        assert_eq!(
            available(Kind::Contact, Some("email"), "a@b.co"),
            vec![MailtoLink]
        );

        assert_eq!(
            available(Kind::Text, None, "  hello world  "),
            vec![Trim, SingleLine, UpperCase, TitleCase]
        );
    }

    #[test]
    fn available_text_multiline() {
        let v = available(Kind::Text, None, "one\ntwo");
        assert!(v.contains(&SingleLine));
        let v = available(Kind::Text, None, "one two");
        assert!(!v.contains(&SingleLine));
    }

    #[test]
    fn available_nothing_for_opaque_kinds() {
        for k in [Kind::Sensitive, Kind::File, Kind::Image] {
            assert!(available(k, None, "hello  world").is_empty());
        }
        assert!(available(Kind::Text, None, "   ").is_empty());
        assert!(available(Kind::Text, None, "").is_empty());
    }

    #[test]
    fn available_never_offers_a_noop() {
        let samples: &[(Kind, Option<&str>, &str)] = &[
            (Kind::Text, None, "hello"),
            (Kind::Text, None, "HELLO"),
            (Kind::Text, None, "Hello World"),
            (Kind::Text, None, "a\nb\nc"),
            (Kind::Text, None, "  indented\n  text"),
            (Kind::Link, Some("e.com"), "https://e.com/"),
            (Kind::Link, Some("e.com"), "https://e.com/p?a=1&b=2#x"),
            (Kind::Link, Some("e.com"), "https://e.com/p?utm_source=a"),
            (Kind::Link, Some("e.com"), "https://e.com/a%2Fb"),
            (Kind::Color, Some("hex"), "#ff8800"),
            (Kind::Color, Some("hex"), "#FF8800"),
            (Kind::Color, Some("hex"), "#f80"),
            (Kind::Color, Some("rgb"), "rgb(1, 2, 3)"),
            (Kind::Color, Some("hsl"), "hsl(210, 50%, 40%)"),
            (Kind::Color, Some("hsl"), "hsl(210 50% 40% / 0.5)"),
            (Kind::Code, Some("json"), "{}"),
            (Kind::Code, Some("json"), "[]"),
            (Kind::Code, Some("json"), "{\"a\":1}"),
            (Kind::Code, Some("json"), "{\n  \"a\": 1\n}"),
            (Kind::Code, Some("json"), "{\n  \"a\": 1\n}\n"),
            (Kind::Code, Some("rust"), "fn main() {}"),
            (Kind::Code, None, "x = 1"),
            (Kind::Code, None, "```\nx\n```"),
            (Kind::Contact, Some("phone"), "+15551234567"),
            (Kind::Contact, Some("phone"), "555 123 4567"),
            (Kind::Contact, Some("email"), "a@b.co"),
        ];
        for &(kind, sub, text) in samples {
            for t in available(kind, sub, text) {
                let out = apply_for(t, sub, text).unwrap();
                assert_ne!(out, text, "{t:?} on {text:?} is a no-op");
            }
        }
    }

    #[test]
    fn available_results_always_apply() {
        let text = "https://e.com/a%20b?utm_source=x&q=1";
        for t in available(Kind::Link, None, text) {
            assert!(apply(t, text).is_ok());
        }
    }

    #[test]
    fn errors_never_echo_content() {
        let secret = "hunter-secret-body";
        for t in [
            CleanUrl,
            MarkdownLink,
            ColorHex,
            JsonPretty,
            JsonMinify,
            PhoneDigits,
            MailtoLink,
        ] {
            let e = apply(t, secret).unwrap_err().to_string();
            assert!(!e.contains(secret), "{t:?}");
        }
    }
}
