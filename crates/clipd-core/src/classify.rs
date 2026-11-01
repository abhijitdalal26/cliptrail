//! Text classifier: decides what kind of data was copied so the UI can show
//! and paste it appropriately.
//!
//! Checks run in order and the first match wins: sensitive → link →
//! contact → color → file path → code → text. Everything is heuristic and
//! local; regexes compile once. Only the first [`MAX_ANALYZE_CHARS`] chars
//! are looked at, so huge clips stay cheap. Never panics on any input.

use std::sync::LazyLock;

use regex::Regex;

use crate::model::{Classification, Kind};

/// Analysis cap; longer texts are classified by their beginning.
pub const MAX_ANALYZE_CHARS: usize = 20_000;

/// Classify a piece of copied text. Pure, fast (< 1 ms typical), local.
pub fn classify_text(text: &str) -> Classification {
    let t = head(text, MAX_ANALYZE_CHARS).trim();
    if t.is_empty() {
        return class(Kind::Text, None);
    }
    if let Some(sub) = sensitive(t) {
        return class(Kind::Sensitive, Some(sub));
    }
    if let Some(c) = link(t) {
        return c;
    }
    if let Some(sub) = contact(t) {
        return class(Kind::Contact, Some(sub));
    }
    if let Some(sub) = color(t) {
        return class(Kind::Color, Some(sub));
    }
    if is_path(t) {
        return class(Kind::File, Some("path"));
    }
    if let Some(lang) = code(t) {
        return class(Kind::Code, lang);
    }
    class(Kind::Text, None)
}

fn class(kind: Kind, subkind: Option<&str>) -> Classification {
    Classification {
        kind,
        subkind: subkind.map(str::to_owned),
    }
}

/// First `n` chars of `s`.
fn head(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Compile a constant pattern. `\b` is made ASCII-only so the fast DFA
/// engines stay usable on non-ASCII input.
fn re(pattern: &str) -> Regex {
    Regex::new(&pattern.replace(r"\b", r"(?-u:\b)")).expect("classifier regex")
}

macro_rules! regex {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| re($pat));
    };
}

// ---------------------------------------------------------------- sensitive

regex!(
    PRIVATE_KEY,
    r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----"
);
regex!(
    JWT,
    r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]*"
);
regex!(
    API_KEY,
    r"(?x)
      \b(?:AKIA|ASIA)[0-9A-Z]{16}\b
    | \bgh[pousr]_[A-Za-z0-9]{36,}
    | \bgithub_pat_[A-Za-z0-9_]{22,}
    | \bglpat-[A-Za-z0-9_-]{20,}
    | \bxox[abposr]-[A-Za-z0-9-]{10,}
    | \b[sr]k_(?:live|test)_[A-Za-z0-9]{16,}
    | \bsk-(?:proj-|ant-|svcacct-|admin-)?[A-Za-z0-9_-]{32,}
    | \bAIza[0-9A-Za-z_-]{35}
    | \bnpm_[A-Za-z0-9]{36}
    | \bhf_[A-Za-z0-9]{30,}
    | \bSG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}
    "
);
regex!(CARD, r"^\d(?:[ -]?\d){12,18}$");
// 6–8 digits only: shorter numbers are far more often years, PINs-in-docs,
// counts or issue numbers than one-time codes.
regex!(OTP, r"^(?:\d{6,8}|\d{3}[ -]\d{3})$");

fn sensitive(t: &str) -> Option<&'static str> {
    if PRIVATE_KEY.is_match(t) {
        return Some("private-key");
    }
    if JWT.is_match(t) {
        return Some("jwt");
    }
    // Real keys are random: demand digits and letters in the match so
    // hyphenated words like "sk-learn-is-a-…" don't trip it.
    if API_KEY.find_iter(t).any(|m| {
        let s = m.as_str();
        s.bytes().any(|b| b.is_ascii_digit())
            && s.bytes().filter(u8::is_ascii_alphabetic).count() >= 4
    }) {
        return Some("api-key");
    }
    if CARD.is_match(t) && is_card_number(t) {
        return Some("card");
    }
    if OTP.is_match(t) {
        return Some("otp");
    }
    None
}

fn is_card_number(t: &str) -> bool {
    let digits: Vec<u32> = t.chars().filter_map(|c| c.to_digit(10)).collect();
    // Issuer prefixes start 2–6; this also rejects ms timestamps ("17…").
    let issuer_ok = matches!(digits.first(), Some(2..=6));
    let all_same = digits.windows(2).all(|w| w[0] == w[1]);
    issuer_ok && !all_same && luhn(&digits)
}

fn luhn(digits: &[u32]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

// --------------------------------------------------------------------- link

regex!(
    BARE_DOMAIN_PATH,
    r"(?i)^(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,24}(?::\d{1,5})?/\S*$"
);
regex!(
    HOST,
    r"^(?:[\p{L}\p{N}](?:[\p{L}\p{N}-]*[\p{L}\p{N}])?\.?)+$"
);

fn link(t: &str) -> Option<Classification> {
    if t.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some(rest) = strip_prefix_ci(t, "mailto:") {
        let addr = rest.split('?').next().unwrap_or_default();
        return EMAIL
            .is_match(addr)
            .then(|| class(Kind::Contact, Some("email")));
    }
    if strip_prefix_ci(t, "file:///").is_some() {
        return Some(class(Kind::File, Some("path")));
    }
    let host_part = ["https://", "http://", "ftp://"]
        .iter()
        .find_map(|scheme| strip_prefix_ci(t, scheme))
        .or_else(|| strip_prefix_ci(t, "www.").map(|_| t))
        .or_else(|| BARE_DOMAIN_PATH.is_match(t).then_some(t))?;
    host_of(host_part).map(|host| Classification {
        kind: Kind::Link,
        subkind: Some(host),
    })
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Host of `authority/path…`, lowercased and without `www.`.
fn host_of(rest: &str) -> Option<String> {
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        format!("[{}]", v6.split(']').next()?)
    } else {
        let h = host_port
            .split(':')
            .next()?
            .trim_end_matches('.')
            .to_lowercase();
        if !HOST.is_match(&h) {
            return None;
        }
        h
    };
    let host = host.strip_prefix("www.").unwrap_or(&host).to_owned();
    (!host.is_empty() && host != "[]").then_some(host)
}

// ------------------------------------------------------------------ contact

regex!(
    EMAIL,
    r"^[A-Za-z0-9._%+-]+@[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)*\.[A-Za-z]{2,}$"
);
regex!(PHONE_CHARS, r"^[+(]?[0-9][0-9 ().-]*$");
regex!(
    DATE_LIKE,
    r"^(?:\d{4}[-./]\d{1,2}[-./]\d{1,2}|\d{1,2}[-./]\d{1,2}[-./]\d{2,4})$"
);
regex!(DECIMAL, r"^\d+\.\d+$");

fn contact(t: &str) -> Option<&'static str> {
    if EMAIL.is_match(t) {
        return Some("email");
    }
    is_phone(t).then_some("phone")
}

fn is_phone(t: &str) -> bool {
    if t.len() > 32 || !PHONE_CHARS.is_match(t) || DATE_LIKE.is_match(t) || DECIMAL.is_match(t) {
        return false;
    }
    if [" - ", "--", "..", "  ", "()"]
        .iter()
        .any(|bad| t.contains(bad))
    {
        return false;
    }
    let digits = t.bytes().filter(u8::is_ascii_digit).count();
    if !(7..=15).contains(&digits) {
        return false;
    }
    let plus = t.starts_with('+');
    let separated = t
        .bytes()
        .any(|b| matches!(b, b' ' | b'(' | b')' | b'.' | b'-'));
    if !plus && !separated {
        // Bare digit runs: 10–12 digits, but not a 10-digit Unix timestamp.
        return (10..=12).contains(&digits) && !(digits == 10 && t.starts_with('1'));
    }
    if separated && !t.contains("(0)") {
        // IPs and versions (192.168.1.1, 10.0.26300) have 1-digit groups
        // after the first; phone numbers don't.
        let groups: Vec<&str> = t
            .split(|c: char| !c.is_ascii_digit())
            .filter(|g| !g.is_empty())
            .collect();
        if groups.iter().skip(1).any(|g| g.len() == 1) {
            return false;
        }
    }
    true
}

// -------------------------------------------------------------------- color

regex!(
    HEX_COLOR,
    r"^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$"
);
regex!(
    RGB_COLOR,
    r"(?i)^rgba?\(\s*[\d.]+%?(?:\s*[,\s/]\s*[\d.]+%?){2,3}\s*\)$"
);
regex!(
    HSL_COLOR,
    r"(?i)^hsla?\(\s*[\d.]+(?:deg|rad|grad|turn)?(?:\s*[,\s/]\s*[\d.]+%?){2,3}\s*\)$"
);

fn color(t: &str) -> Option<&'static str> {
    if HEX_COLOR.is_match(t) && !is_issue_ref(t) {
        Some("hex")
    } else if RGB_COLOR.is_match(t) {
        Some("rgb")
    } else if HSL_COLOR.is_match(t) {
        Some("hsl")
    } else {
        None
    }
}

/// "#123" / "#1234" with mixed digits reads as an issue or PR number;
/// all-same-digit greys like "#333" stay colors.
fn is_issue_ref(t: &str) -> bool {
    let digits = &t.as_bytes()[1..];
    digits.len() <= 4
        && digits.iter().all(u8::is_ascii_digit)
        && digits.iter().any(|&b| b != digits[0])
}

// --------------------------------------------------------------------- path

regex!(WIN_PATH, r#"^"?[A-Za-z]:[\\/][^<>"|?*\r\n]*"?$"#);
regex!(UNC_PATH, r#"^"?\\\\[^\\/\s<>"|?*]+[\\/][^<>"|?*\r\n]*"?$"#);
regex!(UNIX_PATH, r"^(?:~|\.{1,2})?/(?:[^\s\\]|\\.)+$|^~/$");
regex!(QUOTED_UNIX_PATH, r#"^"(?:~|\.{1,2})?/[^"\r\n]+"$"#);

fn is_path(t: &str) -> bool {
    if t.contains('\n') {
        return false;
    }
    if WIN_PATH.is_match(t) || UNC_PATH.is_match(t) {
        return true;
    }
    // "//" is a comment or protocol-relative URL, not a path.
    !t.starts_with("//") && (UNIX_PATH.is_match(t) || QUOTED_UNIX_PATH.is_match(t))
}

// --------------------------------------------------------------------- code

/// Some(language-or-None) if `t` looks like code.
fn code(t: &str) -> Option<Option<&'static str>> {
    if let Some(found) = fenced_block(t) {
        return found;
    }
    if is_json(t) {
        return Some(Some("json"));
    }
    if is_diff(t) {
        return Some(Some("diff"));
    }
    if let Some(lang) = markup(t) {
        return Some(Some(lang));
    }
    if is_sql(t) {
        return Some(Some("sql"));
    }
    if !t.contains('\n') {
        return single_line_code(t);
    }
    if is_yaml(t) {
        return Some(Some("yaml"));
    }
    if is_toml(t) {
        return Some(Some("toml"));
    }
    multi_line_code(t)
}

// --- structural formats

/// A Markdown fenced block. Outer None: no fences, keep looking. Some(None):
/// fences but mostly prose → Text. Some(Some(lang)): mostly a code block.
fn fenced_block(t: &str) -> Option<Option<Option<&'static str>>> {
    let is_fence = |l: &str| {
        let l = l.trim_start();
        l.starts_with("```") || l.starts_with("~~~")
    };
    let first_fence = t.lines().position(is_fence)?;
    let lines: Vec<&str> = t.lines().collect();
    let nonempty = lines.iter().filter(|l| !l.trim().is_empty()).count();
    let mut fenced = 0;
    let mut inside = false;
    let mut info = "";
    let mut body = String::new();
    for (i, &line) in lines.iter().enumerate().skip(first_fence) {
        if is_fence(line) {
            if !inside && i == first_fence {
                info = line.trim_start().trim_start_matches(['`', '~']).trim();
            }
            inside = !inside;
            fenced += 1;
        } else if inside {
            fenced += usize::from(!line.trim().is_empty());
            if body.len() < MAX_ANALYZE_CHARS {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    if fenced * 10 < nonempty * 6 {
        return Some(None);
    }
    let lang = normalize_lang(info).or_else(|| guess_language(&body));
    Some(Some(lang))
}

fn normalize_lang(info: &str) -> Option<&'static str> {
    let word = info
        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .next()?;
    Some(match word.to_ascii_lowercase().as_str() {
        "rust" | "rs" => "rust",
        "python" | "py" | "python3" => "python",
        "javascript" | "js" | "jsx" | "mjs" | "node" => "javascript",
        "typescript" | "ts" | "tsx" => "typescript",
        "json" | "jsonc" | "json5" => "json",
        "sql" | "sqlite" | "postgres" | "postgresql" | "mysql" => "sql",
        "html" | "htm" | "vue" | "svelte" => "html",
        "xml" | "svg" | "xaml" | "plist" => "xml",
        "css" | "scss" | "sass" | "less" => "css",
        "sh" | "bash" | "shell" | "zsh" | "console" | "shell-session" => "shell",
        "powershell" | "ps1" | "pwsh" | "ps" => "powershell",
        "c" | "h" => "c",
        "cpp" | "c++" | "cc" | "cxx" | "hpp" => "cpp",
        "csharp" | "cs" | "c#" => "csharp",
        "java" => "java",
        "go" | "golang" => "go",
        "kotlin" | "kt" | "kts" => "kotlin",
        "yaml" | "yml" => "yaml",
        "toml" | "ini" => "toml",
        "diff" | "patch" => "diff",
        "markdown" | "md" => "markdown",
        _ => return None,
    })
}

fn is_json(t: &str) -> bool {
    let (first, last) = (t.as_bytes()[0], t.as_bytes()[t.len() - 1]);
    let shaped = matches!((first, last), (b'{', b'}') | (b'[', b']'));
    // `[1]` is a footnote, not data: arrays need some structure.
    shaped
        && t.len() > 2
        && (first == b'{' || t.contains([',', '{', '"']))
        && serde_json::from_str::<serde::de::IgnoredAny>(t).is_ok()
}

regex!(
    DIFF_HEADER,
    r"(?m)^(?:diff --git |@@ -\d+(?:,\d+)? \+\d+(?:,\d+)? @@|--- \S.*\n\+\+\+ \S)"
);

fn is_diff(t: &str) -> bool {
    if !DIFF_HEADER.is_match(t) {
        return false;
    }
    let lines: Vec<&str> = t.lines().filter(|l| !l.is_empty()).collect();
    let diffy = lines
        .iter()
        .filter(|l| {
            l.starts_with(['+', '-', ' ', '@', '\\'])
                || l.starts_with("diff ")
                || l.starts_with("index ")
                || l.starts_with("new file")
                || l.starts_with("deleted file")
        })
        .count();
    diffy * 10 >= lines.len() * 8
}

regex!(
    HTML_TAG,
    r"(?i)<(?:!doctype\s+html|html|head|body|div|span|p|a|ul|ol|li|script|style|table|thead|tbody|tr|td|th|button|input|img|section|nav|header|footer|main|form|label|h[1-6]|br|hr|meta|link|title|select|option|textarea|template|article|aside|strong|em|b|i|code|pre|iframe|video|canvas)\b[^>]*>"
);
regex!(XML_TAG, r"<[A-Za-z_][\w:.-]*(?:\s[^<>]*)?>");
regex!(CLOSE_OR_SELF_CLOSING, r"</[A-Za-z_][\w:.-]*\s*>|/>");

/// HTML or XML documents/fragments: starts with a tag, ends with `>`.
fn markup(t: &str) -> Option<&'static str> {
    if !(t.starts_with('<') && t.ends_with('>')) {
        return None;
    }
    if t.starts_with("<?xml") {
        return Some("xml");
    }
    let tagged = CLOSE_OR_SELF_CLOSING.is_match(t)
        || head(t, 15).to_ascii_lowercase().starts_with("<!doctype");
    if !tagged || !XML_TAG.is_match(t) && !HTML_TAG.is_match(t) {
        return None;
    }
    if HTML_TAG.is_match(t) {
        Some("html")
    } else {
        Some("xml")
    }
}

regex!(
    SQL_STATEMENT,
    r#"(?isx)^(?:
        insert\s+(?:or\s+\w+\s+)?into\s+[\w."`\[\]]+\s*(?:\(|values|select|default)
      | update\s+[\w."`\[\]]+\s+set\s+[\w."`\[\]]+\s*=
      | delete\s+from\s+[\w."`\[\]]+\s*(?:;|$|where\b)
      | (?:create|alter|drop)\s+(?:or\s+replace\s+)?(?:temp(?:orary)?\s+)?(?:unique\s+)?
          (?:table|index|view|trigger|database|schema|function|procedure)\s+
          (?:if\s+(?:not\s+)?exists\s+)?[\w."`\[\]]+
      | with\s+(?:recursive\s+)?\w+(?:\s*\([^)]*\))?\s+as\s*\(
      | truncate\s+table\s+\S
    )"#
);
regex!(
    SQL_SELECT,
    r#"(?is)^select\s+(?:distinct\s+|top\s+\d+\s+)?(.+?)\s+from\s+[\w."`\[\]]+"#
);
regex!(
    SQL_COLUMN,
    r#"(?i)^(?:\*|[\w."`\[\]]+(?:\.\*)?(?:\s+as\s+\w+)?|\w+\s*\(.*\)(?:\s+(?:as\s+)?\w+)?|'[^']*'(?:\s+as\s+\w+)?|\d+)$"#
);
regex!(
    SQL_EXTRA,
    r"(?i)\b(?:where|join|group\s+by|order\s+by|limit|having|union)\b|[*=;]"
);

fn is_sql(t: &str) -> bool {
    if SQL_STATEMENT.is_match(t) {
        return true;
    }
    let Some(caps) = SQL_SELECT.captures(t) else {
        return false;
    };
    if t.starts_with("SELECT") {
        return true;
    }
    // Lower/Title case "select … from" is often English: demand a real
    // column list plus some other SQL signal, and no sentence ending.
    let cols_ok = caps[1].split(',').all(|c| SQL_COLUMN.is_match(c.trim()));
    cols_ok && SQL_EXTRA.is_match(t) && !t.ends_with('.')
}

regex!(YAML_KV, r#"^(\s*)(?:- )?([\w.\-/"']+):(?:\s+\S.*)?$"#);
regex!(YAML_ITEM, r"^\s*- \S");

fn is_yaml(t: &str) -> bool {
    let (mut kv, mut nested, mut lower, mut doc_marker, mut n, mut other) = (0, 0, 0, false, 0, 0);
    for line in t
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
    {
        if line.trim_start().starts_with('#') {
            continue;
        }
        n += 1;
        if line == "---" {
            doc_marker = true;
        } else if let Some(c) = YAML_KV.captures(line) {
            kv += 1;
            nested += usize::from(!c[1].is_empty() || line.trim_start().starts_with("- "));
            lower += usize::from(c[2].starts_with(|ch: char| ch.is_ascii_lowercase()));
        } else if !YAML_ITEM.is_match(line) {
            other += 1;
        }
    }
    n >= 2 && kv >= 2 && other * 5 <= n && (nested >= 1 || doc_marker) && lower * 2 >= kv
}

regex!(TOML_TABLE, r#"^\[\[?[\w.\-" ]+\]\]?$"#);
regex!(TOML_KV, r#"^[\w.\-"]+\s*=\s*\S"#);

fn is_toml(t: &str) -> bool {
    let (mut tables, mut kv, mut n) = (0, 0, 0);
    for line in t
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        n += 1;
        if TOML_TABLE.is_match(line) {
            tables += 1;
        } else if TOML_KV.is_match(line) {
            kv += 1;
        }
    }
    tables >= 1 && kv >= 1 && (tables + kv) * 10 >= n * 7
}

// --- single line

regex!(
    CMDLET,
    r"(?i)^(?:get|set|new|remove|add|start|stop|invoke|import|export|test|write|out|select|where|foreach|copy|move|rename|install|uninstall|update|enable|disable|restart|register|unregister|convertto|convertfrom|format|measure|sort|group|resolve|wait|read|clear|join|split|push|pop|enter|exit|expand|compress|show|find|save|send|connect|disconnect|use|grant|revoke|reset|initialize|search|edit|compare|switch)-[a-z][a-z0-9]{2,}\b"
);
regex!(
    CALL_EXPR,
    r"^[A-Za-z_$][\w$]*(?:(?:\.|::|->)[A-Za-z_$][\w$]*)*(?:<[^<>]*>)?\(.*\)[;,]?$"
);
regex!(
    IMPORT_LINE,
    r#"^(?:import\s+[\w.]+(?:\s+as\s+\w+)?(?:\s*,\s*[\w.]+(?:\s+as\s+\w+)?)*;?|from\s+[\w.]+\s+import\s+(?:\*|\(?[\w, ]+\)?)|import\s+(?:\{[^}]*\}|\*\s+as\s+\w+|\w+)\s+from\s+['"].+['"];?|import\s+['"].+['"];?|#include\s*[<"].+[>"]|use\s+[\w:{}, *]+;|using\s+[\w.]+;|package\s+[\w.]+;?|(?:const|let|var)\s+\w+\s*=\s*require\(.+\);?)$"#
);
regex!(
    DECLARATION,
    r"^(?:(?:pub(?:\([\w:]+\))?|export|default|async|static|public|private|protected|internal|final|abstract|override|unsafe|extern|inline|const|virtual)\s+)*(?:fn|def|func|fun|function\*?|class|struct|enum|trait|impl|interface|let|const|var|val|type|mod|namespace)\s+\S.*(?:[;{:)=]|=>.*|\})$"
);
regex!(
    VAR_DECL,
    r"^(?:export\s+)?(?:const|let|var|val|auto)\s+(?:mut\s+)?[\w$]+(?:\s*:\s*[^=]+)?\s*=\s*\S"
);
regex!(
    SHELL_ONELINER,
    r"^(?:for|while|until|if)\s.+;\s*(?:do|then)\s.+;\s*(?:done|fi)$"
);
regex!(
    PY_ONELINER,
    r"^(?:for\s+\w+(?:,\s*\w+)*\s+in|if|elif|while|with)\s+[^:]+:\s*\S.*\)$"
);
regex!(
    ASSIGNMENT,
    r#"^[A-Za-z_$][\w$.\[\]'"]*\s*(?::=|[+\-*/%|&]?=)\s*(?:[-+]?\d[\d_.]*|["'`{\[(]|\w+\(|new\s|true|false|null|None|nil).*$"#
);

fn single_line_code(t: &str) -> Option<Option<&'static str>> {
    if let Some(lang) = command_line(t) {
        return Some(Some(lang));
    }
    if SHELL_ONELINER.is_match(t) {
        return Some(Some("shell"));
    }
    if IMPORT_LINE.is_match(t)
        || VAR_DECL.is_match(t) && !mentions_stopword(t)
        || PY_ONELINER.is_match(t) && CALL.is_match(t)
        || DECLARATION.is_match(t) && !mentions_stopword(t)
        || CALL_EXPR.is_match(t)
        || ASSIGNMENT.is_match(t) && !has_sentence_shape(t)
        || t.contains("=>") && t.contains('(') && !has_sentence_shape(t)
        || line_score(t) >= 4 && !is_prose_line(t)
    {
        return Some(guess_language(t));
    }
    None
}

/// A whole line that is a shell or PowerShell command.
fn command_line(line: &str) -> Option<&'static str> {
    let l = line.trim();
    if let Some(rest) = l.strip_prefix("$ ") {
        // "$ 5.00" is a price, not a prompt.
        return (!rest.starts_with(|c: char| c.is_ascii_digit())).then_some("shell");
    }
    if l.starts_with("PS ")
        && l.contains('>')
        && l.split('>').next().is_some_and(|p| p.contains(':'))
    {
        return Some("powershell");
    }
    if l.starts_with('$') && (l.contains(" = ") || l.starts_with("$env:")) && !has_sentence_shape(l)
    {
        return Some("powershell");
    }
    if CMDLET.is_match(l) && !mentions_stopword(l) {
        return Some("powershell");
    }
    is_shell_command(l).then_some("shell")
}

/// Commands whose names are rarely English words.
const SHELL_COMMANDS: &[&str] = &[
    "git",
    "cargo",
    "npm",
    "npx",
    "pnpm",
    "yarn",
    "docker",
    "docker-compose",
    "kubectl",
    "helm",
    "sudo",
    "apt",
    "apt-get",
    "brew",
    "curl",
    "wget",
    "ssh",
    "scp",
    "rsync",
    "rustup",
    "rustc",
    "winget",
    "choco",
    "scoop",
    "pip",
    "pip3",
    "python",
    "python3",
    "py",
    "node",
    "deno",
    "bun",
    "mkdir",
    "chmod",
    "chown",
    "tar",
    "unzip",
    "systemctl",
    "journalctl",
    "gh",
    "terraform",
    "ffmpeg",
    "adb",
    "gradle",
    "./gradlew",
    "gradlew",
    "mvn",
    "dotnet",
    "conda",
    "poetry",
    "uv",
    "bash",
    "zsh",
    "nvm",
    "javac",
    "gcc",
    "g++",
    "clang",
    "cmake",
    "psql",
    "mysql",
    "sqlite3",
    "redis-cli",
    "aws",
    "gcloud",
    "az",
    "ping",
    "nslookup",
    "ipconfig",
    "ifconfig",
    "netstat",
    "setx",
    "robocopy",
    "xcopy",
    "grep",
    "rg",
    "sed",
    "awk",
    "ls",
    "pwd",
    "chcp",
    "wsl",
    "tauri",
    "vite",
    "pytest",
    "jest",
    "flutter",
    "swift",
    "xcodebuild",
    "kill",
    "pkill",
    "ps",
    "du",
    "df",
    "tail",
    "head",
    "less",
    "vim",
    "nano",
    "code",
    "openssl",
    "ssh-keygen",
    "export",
    "source",
    "echo",
    "cd",
    "rm",
    "cp",
    "mv",
    "cat",
    "touch",
    "find",
    "make",
    "go",
    "java",
    "dir",
    "del",
    "type",
    "set",
    "where",
    "sh",
    "env",
    "watch",
];
/// Of the above, the ones that are also everyday English words: these need
/// a clearly code-like argument (flag, path, pipe…) to count.
const AMBIGUOUS_COMMANDS: &[&str] = &[
    "export", "source", "echo", "cd", "rm", "cp", "mv", "cat", "touch", "find", "make", "go",
    "java", "dir", "del", "type", "set", "where", "sh", "env", "watch", "kill", "head", "tail",
    "less", "code", "ping", "python", "py", "node", "swift", "ls", "ps", "du", "df",
];
const GO_SUBCOMMANDS: &[&str] = &[
    "build", "run", "test", "get", "mod", "install", "fmt", "vet", "version", "env", "generate",
    "work", "clean", "tool",
];
const MAKE_TARGETS: &[&str] = &[
    "install", "build", "test", "clean", "all", "release", "debug", "run", "check", "lint", "dev",
    "docs", "dist", "deploy",
];
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "to", "of", "and", "or", "but",
    "on", "at", "for", "with", "you", "your", "i", "we", "it", "its", "this", "that", "my", "our",
    "they", "he", "she", "will", "can", "should", "would", "could", "have", "has", "had", "not",
    "so", "if", "as", "by", "about", "what", "how", "why", "when", "sure", "me", "us", "them",
    "his", "her", "their", "there", "here", "do", "does", "did", "just", "please", "really",
    "very", "out", "into",
];

fn is_shell_command(l: &str) -> bool {
    let mut words = l.split_whitespace();
    let Some(cmd) = words.next() else {
        return false;
    };
    let Some(arg) = words.next() else {
        return false;
    };
    if !SHELL_COMMANDS.contains(&cmd) || STOPWORDS.contains(&arg.to_ascii_lowercase().as_str()) {
        return false;
    }
    // A sentence ending ("git is great.") — but "git add ." is fine.
    if l.ends_with('.') && !l.ends_with(" .") && !l.ends_with("..") {
        return false;
    }
    let codey = has_code_token(l);
    if AMBIGUOUS_COMMANDS.contains(&cmd) {
        return codey
            || cmd == "go" && GO_SUBCOMMANDS.contains(&arg)
            || cmd == "make" && MAKE_TARGETS.contains(&arg) && l.split_whitespace().count() <= 3;
    }
    codey || !mentions_stopword(l)
}

/// A flag, path, pipe, redirect, variable or quoted string.
fn has_code_token(l: &str) -> bool {
    l.split_whitespace().skip(1).any(|w| {
        (w.starts_with('-') && w.len() > 1 && w != "--" && !w.starts_with("- "))
            || w.contains([
                '/', '\\', '=', '$', '|', '>', '<', '*', '~', '"', '\'', '`', '@',
            ])
            || w == "&&"
            || w == "."
            || w.contains('.') && !w.ends_with('.')
    })
}

/// Does the line (quoted strings removed) contain an English stopword?
fn mentions_stopword(l: &str) -> bool {
    let mut out = String::with_capacity(l.len());
    let mut quote: Option<char> = None;
    for c in l.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None => out.push(c),
        }
    }
    out.split_whitespace().skip(1).any(|w| {
        let w = w
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_ascii_lowercase();
        STOPWORDS.contains(&w.as_str())
    })
}

/// Reads like a sentence: several words with stopwords and little syntax.
fn has_sentence_shape(l: &str) -> bool {
    l.split_whitespace().count() >= 5 && mentions_stopword(l) && symbol_ratio(l) < 0.08
}

// --- multi line

regex!(
    KEYWORD_LINE,
    r#"(?x)^(?:
        (?:pub(?:\([\w:]+\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:fn|struct|enum|trait|impl|mod)\s+\S
      | use\s+[\w:]+(?:::|;)
      | (?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function\*?|class|interface)\s+[\w$]
      | (?:const|let|var|val)\s+(?:mut\s+)?[\w${\[]+.*(?:=|;$)
      | def\s+\w+\s*\(
      | class\s+\w+.*[:{]$
      | import\s+[\w{*"']
      | from\s+[\w.]+\s+import\s
      | package\s+[\w.]+;?$
      | using\s+[\w.]+;$
      | (?:public|private|protected|internal|static|final|override|abstract|virtual)\s+\w
      | \#(?:include|define|ifdef|ifndef|endif|pragma|!)
      | return\b.*;$ | return$ | return\s+\S+$
      | (?:if|while|for|switch|catch|foreach)\s*\(
      | (?:else|try|finally|do)\s*\{
      | \}\s*(?:else|catch|finally|while)\b
      | (?:if|elif|while|with)\s+[^,]+:$
      | for\s+\w+(?:,\s*\w+)*\s+in\s+.+:$
      | (?:else|try|finally):$
      | except\b.*:$
      | (?:func|fun)\s+[\w(]
      | (?:SELECT|FROM|WHERE|JOIN|LEFT\sJOIN|INNER\sJOIN|GROUP\sBY|ORDER\sBY|INSERT\sINTO|VALUES|UPDATE|SET|DELETE\sFROM|CREATE|ALTER|DROP|HAVING|LIMIT|UNION)\b
      | (?:echo|export|alias)\s+\S
      | (?:fi|done|esac|then|do|end)$
      | </?[a-zA-Z][\w.-]*(?:\s[^>]*)?/?>
      | @(?:Override|Test|Autowired|Composable|Deprecated|property|staticmethod|classmethod|dataclass)\b
    )"#
);
regex!(
    OPERATOR,
    r"=>|->|::|:=|==|!=|&&|\|\||\+=|-=|\+\+|<=|>=|<<|>>"
);
regex!(CALL, r"[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*\([^()]*\)");
regex!(
    ASSIGN_STMT,
    r#"^[A-Za-z_$][\w$.\[\]'"]*\s*(?::=|[+\-*/%|&]?=)\s*[^=\s]"#
);

/// 0 = no code signal; 2+ = this line on its own looks like code.
fn line_score(line: &str) -> u32 {
    let l = line.trim();
    let mut score = 0;
    if l.ends_with(';') && l.len() > 1 {
        score += 2;
    }
    if l.ends_with('{')
        || l.starts_with('}')
        || l.ends_with('}') && l.contains('{')
        || matches!(l, ")" | "]" | ");" | "];" | "})" | "});" | "]," | "),")
    {
        score += 2;
    }
    if KEYWORD_LINE.is_match(l) {
        score += 2;
    }
    if command_line(l).is_some() {
        score += 2;
    }
    if OPERATOR.is_match(l) {
        score += 1;
    }
    if CALL.is_match(l) {
        score += 1;
        if l.ends_with(')') {
            score += 1;
        }
    }
    if ASSIGN_STMT.is_match(l) {
        score += 1;
    }
    score
}

fn is_comment_line(l: &str) -> bool {
    let l = l.trim_start();
    (l.starts_with('#')
        && !l.starts_with("#include")
        && !l.starts_with("#define")
        && !l.starts_with("#!"))
        || l.starts_with("//")
        || l.starts_with("/*")
        || l.starts_with("* ")
        || l == "*"
        || l.starts_with("*/")
        || l.starts_with("-- ")
}

/// Natural-language line: mostly real words, little syntax.
fn is_prose_line(line: &str) -> bool {
    let words: Vec<&str> = line.split_whitespace().collect();
    if words.len() < 4 {
        return false;
    }
    let wordy = words
        .iter()
        .filter(|w| {
            let core = w.trim_matches(|c: char| ",.;:!?\"'()[]“”‘’…".contains(c));
            !core.is_empty()
                && core
                    .chars()
                    .all(|c| c.is_alphabetic() || c == '\'' || c == '-' || c == '’')
        })
        .count();
    wordy * 10 >= words.len() * 7 && symbol_ratio(line) < 0.08
}

/// Share of non-space chars that are code punctuation.
fn symbol_ratio(s: &str) -> f32 {
    let (mut sym, mut total) = (0u32, 0u32);
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        sym += u32::from("{}[]()<>;=*&|/\\$#@_:+-%^!~`\"".contains(c));
    }
    if total == 0 {
        0.0
    } else {
        sym as f32 / total as f32
    }
}

fn multi_line_code(t: &str) -> Option<Option<&'static str>> {
    let (mut n, mut code, mut prose, mut indented, mut comments) = (0u32, 0f32, 0u32, 0u32, 0u32);
    let mut brackets = 0u32;
    let mut chars = 0u32;
    for line in t.lines().filter(|l| !l.trim().is_empty()).take(500) {
        if is_comment_line(line) {
            comments += 1;
            continue;
        }
        n += 1;
        let score = line_score(line);
        if score >= 2 {
            code += 1.0;
        } else if is_prose_line(line) || is_list_item(line) {
            prose += 1;
        } else if score == 1 {
            code += 0.5;
        }
        indented += u32::from(line.starts_with([' ', '\t']));
        for c in line.chars().filter(|c| !c.is_whitespace()) {
            chars += 1;
            brackets += u32::from("{}[]();".contains(c));
        }
    }
    if n == 0 {
        // Only comments: code if they're code-style comments, not "# headings".
        let slashes = t
            .lines()
            .filter(|l| l.trim_start().starts_with("//"))
            .count() as u32;
        return (comments > 0 && slashes == comments).then(|| guess_language(t));
    }
    let nf = n as f32;
    let (cr, pr) = (code / nf, prose as f32 / nf);
    let density = brackets as f32 / chars.max(1) as f32;
    let scores = language_scores(t);
    let lang = pick_language(&scores);
    let fingerprint = scores.first().map_or(0, |s| s.1);
    let is_code = (cr >= 0.5 && pr <= 0.34)
        || (cr >= 0.3 && pr <= 0.2 && (indented * 3 >= n || density >= 0.04 || fingerprint >= 4))
        || (fingerprint >= 6 && cr >= 0.2 && pr <= 0.3);
    is_code.then_some(lang)
}

regex!(LIST_ITEM, r"^\s*(?:[-*•‣◦]|\d{1,3}[.)]|[a-z][.)])\s+\S");

fn is_list_item(l: &str) -> bool {
    LIST_ITEM.is_match(l) && line_score(l) < 2
}

// --- language guess

/// (language, regex, weight). Each pattern counts once.
const FINGERPRINTS: &[(&str, &str, u32)] = &[
    // Rust
    ("rust", r"\bfn\s+\w+\s*(?:<[^>]*>)?\s*\(", 3),
    ("rust", r"\blet\s+mut\b", 3),
    ("rust", r"\bimpl\b(?:\s*<[^>]*>)?\s+[\w:]+", 2),
    (
        "rust",
        r"\bpub(?:\(crate\))?\s+(?:fn|struct|enum|mod|use|trait|const)\b",
        3,
    ),
    ("rust", r"\buse\s+(?:std|crate|super|self)::", 3),
    ("rust", r"#\[(?:derive|cfg|test|allow|tokio)", 3),
    (
        "rust",
        r"\b(?:println|format|vec|eprintln|panic|assert_eq|assert|write|writeln)!\(",
        3,
    ),
    ("rust", r"\.unwrap\(\)|\)\?;", 1),
    ("rust", r"&(?:mut\s+)?self\b|&'\w+|&str\b", 2),
    ("rust", r"\b(?:Option|Result|Vec|Box|Arc|Rc|HashMap)<", 1),
    // Python
    (
        "python",
        r"(?m)^\s*def\s+\w+\s*\(.*\)\s*(?:->\s*[^:]+)?:\s*$",
        3,
    ),
    (
        "python",
        r"(?m)^\s*(?:from\s+[\w.]+\s+import\s+|import\s+\w+(?:\s+as\s+\w+)?\s*$)",
        3,
    ),
    (
        "python",
        r"(?m)^\s*(?:elif\s+.*|else|try|finally|except(?:\s+.*)?)\s*:\s*$",
        2,
    ),
    ("python", r"(?m)^\s*class\s+\w+(?:\(.*\))?:\s*$", 3),
    ("python", r"\bself\.\w+", 1),
    ("python", r"\bprint\(", 1),
    ("python", r"\b(?:None|True|False)\b", 1),
    ("python", r"__\w+__", 2),
    ("python", r"(?m)^\s*for\s+\w+(?:,\s*\w+)*\s+in\s+.+:\s*$", 2),
    ("python", r"\blambda\s+\w*:", 2),
    (
        "python",
        r"\bin\s+(?:range|enumerate|zip)\(|\.items\(\)|\blen\(",
        2,
    ),
    // JavaScript (TypeScript adds its own markers on top of these)
    ("javascript", r"\b(?:const|var)\s+[\w${}\[\], ]+\s*=", 2),
    ("javascript", r"\blet\s+\w+\s*=", 1),
    ("javascript", r"\bfunction\s*\*?\s*[\w$]*\s*\(", 3),
    ("javascript", r"=>\s*[{(\w`'\x22]", 2),
    ("javascript", r"\bconsole\.(?:log|error|warn|info)\(", 3),
    ("javascript", r"\brequire\(['\x22]", 3),
    ("javascript", r"\bmodule\.exports\b|\bexports\.\w+", 3),
    ("javascript", r"\bdocument\.\w|\bwindow\.\w", 2),
    ("javascript", r"===|!==", 2),
    ("javascript", r"(?m)^\s*import\s+.*\s+from\s+['\x22]", 3),
    (
        "javascript",
        r"(?m)^\s*export\s+(?:default|const|function|class|async|let)\b",
        3,
    ),
    (
        "javascript",
        r"\.then\(|\.addEventListener\(|\bJSON\.(?:parse|stringify)\(",
        2,
    ),
    (
        "typescript",
        r":\s*(?:string|number|boolean|any|void|unknown|never)(?:\[\])?\s*[,;)=|{]",
        3,
    ),
    (
        "typescript",
        r"(?m)^\s*(?:export\s+)?interface\s+\w+\s*(?:<[^>]*>)?\s*(?:extends\s+[\w, <>]+)?\{",
        3,
    ),
    (
        "typescript",
        r"(?m)^\s*(?:export\s+)?type\s+\w+(?:<[^>]*>)?\s*=",
        3,
    ),
    (
        "typescript",
        r"\bas\s+const\b|\breadonly\s+\w+\s*:|\bimplements\s+\w",
        2,
    ),
    // Go
    ("go", r"(?m)^package\s+\w+\s*$", 3),
    ("go", r"\bfunc\s+(?:\(\w+\s+\*?\w+\)\s*)?\w+\(", 3),
    ("go", r":=", 2),
    ("go", r"\bfmt\.\w+\(", 3),
    ("go", r"\bif\s+err\s*!=\s*nil", 3),
    (
        "go",
        r"\bgo\s+func\b|\bdefer\s+\w|\bchan\s+\w|\bmake\(\[\]",
        2,
    ),
    ("go", r"(?m)^import\s+\(", 3),
    // Java
    (
        "java",
        r"\bpublic\s+(?:static\s+)?(?:final\s+)?(?:class|interface|enum|void)\b",
        2,
    ),
    ("java", r"\bpublic\s+static\s+void\s+main\s*\(\s*String", 4),
    ("java", r"\bSystem\.out\.print(?:ln)?\(", 4),
    ("java", r"(?m)^import\s+javax?\.[\w.*]+;", 4),
    ("java", r"(?m)^package\s+[\w.]+;", 3),
    ("java", r"@Override\b|@Autowired\b", 2),
    ("java", r"\bnew\s+[A-Z]\w*(?:<[^>]*>)?\(", 1),
    ("java", r"\bString\[\]", 1),
    // Kotlin
    ("kotlin", r"\bfun\s+(?:<[^>]*>\s*)?[\w.]+\s*\(", 4),
    ("kotlin", r"\bval\s+\w+(?:\s*:\s*[\w<>?]+)?\s*=", 3),
    ("kotlin", r"\b(?:data|sealed|open)\s+class\b", 3),
    ("kotlin", r"\bcompanion\s+object\b|\boverride\s+fun\b", 3),
    ("kotlin", r"\bwhen\s*\([^)]*\)\s*\{", 2),
    (
        "kotlin",
        r"(?m)^import\s+(?:kotlin|kotlinx|android|androidx)\.",
        3,
    ),
    // C#
    ("csharp", r"(?m)^using\s+System(?:\.[\w.]+)?;", 4),
    ("csharp", r"\bConsole\.(?:WriteLine|Write|ReadLine)\(", 4),
    (
        "csharp",
        r"\{\s*get;\s*(?:(?:private\s+)?set;|init;)?\s*\}",
        4,
    ),
    ("csharp", r"\basync\s+Task\b|\bTask<", 3),
    (
        "csharp",
        r"\b(?:public|private|internal|protected)\s+(?:static\s+)?(?:async\s+)?(?:void|string|int|bool|Task)\s+[A-Z]\w*\s*\(",
        2,
    ),
    ("csharp", r"(?m)^\s*namespace\s+[\w.]+\s*;?\s*$", 2),
    ("csharp", r"\bstring\[\]|\bstring\s+\w+\s*[=;),]", 2),
    // C
    (
        "c",
        r"#include\s*<(?:stdio|stdlib|string|unistd|math|stdint|stdbool|errno|time)\.h>",
        4,
    ),
    ("c", r"\bint\s+main\s*\(", 3),
    (
        "c",
        r"\bprintf\s*\(|\bscanf\s*\(|\bmalloc\s*\(|\bfree\s*\(",
        2,
    ),
    ("c", r"(?m)^#\s*(?:define|ifdef|ifndef|endif|include)\b", 2),
    (
        "c",
        r"\b(?:char|int|void|unsigned|struct\s+\w+)\s*\*+\s*\w+",
        2,
    ),
    ("c", r"\bsizeof\s*\(|\bNULL\b", 1),
    // C++
    (
        "cpp",
        r"#include\s*<(?:iostream|vector|string|map|memory|algorithm|unordered_map|cstdio|cstdlib)>",
        4,
    ),
    ("cpp", r"\bstd::", 3),
    ("cpp", r"\b(?:cout|cerr)\s*<<", 3),
    ("cpp", r"\btemplate\s*<", 3),
    ("cpp", r"\bnullptr\b", 2),
    ("cpp", r"(?m)^\s*(?:public|private|protected):\s*$", 2),
    ("cpp", r"\busing\s+namespace\s+std\b", 4),
    ("cpp", r"\bint\s+main\s*\(", 2),
    // SQL
    ("sql", r"(?is)\bselect\b.+?\bfrom\b", 2),
    (
        "sql",
        r"\b(?:SELECT|INSERT INTO|DELETE FROM|CREATE TABLE|ALTER TABLE|DROP TABLE)\b",
        3,
    ),
    (
        "sql",
        r"\b(?:WHERE|JOIN|GROUP BY|ORDER BY|VALUES|PRIMARY KEY|NOT NULL)\b",
        2,
    ),
    ("sql", r"(?i)\b(?:inner|left|right)\s+join\b", 2),
    // Shell
    (
        "shell",
        r"(?m)^#!\s*/(?:usr/)?bin/(?:env\s+)?(?:ba|z|da)?sh\b",
        6,
    ),
    ("shell", r"(?m)^\s*(?:fi|done|esac|then|do)\s*$", 3),
    ("shell", r"(?m)^\s*(?:if|while)\s+\[\[?\s", 3),
    ("shell", r"(?m)^\s*export\s+[A-Za-z_]\w*=", 3),
    ("shell", r"(?m)^\s*alias\s+[\w-]+=", 3),
    (
        "shell",
        r"(?m)^\s*(?:sudo|apt(?:-get)?|brew|curl|wget|chmod|chown|mkdir|echo|cd|git|npm|cargo|docker|pip3?|kubectl|rm|cp|mv)\s+[-\w./~$\x22']",
        3,
    ),
    (
        "shell",
        r"\|\s*(?:grep|awk|sed|xargs|sort|uniq|head|tail|wc|tee|cut)\b",
        3,
    ),
    ("shell", r"\$\{\w+[^}]*\}|\$\(\w", 1),
    ("shell", r"(?m)^\$ \S", 3),
    // PowerShell
    (
        "powershell",
        r"\b(?:Get|Set|New|Remove|Add|Start|Stop|Invoke|Import|Export|Test|Write|Out|Select|Where|ForEach|Copy|Move|Rename|Install|Update|Enable|Disable|Restart|Register|ConvertTo|ConvertFrom|Format|Measure|Sort|Group|Resolve|Wait|Read|Clear|Join|Split)-[A-Z][a-z]+[A-Za-z]*\b",
        3,
    ),
    (
        "powershell",
        r"\$(?:env:|_\.|PSScriptRoot\b|PSVersionTable\b|true\b|false\b|null\b)",
        3,
    ),
    (
        "powershell",
        r"\s-(?:eq|ne|gt|lt|ge|le|like|notlike|match|contains|and|or|not)\s",
        3,
    ),
    (
        "powershell",
        r"\[(?:string|int|bool|switch|PSCustomObject|array|hashtable|datetime)\]",
        3,
    ),
    ("powershell", r"(?mi)^\s*param\s*\(", 3),
    ("powershell", r"@\{", 1),
    ("powershell", r"(?m)^PS [A-Z]:\\[^>]*>", 4),
    // Markup and styles (when mixed into other text)
    ("html", r"(?i)<!doctype\s+html", 5),
    (
        "html",
        r"(?i)</?(?:div|span|ul|li|body|html|head|script|style|table|button|input|img|section|nav|form)\b[^>]*>",
        3,
    ),
    ("xml", r"<\?xml\b", 5),
    (
        "css",
        r"(?m)^\s*(?:color|background(?:-color)?|margin|padding|display|font-(?:size|family|weight)|border(?:-radius)?|width|height|position|flex|grid-template-columns|z-index|opacity|transition|transform|justify-content|align-items|box-shadow|text-align|overflow|cursor)\s*:\s*[^;]+;",
        3,
    ),
    (
        "css",
        r"@media\b|@import\s+url|@keyframes\b|:hover\b|!important\b",
        3,
    ),
    (
        "css",
        r"(?m)^\s*[.#]?[\w-]+(?:[\s>+~,.#:]+[\w-]+)*\s*\{\s*$",
        1,
    ),
    ("css", r"\b\d+(?:px|rem|em|vh|vw)\b", 1),
];

static FINGERPRINT_RES: LazyLock<Vec<(&'static str, Regex, u32)>> = LazyLock::new(|| {
    FINGERPRINTS
        .iter()
        .map(|&(lang, pat, w)| (lang, re(pat), w))
        .collect()
});

/// Fingerprint score per language, highest first.
fn language_scores(t: &str) -> Vec<(&'static str, u32)> {
    let mut scores: Vec<(&'static str, u32)> = Vec::new();
    for (lang, regex, weight) in FINGERPRINT_RES.iter() {
        if regex.is_match(t) {
            match scores.iter_mut().find(|s| s.0 == *lang) {
                Some(s) => s.1 += weight,
                None => scores.push((lang, *weight)),
            }
        }
    }
    // TypeScript is JavaScript plus type syntax.
    let js = scores
        .iter()
        .find(|s| s.0 == "javascript")
        .map_or(0, |s| s.1);
    if let Some(ts) = scores.iter_mut().find(|s| s.0 == "typescript") {
        ts.1 += js;
    }
    scores.sort_by_key(|s| std::cmp::Reverse(s.1));
    scores
}

/// Best language guess, or None when nothing is clearly ahead.
fn guess_language(t: &str) -> Option<&'static str> {
    pick_language(&language_scores(t))
}

fn pick_language(scores: &[(&'static str, u32)]) -> Option<&'static str> {
    let (best, top) = *scores.first()?;
    let second = scores.get(1).map_or(0, |s| s.1);
    (top >= 3 && top > second).then_some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (input, kind, subkind)
    const SAMPLES: &[(&str, Kind, Option<&str>)] = &[
        // --- sensitive
        (
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA7\n-----END RSA PRIVATE KEY-----",
            Kind::Sensitive,
            Some("private-key"),
        ),
        (
            "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----",
            Kind::Sensitive,
            Some("private-key"),
        ),
        (
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
            Kind::Sensitive,
            Some("jwt"),
        ),
        (
            "Authorization: Bearer eyJhbGciOiJSUzI1NiJ9.eyJpc3MiOiJodHRwczovL2V4YW1wbGUifQ.abcDEF123_-xyz",
            Kind::Sensitive,
            Some("jwt"),
        ),
        ("AKIAIOSFODNN7EXAMPLE", Kind::Sensitive, Some("api-key")),
        (
            "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8",
            Kind::Sensitive,
            Some("api-key"),
        ),
        (
            "github_pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz",
            Kind::Sensitive,
            Some("api-key"),
        ),
        (
            // Fake tokens are split so secret scanners don't flag the source.
            concat!(
                "xox",
                "b-123456789012-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx"
            ),
            Kind::Sensitive,
            Some("api-key"),
        ),
        (
            concat!("sk_", "live_51H8abcDEF123ghiJKL456mno"),
            Kind::Sensitive,
            Some("api-key"),
        ),
        (
            "sk-proj-Ab3dEf6hIj9kLm2nOp5qRs8tUv1wXy4zAb7cDe0fGh3",
            Kind::Sensitive,
            Some("api-key"),
        ),
        (
            "sk-ant-api03-AbCdEf123456GhIjKl789012MnOpQr345678StUvWx",
            Kind::Sensitive,
            Some("api-key"),
        ),
        (
            "AIzaSyD-9tSrke72PouQMnMX-a7eZSW0jkFMBWY",
            Kind::Sensitive,
            Some("api-key"),
        ),
        (
            "OPENAI_API_KEY=sk-Ab3dEf6hIj9kLm2nOp5qRs8tUv1wXy4zAb7cDe0f\nDEBUG=true",
            Kind::Sensitive,
            Some("api-key"),
        ),
        ("4111 1111 1111 1111", Kind::Sensitive, Some("card")),
        ("5500-0000-0000-0004", Kind::Sensitive, Some("card")),
        ("378282246310005", Kind::Sensitive, Some("card")),
        ("482913", Kind::Sensitive, Some("otp")),
        ("1234", Kind::Text, None),
        ("2023", Kind::Text, None),
        ("90210", Kind::Text, None),
        ("123 456", Kind::Sensitive, Some("otp")),
        // --- not sensitive
        ("4111 1111 1111 1112", Kind::Text, None),
        ("1718452800000", Kind::Text, None),
        ("123", Kind::Text, None),
        (
            "scikit-learn is also known as sk-learn-library-for-machine-learning",
            Kind::Text,
            None,
        ),
        // --- links
        (
            "https://github.com/rust-lang/rust/issues/1234",
            Kind::Link,
            Some("github.com"),
        ),
        (
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s",
            Kind::Link,
            Some("youtube.com"),
        ),
        (
            "http://localhost:3000/api/v1/users",
            Kind::Link,
            Some("localhost"),
        ),
        (
            "www.example.org/docs/intro",
            Kind::Link,
            Some("example.org"),
        ),
        ("docs.rs/regex/latest/regex/", Kind::Link, Some("docs.rs")),
        (
            "ftp://files.example.com/pub/file.tar.gz",
            Kind::Link,
            Some("files.example.com"),
        ),
        (
            "HTTPS://User:pw@Sub.Example.COM:8443/x",
            Kind::Link,
            Some("sub.example.com"),
        ),
        (
            "Check out https://github.com/rust-lang/rust when you get a chance",
            Kind::Text,
            None,
        ),
        ("example.com", Kind::Text, None),
        ("https://", Kind::Text, None),
        // --- contacts
        ("john.doe+news@example.co.uk", Kind::Contact, Some("email")),
        (
            "mailto:support@example.com?subject=Hi",
            Kind::Contact,
            Some("email"),
        ),
        ("+1 (555) 123-4567", Kind::Contact, Some("phone")),
        ("+919876543210", Kind::Contact, Some("phone")),
        ("(555) 123-4567", Kind::Contact, Some("phone")),
        ("+44 20 7946 0958", Kind::Contact, Some("phone")),
        ("555.123.4567", Kind::Contact, Some("phone")),
        ("9876543210", Kind::Contact, Some("phone")),
        ("192.168.1.1", Kind::Text, None),
        ("10.0.26300", Kind::Text, None),
        ("2024-06-15", Kind::Text, None),
        ("3.14159265", Kind::Text, None),
        ("1718452800", Kind::Text, None),
        ("12345678901234567", Kind::Text, None),
        ("42", Kind::Text, None),
        // --- colors
        ("#ff8800", Kind::Color, Some("hex")),
        ("#FFF", Kind::Color, Some("hex")),
        ("#11223344", Kind::Color, Some("hex")),
        ("rgb(255, 128, 0)", Kind::Color, Some("rgb")),
        ("rgba(0 0 0 / 50%)", Kind::Color, Some("rgb")),
        ("hsl(210deg, 40%, 50%)", Kind::Color, Some("hsl")),
        ("#hashtag", Kind::Text, None),
        // --- paths
        (
            r"C:\Users\abhijit\Desktop\report.docx",
            Kind::File,
            Some("path"),
        ),
        (
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            Kind::File,
            Some("path"),
        ),
        (r#""D:\My Projects\notes.txt""#, Kind::File, Some("path")),
        (
            r"\\fileserver\share\team\plan.xlsx",
            Kind::File,
            Some("path"),
        ),
        ("/usr/local/bin/python3", Kind::File, Some("path")),
        ("~/.config/nvim/init.lua", Kind::File, Some("path")),
        ("./scripts/build.sh", Kind::File, Some("path")),
        ("file:///C:/Users/me/a.txt", Kind::File, Some("path")),
        // --- code: rust
        (
            "fn main() {\n    let mut v = Vec::new();\n    v.push(1);\n    println!(\"{:?}\", v);\n}",
            Kind::Code,
            Some("rust"),
        ),
        (
            "use std::collections::HashMap;\n\n#[derive(Debug, Clone)]\npub struct Config {\n    pub name: String,\n    pub retries: u32,\n}\n\nimpl Config {\n    pub fn new(name: &str) -> Self {\n        Self { name: name.to_owned(), retries: 3 }\n    }\n}",
            Kind::Code,
            Some("rust"),
        ),
        (
            "let total: u64 = items.iter().map(|i| i.size).sum();",
            Kind::Code,
            None,
        ),
        // --- code: python
        (
            "import os\nimport sys\n\ndef main(args):\n    for name in os.listdir(args[0]):\n        if name.endswith('.py'):\n            print(name)\n    return 0\n\nif __name__ == '__main__':\n    sys.exit(main(sys.argv[1:]))",
            Kind::Code,
            Some("python"),
        ),
        (
            "class Point:\n    def __init__(self, x, y):\n        self.x = x\n        self.y = y\n\n    def norm(self):\n        return (self.x ** 2 + self.y ** 2) ** 0.5",
            Kind::Code,
            Some("python"),
        ),
        ("import numpy as np", Kind::Code, Some("python")),
        ("let me know :)", Kind::Text, None),
        ("print(\"hello world\")", Kind::Code, None),
        // --- code: javascript / typescript
        (
            "const express = require('express');\nconst app = express();\n\napp.get('/', (req, res) => {\n  res.send('Hello');\n});\n\napp.listen(3000);",
            Kind::Code,
            Some("javascript"),
        ),
        (
            "export function debounce(fn, ms) {\n  let t;\n  return (...args) => {\n    clearTimeout(t);\n    t = setTimeout(() => fn(...args), ms);\n  };\n}",
            Kind::Code,
            Some("javascript"),
        ),
        (
            "interface User {\n  id: number;\n  name: string;\n}\n\nexport const greet = (u: User): string => `Hi ${u.name}`;",
            Kind::Code,
            Some("typescript"),
        ),
        (
            "console.log(JSON.stringify(data, null, 2));",
            Kind::Code,
            Some("javascript"),
        ),
        // --- code: other languages
        (
            "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tx := 42\n\tfmt.Println(x)\n}",
            Kind::Code,
            Some("go"),
        ),
        (
            "public class Hello {\n    public static void main(String[] args) {\n        System.out.println(\"Hello\");\n    }\n}",
            Kind::Code,
            Some("java"),
        ),
        (
            "using System;\n\nnamespace Demo\n{\n    class Program\n    {\n        static void Main(string[] args)\n        {\n            Console.WriteLine(\"Hi\");\n        }\n    }\n}",
            Kind::Code,
            Some("csharp"),
        ),
        (
            "#include <stdio.h>\n\nint main(void) {\n    printf(\"hi\\n\");\n    return 0;\n}",
            Kind::Code,
            Some("c"),
        ),
        (
            "#include <iostream>\n#include <vector>\n\nint main() {\n    std::vector<int> v{1, 2, 3};\n    std::cout << v.size() << std::endl;\n}",
            Kind::Code,
            Some("cpp"),
        ),
        (
            "data class User(val id: Int, val name: String)\n\nfun main() {\n    val u = User(1, \"Ann\")\n    println(u.name)\n}",
            Kind::Code,
            Some("kotlin"),
        ),
        // --- code: data/markup
        (
            "{\"name\": \"ClipTrail\", \"version\": \"0.1.0\", \"tags\": [\"rust\", \"tauri\"]}",
            Kind::Code,
            Some("json"),
        ),
        (
            "[\n  {\"id\": 1},\n  {\"id\": 2}\n]",
            Kind::Code,
            Some("json"),
        ),
        (
            "SELECT id, name FROM users WHERE created_at > '2024-01-01' ORDER BY name;",
            Kind::Code,
            Some("sql"),
        ),
        (
            "select u.id, count(*) from users u join orders o on o.user_id = u.id group by u.id",
            Kind::Code,
            Some("sql"),
        ),
        (
            "INSERT INTO logs (level, msg) VALUES ('info', 'started');",
            Kind::Code,
            Some("sql"),
        ),
        (
            "<!DOCTYPE html>\n<html>\n<head><title>Hi</title></head>\n<body><p>Hello</p></body>\n</html>",
            Kind::Code,
            Some("html"),
        ),
        (
            "<div class=\"card\"><span>Hello</span></div>",
            Kind::Code,
            Some("html"),
        ),
        (
            "<?xml version=\"1.0\"?>\n<project><name>demo</name></project>",
            Kind::Code,
            Some("xml"),
        ),
        (
            ".button {\n  color: #fff;\n  background-color: #0066ff;\n  padding: 8px 16px;\n}\n\n.button:hover {\n  opacity: 0.9;\n}",
            Kind::Code,
            Some("css"),
        ),
        (
            "services:\n  web:\n    image: nginx:latest\n    ports:\n      - \"80:80\"",
            Kind::Code,
            Some("yaml"),
        ),
        (
            "[package]\nname = \"clipd-core\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"",
            Kind::Code,
            Some("toml"),
        ),
        (
            "diff --git a/src/lib.rs b/src/lib.rs\nindex 83db48f..bf269f4 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@\n-fn old() {}\n+fn new() {}\n fn same() {}",
            Kind::Code,
            Some("diff"),
        ),
        (
            "```python\nprint('hi')\nx = 1\n```",
            Kind::Code,
            Some("python"),
        ),
        // --- code: shell / powershell
        (
            "git commit -m \"fix the parser bug\"",
            Kind::Code,
            Some("shell"),
        ),
        ("cargo build --release", Kind::Code, Some("shell")),
        (
            "npm install --save-dev typescript",
            Kind::Code,
            Some("shell"),
        ),
        ("docker run -d -p 8080:80 nginx", Kind::Code, Some("shell")),
        ("cd ~/projects/cliptrail", Kind::Code, Some("shell")),
        ("$ ls -la /var/log", Kind::Code, Some("shell")),
        (
            "curl -fsSL https://example.com/install.sh | sh",
            Kind::Code,
            Some("shell"),
        ),
        (
            "#!/bin/bash\nset -euo pipefail\nfor f in *.log; do\n  gzip \"$f\"\ndone",
            Kind::Code,
            Some("shell"),
        ),
        (
            "git add .\ngit commit -m \"wip\"\ngit push origin main",
            Kind::Code,
            Some("shell"),
        ),
        (
            "Get-ChildItem -Path C:\\Logs -Recurse | Where-Object { $_.Length -gt 1MB }",
            Kind::Code,
            Some("powershell"),
        ),
        (
            "$files = Get-ChildItem -Filter *.txt\nforeach ($f in $files) {\n    Write-Host $f.Name\n}",
            Kind::Code,
            Some("powershell"),
        ),
        // --- prose and other text
        (
            "The meeting has been moved to Thursday at 3pm. Please update your calendars and let me know if that doesn't work.",
            Kind::Text,
            None,
        ),
        (
            "Hi Sam,\n\nThanks for sending the report over. I had a look at the numbers and they seem fine to me.\n\nBest,\nAnna",
            Kind::Text,
            None,
        ),
        (
            "Shopping list:\n- milk\n- eggs\n- bread\n- coffee beans",
            Kind::Text,
            None,
        ),
        (
            "1. Install Rust from the official site\n2. Clone the repository\n3. Run the build script and wait",
            Kind::Text,
            None,
        ),
        (
            "# Release notes\n\nThis release fixes a crash when the clipboard is empty. It also makes search faster.\n\n## Changes\n\n- Faster search\n- Fewer crashes",
            Kind::Text,
            None,
        ),
        (
            "Select the items from the list, then click Save.",
            Kind::Text,
            None,
        ),
        ("select all of them from the drawer", Kind::Text, None),
        (
            "git is a distributed version control system.",
            Kind::Text,
            None,
        ),
        (
            "make sure the build passes before merging",
            Kind::Text,
            None,
        ),
        ("go to the store and buy milk", Kind::Text, None),
        (
            "Note: the function returns early if (and only if) the list is empty; otherwise it loops.",
            Kind::Text,
            None,
        ),
        (
            "To declare a variable in Rust, write let x = 5; and the compiler infers the type for you.",
            Kind::Text,
            None,
        ),
        (
            "Name: John Smith\nCompany: Acme Corp\nRole: Engineer",
            Kind::Text,
            None,
        ),
        ("Hello world", Kind::Text, None),
        ("Get-Started with the new dashboard today", Kind::Text, None),
        ("नमस्ते दुनिया, यह एक परीक्षण है।", Kind::Text, None),
        ("😀🎉 party time 🎉😀", Kind::Text, None),
        (
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\nUt enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.",
            Kind::Text,
            None,
        ),
        ("[1]", Kind::Text, None),
        ("I like apples;", Kind::Text, None),
        ("Call me at (555) 123-4567 tomorrow", Kind::Text, None),
        (
            "Preheat oven to 180°C.\nMix flour (200g) and sugar (100g).\nBake for 25 minutes.",
            Kind::Text,
            None,
        ),
        (
            "Dear team,\nPlease find attached the report (v2) for Q3; let me know if you have questions.\nThanks!",
            Kind::Text,
            None,
        ),
        (
            "I think we should use the new API (it's faster) and deploy on Monday.",
            Kind::Text,
            None,
        ),
        (
            "id,name,email\n1,Ann,ann@x.com\n2,Bob,bob@x.com",
            Kind::Text,
            None,
        ),
        ("550e8400-e29b-41d4-a716-446655440000", Kind::Text, None),
        ("#123", Kind::Text, None),
        ("#333", Kind::Color, Some("hex")),
        ("~", Kind::Text, None),
        ("const x = 5", Kind::Code, None),
        ("if (x > 5) { return; }", Kind::Code, None),
        ("while true; do echo hi; done", Kind::Code, Some("shell")),
        ("for i in range(10): print(i)", Kind::Code, Some("python")),
        (
            "Set-ExecutionPolicy RemoteSigned -Scope CurrentUser",
            Kind::Code,
            Some("powershell"),
        ),
        ("kubectl get pods -n kube-system", Kind::Code, Some("shell")),
        (
            "name: CI\non:\n  push:\n    branches: [main]\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: cargo test",
            Kind::Code,
            Some("yaml"),
        ),
        (
            "export default function App() {\n  return (\n    <div className=\"app\">\n      <h1>Hello</h1>\n    </div>\n  );\n}",
            Kind::Code,
            Some("javascript"),
        ),
        ("+91-98765-43210", Kind::Contact, Some("phone")),
        ("1-800-555-0199", Kind::Contact, Some("phone")),
        (
            "Some notes\n\n```\nfoo\n```\n\nThis paragraph explains the snippet above in plenty of detail for readers.\nAnd another long line of explanation follows here to make it mostly prose.\nOne more sentence for good measure, because prose should win here.",
            Kind::Text,
            None,
        ),
    ];

    #[test]
    fn sample_table() {
        let mut failures = Vec::new();
        for (input, kind, sub) in SAMPLES {
            let got = classify_text(input);
            if got.kind != *kind || got.subkind.as_deref() != *sub {
                failures.push(format!(
                    "{input:?}\n    expected {kind:?}/{sub:?}, got {:?}/{:?}",
                    got.kind, got.subkind
                ));
            }
        }
        let total = SAMPLES.len();
        assert!(
            failures.is_empty(),
            "{}/{} samples misclassified:\n{}",
            failures.len(),
            total,
            failures.join("\n")
        );
        assert!(total >= 60);
    }

    #[test]
    fn all_regexes_compile() {
        assert!(!FINGERPRINT_RES.is_empty());
        for r in [
            &PRIVATE_KEY,
            &JWT,
            &API_KEY,
            &CARD,
            &OTP,
            &BARE_DOMAIN_PATH,
            &HOST,
            &EMAIL,
            &PHONE_CHARS,
            &DATE_LIKE,
            &DECIMAL,
            &HEX_COLOR,
            &RGB_COLOR,
            &HSL_COLOR,
            &WIN_PATH,
            &UNC_PATH,
            &UNIX_PATH,
            &QUOTED_UNIX_PATH,
            &DIFF_HEADER,
            &HTML_TAG,
            &XML_TAG,
            &CLOSE_OR_SELF_CLOSING,
            &SQL_STATEMENT,
            &SQL_SELECT,
            &SQL_COLUMN,
            &SQL_EXTRA,
            &YAML_KV,
            &YAML_ITEM,
            &TOML_TABLE,
            &TOML_KV,
            &CMDLET,
            &CALL_EXPR,
            &IMPORT_LINE,
            &DECLARATION,
            &VAR_DECL,
            &SHELL_ONELINER,
            &PY_ONELINER,
            &ASSIGNMENT,
            &KEYWORD_LINE,
            &OPERATOR,
            &CALL,
            &ASSIGN_STMT,
            &LIST_ITEM,
        ] {
            let _ = r.is_match("");
        }
    }

    #[test]
    fn luhn_check() {
        assert!(luhn(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]));
        assert!(!luhn(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2]));
    }

    #[test]
    fn never_panics_on_odd_input() {
        let long_code = "let x = 1;\n".repeat(100_000);
        let long_prose = "The quick brown fox jumps over the lazy dog. ".repeat(50_000);
        let inputs = [
            String::new(),
            " \n\t ".into(),
            "\u{0}\u{1}\u{2}".into(),
            "é".repeat(30_000),
            "{".repeat(50_000),
            "[".into(),
            "``````".into(),
            "```".into(),
            "<".into(),
            "<>".into(),
            "#".into(),
            "$".into(),
            "$ ".into(),
            "PS >".into(),
            "https://[::1]:8080/x".into(),
            "https://[".into(),
            "http://@/".into(),
            "mailto:".into(),
            "\u{202e}gnp.exe".into(),
            "a\r\nb\rc".into(),
            "🦀".repeat(25_000),
            long_code.clone(),
            long_prose.clone(),
        ];
        for input in &inputs {
            let _ = classify_text(input);
        }
        assert_eq!(classify_text(&long_code).kind, Kind::Code);
        assert_eq!(classify_text(&long_prose).kind, Kind::Text);
        assert_eq!(
            classify_text("https://[::1]:8080/x").subkind.as_deref(),
            Some("[::1]")
        );
    }

    #[test]
    fn huge_input_is_fast() {
        let big = "fn f() { let x = 1; }\n".repeat(200_000); // ~4.4 MB
        let start = std::time::Instant::now();
        let _ = classify_text(&big);
        // Generous: debug builds on CI. The point is "capped, not O(n)".
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}
