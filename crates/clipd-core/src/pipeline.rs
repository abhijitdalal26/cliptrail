//! normalize → hash → dedup → classify → rules → persist.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use image::{ImageFormat, RgbaImage, imageops};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use crate::classify::classify_text;
use crate::model::*;
use crate::now_millis;
use crate::store::{NewClip, Store};

/// Thumbnails fit inside this box (aspect ratio kept, never upscaled).
pub const THUMB_MAX_WIDTH: u32 = 480;
pub const THUMB_MAX_HEIGHT: u32 = 320;

/// Password managers and authenticator apps whose copies are never stored.
/// Lowercased executable names; `*` is a wildcard. Names without `.exe`
/// cover macOS/Linux process names.
pub const DEFAULT_EXCLUDED_APPS: &[&str] = &[
    "1password.exe",
    "1password",
    "bitwarden.exe",
    "bitwarden",
    "keepass.exe",
    "keepassxc.exe",
    "keepassxc",
    "keepassx",
    "dashlane.exe",
    "dashlane",
    "lastpass.exe",
    "lastpass",
    "enpass.exe",
    "enpass",
    "nordpass.exe",
    "nordpass",
    "proton pass.exe",
    "proton pass",
    "protonpass.exe",
    "keeper*.exe",
    "keeper password manager",
    "roboform*.exe",
    "pwsafe.exe",
    "passwordsafe",
    "authy*.exe",
    "authy*",
    "seahorse",
    "kwalletmanager5",
];

const DEVICE_ID_FILE: &str = "device_id";
const DB_FILE: &str = "clips.db";
pub(crate) const IMAGES_DIR: &str = "images";
pub(crate) const THUMBS_DIR: &str = "thumbs";

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Root for `clips.db`, `images/`, `thumbs/`.
    pub data_dir: PathBuf,
    /// Stable id of this device (persisted in `data_dir/device_id`).
    pub device_id: String,
    /// Lowercased executable names whose copies are never stored
    /// (password managers by default).
    pub excluded_apps: Vec<String>,
    /// How long sensitive clips live before pruning (ms).
    pub sensitive_ttl_ms: Millis,
    /// Texts longer than this (bytes) are skipped.
    pub max_text_bytes: usize,
}

impl PipelineConfig {
    /// Defaults rooted at `data_dir`; creates the dir and device id file.
    pub fn with_defaults(data_dir: PathBuf) -> Result<PipelineConfig> {
        let data_dir = std::path::absolute(&data_dir).unwrap_or(data_dir);
        create_data_dirs(&data_dir)?;
        Ok(PipelineConfig {
            device_id: load_or_create_device_id(&data_dir)?,
            data_dir,
            excluded_apps: DEFAULT_EXCLUDED_APPS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            sensitive_ttl_ms: 60 * 60 * 1000,
            max_text_bytes: 2 * 1024 * 1024,
        })
    }
}

fn create_data_dirs(data_dir: &Path) -> Result<()> {
    for dir in [
        data_dir.to_path_buf(),
        data_dir.join(IMAGES_DIR),
        data_dir.join(THUMBS_DIR),
    ] {
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }
    Ok(())
}

fn load_or_create_device_id(data_dir: &Path) -> Result<String> {
    let path = data_dir.join(DEVICE_ID_FILE);
    match fs::read_to_string(&path) {
        Ok(s) => {
            let id = s.trim();
            let valid = !id.is_empty()
                && id.len() <= 64
                && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            if valid {
                return Ok(id.to_owned());
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    }
    let id = Ulid::generate().to_string();
    fs::write(&path, &id).with_context(|| format!("write {}", path.display()))?;
    Ok(id)
}

/// Which devices a history clear applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearScope {
    /// This device only (like retention); synced devices keep their clips.
    LocalOnly,
    /// Also emit per-clip delete tombstones for the sync engine.
    AllDevices,
}

pub struct Pipeline {
    config: PipelineConfig,
    store: Arc<Store>,
}

impl Pipeline {
    /// Open `data_dir/clips.db` and build a pipeline.
    pub fn open(mut config: PipelineConfig) -> Result<Pipeline> {
        if let Ok(abs) = std::path::absolute(&config.data_dir) {
            config.data_dir = abs;
        }
        create_data_dirs(&config.data_dir)?;
        let store = Store::open(&config.data_dir.join(DB_FILE))?;
        Ok(Pipeline {
            config,
            store: Arc::new(store),
        })
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    /// True if copies from `app` (an executable name or path) are dropped.
    pub fn is_excluded_app(&self, app: &str) -> bool {
        let name = app
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(app)
            .trim()
            .to_lowercase();
        !name.is_empty()
            && self
                .config
                .excluded_apps
                .iter()
                .any(|pat| wildcard_match(&pat.to_lowercase(), &name))
    }

    pub fn ingest(&self, raw: RawClip) -> Result<IngestOutcome> {
        if raw
            .source_app
            .as_deref()
            .is_some_and(|a| self.is_excluded_app(a))
        {
            return Ok(skipped("excluded source app"));
        }
        let at = if raw.captured_at > 0 {
            raw.captured_at
        } else {
            now_millis()
        };
        let outcome = match raw.content {
            RawContent::Text { text, html } => self.ingest_text(text, html, raw.source_app, at)?,
            RawContent::Image {
                width,
                height,
                rgba,
            } => self.ingest_image(width, height, rgba, raw.source_app, at)?,
            RawContent::Files(paths) => self.ingest_files(paths, raw.source_app, at)?,
        };
        match &outcome {
            IngestOutcome::Stored { id, kind } => {
                tracing::debug!(%id, kind = kind.as_str(), "clip stored")
            }
            IngestOutcome::Bumped { id, kind } => {
                tracing::debug!(%id, kind = kind.as_str(), "clip bumped")
            }
            IngestOutcome::Skipped { reason } => tracing::debug!(%reason, "clip skipped"),
        }
        Ok(outcome)
    }

    fn ingest_text(
        &self,
        text: String,
        html: Option<String>,
        source_app: Option<String>,
        at: Millis,
    ) -> Result<IngestOutcome> {
        if text.len() > self.config.max_text_bytes {
            return Ok(skipped(format!("text too large ({} bytes)", text.len())));
        }
        let normalized = normalize_text(&text);
        if normalized.trim().is_empty() {
            return Ok(skipped("empty text"));
        }
        let content_hash = hash_parts(b"text", &[normalized.as_bytes()]);
        let class = classify_text(&text);
        let sensitive = class.kind == Kind::Sensitive;
        // Never keep a rich copy of a secret; drop oversized/blank HTML too.
        let html = html.filter(|h| {
            !sensitive && h.len() <= self.config.max_text_bytes && !h.trim().is_empty()
        });
        let clip = NewClip {
            id: Ulid::generate().to_string(),
            content_hash,
            kind: class.kind,
            subkind: class.subkind,
            char_count: text.chars().count() as i64,
            line_count: text.lines().count().max(1) as i64,
            text: Some(text),
            html,
            files: Vec::new(),
            image_path: None,
            thumb_path: None,
            width: None,
            height: None,
            source_app,
            device_id: self.config.device_id.clone(),
            created_at: at,
            masked: sensitive,
            expires_at: sensitive.then(|| at.saturating_add(self.config.sensitive_ttl_ms)),
        };
        self.persist(clip)
    }

    fn ingest_files(
        &self,
        paths: Vec<String>,
        source_app: Option<String>,
        at: Millis,
    ) -> Result<IngestOutcome> {
        let paths: Vec<String> = paths.into_iter().filter(|p| !p.trim().is_empty()).collect();
        if paths.is_empty() {
            return Ok(skipped("empty file list"));
        }
        let mut sorted: Vec<&[u8]> = paths.iter().map(|p| p.as_bytes()).collect();
        sorted.sort_unstable();
        // NUL can't appear in a path, so it's an unambiguous separator.
        let content_hash = hash_parts(b"files", &[&sorted.join(&0u8)]);
        let text = paths.join("\n");
        let clip = NewClip {
            id: Ulid::generate().to_string(),
            content_hash,
            kind: Kind::File,
            subkind: Some("files".to_owned()),
            char_count: text.chars().count() as i64,
            line_count: paths.len() as i64,
            text: Some(text),
            html: None,
            files: paths,
            image_path: None,
            thumb_path: None,
            width: None,
            height: None,
            source_app,
            device_id: self.config.device_id.clone(),
            created_at: at,
            masked: false,
            expires_at: None,
        };
        self.persist(clip)
    }

    fn ingest_image(
        &self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        source_app: Option<String>,
        at: Millis,
    ) -> Result<IngestOutcome> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(4));
        if width == 0 || height == 0 || expected != Some(rgba.len()) {
            return Ok(skipped("invalid image buffer"));
        }
        let content_hash = hash_parts(
            b"image",
            &[&width.to_le_bytes(), &height.to_le_bytes(), &rgba],
        );
        // Cheap path for repeats: skip PNG encoding entirely.
        if let Some(id) = self.store.bump_existing(
            &content_hash,
            at,
            source_app.as_deref(),
            &self.config.device_id,
            None,
        )? {
            return Ok(IngestOutcome::Bumped {
                id,
                kind: Kind::Image,
            });
        }

        let id = Ulid::generate().to_string();
        let img = RgbaImage::from_raw(width, height, rgba).context("image buffer size")?;
        let image_path = self
            .config
            .data_dir
            .join(IMAGES_DIR)
            .join(format!("{id}.png"));
        let thumb_path = self
            .config
            .data_dir
            .join(THUMBS_DIR)
            .join(format!("{id}.png"));
        write_png(&img, &image_path)?;
        if let Err(e) = write_png(&thumbnail(&img), &thumb_path) {
            remove_files([&image_path]);
            return Err(e);
        }

        let clip = NewClip {
            id,
            content_hash,
            kind: Kind::Image,
            subkind: None,
            text: None,
            html: None,
            files: Vec::new(),
            image_path: Some(image_path.to_string_lossy().into_owned()),
            thumb_path: Some(thumb_path.to_string_lossy().into_owned()),
            width: Some(i64::from(width)),
            height: Some(i64::from(height)),
            char_count: 0,
            line_count: 0,
            source_app,
            device_id: self.config.device_id.clone(),
            created_at: at,
            masked: false,
            expires_at: None,
        };
        let result = self.persist(clip);
        // Lost a race with an identical image (or the insert failed): our
        // freshly written files belong to nobody.
        if !matches!(result, Ok(IngestOutcome::Stored { .. })) {
            remove_files([&image_path, &thumb_path]);
        }
        result
    }

    fn persist(&self, clip: NewClip) -> Result<IngestOutcome> {
        let kind = clip.kind;
        let (id, inserted) = self.store.upsert(clip)?;
        Ok(if inserted {
            IngestOutcome::Stored { id, kind }
        } else {
            IngestOutcome::Bumped { id, kind }
        })
    }

    /// Delete a clip and its files on disk.
    pub fn delete(&self, id: &str) -> Result<()> {
        let files = self.store.delete(id)?;
        remove_files(&files);
        Ok(())
    }

    /// How many clips [`Pipeline::clear_history`] would remove.
    pub fn count_clearable(&self, keep_pinned: bool) -> Result<i64> {
        self.store.count_clearable(keep_pinned)
    }

    /// Clear the history and its image files; returns how many clips went.
    ///
    /// [`ClearScope::LocalOnly`] behaves like retention (see
    /// `delete_local`): nothing is journaled, so other synced devices keep
    /// their copies. [`ClearScope::AllDevices`] goes through the normal
    /// delete path, so each unmasked clip leaves a delete tombstone in the
    /// journal and the sync engine removes it on the other devices. Pinned
    /// clips are untouched when `keep_pinned` (on every device). Masked
    /// (sensitive) clips are never journaled, so they only ever clear here.
    pub fn clear_history(&self, keep_pinned: bool, scope: ClearScope) -> Result<usize> {
        let (count, files) = match scope {
            ClearScope::AllDevices => {
                let conn = self.store.conn();
                crate::store::clear_rows(&conn, keep_pinned)?
            }
            ClearScope::LocalOnly => self
                .store
                .quiet(|conn| crate::store::clear_rows(conn, keep_pinned))?,
        };
        remove_files(&files);
        tracing::debug!(count, ?scope, keep_pinned, "history cleared");
        Ok(count)
    }

    /// Remove expired clips and their files. Returns how many were removed.
    pub fn prune(&self) -> Result<usize> {
        let now = now_millis();
        let (count, files) = self.store.prune_expired_counted(now)?;
        remove_files(&files);
        // The sync journal only needs to bridge short gaps (sync engine not
        // running); a month is plenty.
        self.store
            .trim_journal(now - crate::synced::JOURNAL_KEEP_MS)?;
        if count > 0 {
            tracing::debug!(count, "pruned expired clips");
        }
        Ok(count)
    }
}

pub(crate) fn skipped(reason: impl Into<String>) -> IngestOutcome {
    IngestOutcome::Skipped {
        reason: reason.into(),
    }
}

/// Dedup form of a text: CRLF → LF, trailing whitespace dropped.
fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_owned()
}

/// SHA-256 over a domain tag plus length-prefixed parts, so different
/// content types (and part boundaries) can never collide.
fn hash_parts(domain: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(domain);
    for part in parts {
        h.update((part.len() as u64).to_le_bytes());
        h.update(part);
    }
    h.finalize().to_vec()
}

pub(crate) fn thumbnail(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let (tw, th) = thumb_size(w, h);
    if (tw, th) == (w, h) {
        img.clone()
    } else {
        imageops::thumbnail(img, tw, th)
    }
}

/// Fit (w, h) inside the thumbnail box keeping aspect; never upscale.
fn thumb_size(w: u32, h: u32) -> (u32, u32) {
    let scale = (f64::from(THUMB_MAX_WIDTH) / f64::from(w))
        .min(f64::from(THUMB_MAX_HEIGHT) / f64::from(h))
        .min(1.0);
    let fit = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
    (fit(w), fit(h))
}

/// Encode to `<path>.tmp`, then rename into place so readers never see a
/// half-written PNG.
pub(crate) fn write_png(img: &RgbaImage, path: &Path) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = img
        .save_with_format(&tmp, ImageFormat::Png)
        .map_err(anyhow::Error::from)
        .and_then(|()| fs::rename(&tmp, path).map_err(anyhow::Error::from));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.with_context(|| format!("write {}", path.display()))
}

pub(crate) fn remove_files<P: AsRef<Path>>(paths: impl IntoIterator<Item = P>) {
    for p in paths {
        let p = p.as_ref();
        if let Err(e) = fs::remove_file(p)
            && e.kind() != ErrorKind::NotFound
        {
            tracing::warn!(path = %p.display(), error = %e, "could not remove clip file");
        }
    }
}

/// Case-sensitive glob with `*` only (callers lowercase both sides).
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] != '*' && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> TempDir {
            let dir = std::env::temp_dir().join(format!("clipd-test-{}", Ulid::generate()));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn pipeline(dir: &TempDir) -> Pipeline {
        Pipeline::open(PipelineConfig::with_defaults(dir.0.clone()).unwrap()).unwrap()
    }

    fn raw(content: RawContent, app: Option<&str>) -> RawClip {
        RawClip {
            content,
            source_app: app.map(str::to_owned),
            captured_at: now_millis(),
        }
    }

    fn text(s: &str) -> RawClip {
        raw(
            RawContent::Text {
                text: s.to_owned(),
                html: None,
            },
            Some("notepad.exe"),
        )
    }

    fn image(w: u32, h: u32, seed: u8) -> RawClip {
        let rgba = (0..w * h * 4)
            .map(|i| (i as u8).wrapping_add(seed))
            .collect();
        raw(
            RawContent::Image {
                width: w,
                height: h,
                rgba,
            },
            Some("snippingtool.exe"),
        )
    }

    fn stored_id(o: IngestOutcome) -> String {
        match o {
            IngestOutcome::Stored { id, .. } => id,
            other => panic!("expected Stored, got {other:?}"),
        }
    }

    #[test]
    fn config_defaults_and_device_id_persist() {
        let dir = TempDir::new();
        let a = PipelineConfig::with_defaults(dir.0.join("data")).unwrap();
        assert!(a.data_dir.join("images").is_dir());
        assert!(a.data_dir.join("thumbs").is_dir());
        assert_eq!(a.device_id.len(), 26);
        assert_eq!(a.sensitive_ttl_ms, 3_600_000);
        assert_eq!(a.max_text_bytes, 2 * 1024 * 1024);
        assert!(a.excluded_apps.iter().any(|s| s == "keepassxc.exe"));
        let b = PipelineConfig::with_defaults(dir.0.join("data")).unwrap();
        assert_eq!(a.device_id, b.device_id);
    }

    #[test]
    fn text_store_dedup_and_normalization() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        let id = stored_id(p.ingest(text("line one\r\nline two  \r\n")).unwrap());
        assert_eq!(
            p.ingest(text("line one\nline two")).unwrap(),
            IngestOutcome::Bumped {
                id: id.clone(),
                kind: Kind::Text
            }
        );
        let clip = p.store().get(&id).unwrap().unwrap();
        // The original bytes are kept; only the hash is normalized.
        assert_eq!(clip.text.as_deref(), Some("line one\r\nline two  \r\n"));
        assert_eq!(clip.summary.copy_count, 2);
        assert_eq!(clip.summary.line_count, 2);
        assert_eq!(clip.summary.device_id, p.config().device_id);
    }

    #[test]
    fn skips_empty_large_and_excluded() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        for o in [
            p.ingest(text("   \n\t ")).unwrap(),
            p.ingest(text(&"x".repeat(p.config().max_text_bytes + 1)))
                .unwrap(),
            p.ingest(raw(
                RawContent::Text {
                    text: "hunter2".into(),
                    html: None,
                },
                Some(r"C:\Program Files\KeePassXC\KeePassXC.exe"),
            ))
            .unwrap(),
            p.ingest(raw(
                RawContent::Text {
                    text: "x".into(),
                    html: None,
                },
                Some("KeeperPasswordManager.exe"),
            ))
            .unwrap(),
            p.ingest(raw(RawContent::Files(vec![" ".into()]), None))
                .unwrap(),
            p.ingest(raw(
                RawContent::Image {
                    width: 2,
                    height: 2,
                    rgba: vec![0; 3],
                },
                None,
            ))
            .unwrap(),
        ] {
            assert!(matches!(o, IngestOutcome::Skipped { .. }), "{o:?}");
        }
        assert!(p.store().list(&ListQuery::default()).unwrap().is_empty());
        assert!(!p.is_excluded_app("chrome.exe"));
    }

    #[test]
    fn classifies_and_keeps_html() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        let id = stored_id(
            p.ingest(raw(
                RawContent::Text {
                    text: "https://www.github.com/rust-lang".into(),
                    html: Some("<a href=\"https://github.com/rust-lang\">x</a>".into()),
                },
                Some("chrome.exe"),
            ))
            .unwrap(),
        );
        let clip = p.store().get(&id).unwrap().unwrap();
        assert_eq!(clip.summary.kind, Kind::Link);
        assert_eq!(clip.summary.subkind.as_deref(), Some("github.com"));
        assert!(clip.html.is_some());
        assert_eq!(clip.summary.expires_at, None);
    }

    #[test]
    fn sensitive_is_masked_with_expiry_and_pruned() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        let before = now_millis();
        let id = stored_id(
            p.ingest(raw(
                RawContent::Text {
                    text: "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8".into(),
                    html: Some("<b>token</b>".into()),
                },
                Some("chrome.exe"),
            ))
            .unwrap(),
        );
        let clip = p.store().get(&id).unwrap().unwrap();
        assert_eq!(clip.summary.kind, Kind::Sensitive);
        assert_eq!(clip.summary.subkind.as_deref(), Some("api-key"));
        assert!(clip.summary.masked);
        assert!(clip.html.is_none());
        let exp = clip.summary.expires_at.unwrap();
        assert!(exp >= before + 3_600_000 && exp <= now_millis() + 3_600_000);
        let found = p
            .store()
            .list(&ListQuery {
                search: Some("ghp_A1b".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(found.is_empty());

        assert_eq!(p.prune().unwrap(), 0);
        let mut cfg = p.config().clone();
        cfg.sensitive_ttl_ms = -1;
        let p2 = Pipeline::open(cfg).unwrap();
        stored_id(p2.ingest(text("482913")).unwrap());
        assert_eq!(p2.prune().unwrap(), 1);
    }

    #[test]
    fn images_write_png_and_thumbnail_once() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        let id = stored_id(p.ingest(image(1200, 400, 0)).unwrap());
        let clip = p.store().get(&id).unwrap().unwrap();
        assert_eq!(clip.summary.kind, Kind::Image);
        assert_eq!(clip.summary.subkind, None);
        assert_eq!(
            (clip.summary.width, clip.summary.height),
            (Some(1200), Some(400))
        );
        let full = clip.image_path.clone().unwrap();
        let thumb = clip.summary.thumb_path.clone().unwrap();
        assert!(Path::new(&full).is_absolute());
        assert_eq!(image::image_dimensions(&full).unwrap(), (1200, 400));
        assert_eq!(image::image_dimensions(&thumb).unwrap(), (480, 160));

        let mtime = fs::metadata(&full).unwrap().modified().unwrap();
        assert_eq!(
            p.ingest(image(1200, 400, 0)).unwrap(),
            IngestOutcome::Bumped {
                id: id.clone(),
                kind: Kind::Image
            }
        );
        assert_eq!(fs::metadata(&full).unwrap().modified().unwrap(), mtime);
        let images = fs::read_dir(dir.0.join("images")).unwrap().count();
        assert_eq!(images, 1, "no stray or temp files");

        // Small images are not upscaled.
        let small = stored_id(p.ingest(image(20, 10, 7)).unwrap());
        let small_thumb = p
            .store()
            .get(&small)
            .unwrap()
            .unwrap()
            .summary
            .thumb_path
            .unwrap();
        assert_eq!(image::image_dimensions(small_thumb).unwrap(), (20, 10));

        p.delete(&id).unwrap();
        assert!(!Path::new(&full).exists());
        assert!(!Path::new(&thumb).exists());
        assert!(p.store().get(&id).unwrap().is_none());
        p.delete(&id).unwrap();
    }

    #[test]
    fn clear_history_keeps_pinned_and_removes_files() {
        for scope in [ClearScope::LocalOnly, ClearScope::AllDevices] {
            let dir = TempDir::new();
            let p = pipeline(&dir);
            let img = stored_id(p.ingest(image(40, 30, 1)).unwrap());
            let keep = stored_id(p.ingest(text("keep me")).unwrap());
            stored_id(p.ingest(text("drop me")).unwrap());
            stored_id(
                p.ingest(text("ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8"))
                    .unwrap(),
            );
            p.store().set_pinned(&keep, true).unwrap();
            let clip = p.store().get(&img).unwrap().unwrap();
            let files = [
                clip.image_path.clone().unwrap(),
                clip.summary.thumb_path.clone().unwrap(),
            ];
            assert_eq!(p.count_clearable(true).unwrap(), 3);
            assert_eq!(p.count_clearable(false).unwrap(), 4);

            assert_eq!(p.clear_history(true, scope).unwrap(), 3);
            assert!(files.iter().all(|f| !Path::new(f).exists()));
            let left = p.store().list(&ListQuery::default()).unwrap();
            assert_eq!(left.len(), 1);
            assert_eq!(left[0].id, keep);
            assert_eq!(p.clear_history(true, scope).unwrap(), 0);

            assert_eq!(p.clear_history(false, scope).unwrap(), 1);
            assert!(p.store().list(&ListQuery::default()).unwrap().is_empty());
            assert_eq!(fs::read_dir(dir.0.join("images")).unwrap().count(), 0);
            assert_eq!(fs::read_dir(dir.0.join("thumbs")).unwrap().count(), 0);
        }
    }

    #[test]
    fn files_are_searchable_and_order_insensitive() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        let files = vec![
            r"C:\Users\me\report.docx".to_owned(),
            r"C:\Users\me\data.csv".to_owned(),
        ];
        let id = stored_id(
            p.ingest(raw(RawContent::Files(files.clone()), Some("explorer.exe")))
                .unwrap(),
        );
        let reversed: Vec<String> = files.iter().rev().cloned().collect();
        assert_eq!(
            p.ingest(raw(RawContent::Files(reversed), None)).unwrap(),
            IngestOutcome::Bumped {
                id: id.clone(),
                kind: Kind::File
            }
        );
        let clip = p.store().get(&id).unwrap().unwrap();
        assert_eq!(clip.summary.subkind.as_deref(), Some("files"));
        assert_eq!(clip.summary.files, files);
        assert_eq!(clip.summary.line_count, 2);
        let hits = p
            .store()
            .list(&ListQuery {
                search: Some("report.doc".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn same_bytes_different_types_do_not_collide() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        stored_id(p.ingest(text("/tmp/a")).unwrap());
        stored_id(
            p.ingest(raw(RawContent::Files(vec!["/tmp/a".into()]), None))
                .unwrap(),
        );
    }

    #[test]
    fn thumb_sizes() {
        assert_eq!(thumb_size(1920, 1080), (480, 270));
        assert_eq!(thumb_size(400, 1000), (128, 320));
        assert_eq!(thumb_size(100, 50), (100, 50));
        assert_eq!(thumb_size(10_000, 1), (480, 1));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("keeper*.exe", "keeperpasswordmanager.exe"));
        assert!(wildcard_match("keeper*.exe", "keeper.exe"));
        assert!(!wildcard_match("keeper*.exe", "keeper.exe.bak"));
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("a*b*c", "aXXbYYc"));
        assert!(!wildcard_match("a*b*c", "aXXbYY"));
        assert!(wildcard_match("1password.exe", "1password.exe"));
        assert!(!wildcard_match("1password.exe", "1password.exe2"));
    }
}
