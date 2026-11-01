//! The clip model. This file is the contract shared by every crate and the
//! desktop UI (types serialize to the JSON the frontend reads). Change it
//! deliberately and update all callers in the same commit.

use serde::{Deserialize, Serialize};

/// Unix time in milliseconds (UTC).
pub type Millis = i64;

/// Top-level category of a clip. Used for filtering, card rendering and
/// choosing how to paste. Serialized lowercase: "link", "code", ...
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A single URL. subkind = host, e.g. "github.com".
    Link,
    /// Source code or structured text. subkind = language guess
    /// ("rust", "python", "json", "sql", "shell", ...) or None if unknown.
    Code,
    /// Prose, lists, anything else textual. subkind = None.
    Text,
    /// Email address or phone number. subkind = "email" | "phone".
    Contact,
    /// A color literal. subkind = "hex" | "rgb" | "hsl".
    Color,
    /// One or more file/folder references (copied in Explorer, or a pasted
    /// path). subkind = "files" | "path".
    File,
    /// Bitmap image (e.g. a screenshot). subkind = "screenshot" | None.
    Image,
    /// OTPs, API keys, tokens, private keys, card numbers. Stored masked
    /// with a short expiry, never indexed for search.
    Sensitive,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::Link,
        Kind::Code,
        Kind::Text,
        Kind::Contact,
        Kind::Color,
        Kind::File,
        Kind::Image,
        Kind::Sensitive,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Link => "link",
            Kind::Code => "code",
            Kind::Text => "text",
            Kind::Contact => "contact",
            Kind::Color => "color",
            Kind::File => "file",
            Kind::Image => "image",
            Kind::Sensitive => "sensitive",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// What a platform watcher hands to the pipeline. Produced by
/// `clipd-capture` (desktop) or the Android shell.
#[derive(Debug, Clone)]
pub struct RawClip {
    pub content: RawContent,
    /// Executable name of the app that owned the clipboard / foreground
    /// window when the copy happened, e.g. "chrome.exe". None if unknown.
    pub source_app: Option<String>,
    pub captured_at: Millis,
}

#[derive(Debug, Clone)]
pub enum RawContent {
    /// Plain text, plus the HTML flavor when the source offered one.
    Text { text: String, html: Option<String> },
    /// Decoded bitmap, RGBA8, row-major.
    Image {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// File list copied from a file manager (absolute paths).
    Files(Vec<String>),
}

/// Result of the classifier for a piece of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub kind: Kind,
    pub subkind: Option<String>,
}

/// A row as shown in lists: everything the UI needs to draw a card,
/// with the body cut down to `preview`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipSummary {
    pub id: String,
    pub kind: Kind,
    pub subkind: Option<String>,
    /// First ~400 chars (masked clips: "••••••"). Empty for images.
    pub preview: String,
    /// True when `preview` is shorter than the full text.
    pub truncated: bool,
    pub char_count: i64,
    pub line_count: i64,
    pub files: Vec<String>,
    /// Absolute path of the thumbnail PNG on disk (images only).
    pub thumb_path: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub source_app: Option<String>,
    pub device_id: String,
    pub created_at: Millis,
    /// Last time this exact content was copied; lists sort by this.
    pub last_seen_at: Millis,
    pub copy_count: i64,
    pub pinned: bool,
    pub masked: bool,
    pub expires_at: Option<Millis>,
}

/// A full clip: summary plus the complete body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    #[serde(flatten)]
    pub summary: ClipSummary,
    pub text: Option<String>,
    pub html: Option<String>,
    /// Absolute path of the full-size PNG on disk (images only).
    pub image_path: Option<String>,
}

/// Filters for `Store::list`. All fields optional; defaults = newest first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ListQuery {
    pub kind: Option<Kind>,
    /// Full-text / substring search. Empty or None = no text filter.
    pub search: Option<String>,
    pub pinned_only: bool,
    pub source_app: Option<String>,
    /// Only clips with last_seen_at in [from, to).
    pub from: Option<Millis>,
    pub to: Option<Millis>,
    /// Keyset pagination: return clips strictly older than this
    /// last_seen_at (pass the last row's value to get the next page).
    pub before: Option<Millis>,
    /// Tie-breaker for `before`: the last row's id. Pass both to page
    /// exactly even when many clips share one timestamp.
    pub before_id: Option<String>,
    /// Max rows, default 100.
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub total: i64,
    /// (kind, count), one entry per kind that has clips.
    pub by_kind: Vec<(Kind, i64)>,
    pub pinned: i64,
    /// Top source apps by clip count: (app, count), max 10.
    pub top_apps: Vec<(String, i64)>,
}

/// Clip count for one local calendar day, for the heatmap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayCount {
    /// "YYYY-MM-DD" in the machine's local time zone.
    pub day: String,
    pub count: i64,
}

/// What happened to a RawClip that went through the pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum IngestOutcome {
    /// New clip stored.
    Stored { id: String, kind: Kind },
    /// Same content already existed; its last_seen_at/copy_count were bumped.
    Bumped { id: String, kind: Kind },
    /// Dropped (empty, too large, excluded app, ...). `reason` is safe to
    /// log: it never contains clip content.
    Skipped { reason: String },
}
