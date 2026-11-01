//! Sync-facing API: the local change journal (read by a sync engine to
//! build events) and "quiet" writes that apply changes which came from
//! another device without journaling them again (no echo).
//!
//! Masked (sensitive) clips are never journaled and never accepted from
//! sync, so they never leave the device they were copied on.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use image::{ImageFormat, ImageReader};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::model::*;
use crate::pipeline::{
    IMAGES_DIR, Pipeline, THUMBS_DIR, remove_files, skipped, thumbnail, write_png,
};
use crate::store::{SUMMARY_COL_COUNT, SUMMARY_COLS, Store, summary_from_row};

/// Journal rows older than this are trimmed by [`Pipeline::prune`].
pub const JOURNAL_KEEP_MS: Millis = 30 * 86_400_000;
/// Largest full-size image accepted from sync (encoded PNG bytes).
pub const MAX_SYNCED_IMAGE_BYTES: usize = 48 * 1024 * 1024;
/// Largest inline thumbnail accepted from sync.
pub const MAX_SYNCED_THUMB_BYTES: usize = 1024 * 1024;
/// Largest image side accepted from sync, in pixels.
const MAX_SYNCED_IMAGE_SIDE: u32 = 32_768;

/// What happened to a clip locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JournalOp {
    /// New clip or a repeat copy (last_seen_at / copy_count moved).
    Upsert,
    /// Pinned flag changed.
    Pin,
    /// Clip deleted by the user (not by retention or expiry).
    Delete,
}

impl JournalOp {
    fn parse(s: &str) -> Option<JournalOp> {
        match s {
            "upsert" => Some(JournalOp::Upsert),
            "pin" => Some(JournalOp::Pin),
            "delete" => Some(JournalOp::Delete),
            _ => None,
        }
    }
}

/// One row of the local change journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    /// Monotonic, never reused.
    pub seq: i64,
    pub op: JournalOp,
    pub clip_id: String,
    pub content_hash: Vec<u8>,
    /// Upsert: the clip's last_seen_at. Pin/delete: when it happened.
    pub at: Millis,
}

/// A clip as received from another device.
#[derive(Debug, Clone)]
pub struct SyncedClip {
    /// The sender's dedup hash (SHA-256, 32 bytes). Identity across devices.
    pub content_hash: Vec<u8>,
    pub kind: Kind,
    pub subkind: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub files: Vec<String>,
    pub source_app: Option<String>,
    /// Device the clip was (last) copied on.
    pub device_id: String,
    pub created_at: Millis,
    pub last_seen_at: Millis,
    pub copy_count: i64,
    pub image: Option<SyncedImage>,
}

#[derive(Debug, Clone)]
pub struct SyncedImage {
    pub width: u32,
    pub height: u32,
    /// Small PNG preview sent inline. The full image follows separately via
    /// [`Pipeline::attach_synced_image`].
    pub thumb_png: Option<Vec<u8>>,
}

/// A full clip plus its dedup hash.
#[derive(Debug, Clone)]
pub struct ClipWithHash {
    pub clip: Clip,
    pub content_hash: Vec<u8>,
}

impl Store {
    /// Highest journal seq so far (0 when empty). A new sync engine starts
    /// its cursor here and backfills from the clip table instead.
    pub fn journal_head(&self) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT coalesce(max(seq), 0) FROM clip_journal", [], |r| {
                r.get(0)
            })?)
    }

    /// Journal rows with seq > `after`, oldest first.
    pub fn journal_since(&self, after: i64, limit: u32) -> Result<Vec<JournalEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT seq, op, clip_id, content_hash, at FROM clip_journal \
             WHERE seq > ?1 ORDER BY seq LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![after, limit], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, Millis>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, op, clip_id, content_hash, at) = row?;
            if let Some(op) = JournalOp::parse(&op) {
                out.push(JournalEntry {
                    seq,
                    op,
                    clip_id,
                    content_hash,
                    at,
                });
            }
        }
        Ok(out)
    }

    /// Drop journal rows older than `before`. Returns how many went.
    pub fn trim_journal(&self, before: Millis) -> Result<usize> {
        Ok(self
            .conn()
            .execute("DELETE FROM clip_journal WHERE at < ?1", [before])?)
    }

    /// Dedup hash of a clip.
    pub fn content_hash(&self, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn()
            .query_row("SELECT content_hash FROM clips WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Full clip by dedup hash.
    pub fn get_by_hash(&self, content_hash: &[u8]) -> Result<Option<ClipWithHash>> {
        let sql = format!(
            "SELECT {SUMMARY_COLS}, c.text, c.html, c.image_path, c.content_hash \
             FROM clips c WHERE c.content_hash = ?1"
        );
        Ok(self
            .conn()
            .query_row(&sql, [content_hash], |r| {
                Ok(ClipWithHash {
                    clip: Clip {
                        summary: summary_from_row(r)?,
                        text: r.get(SUMMARY_COL_COUNT)?,
                        html: r.get(SUMMARY_COL_COUNT + 1)?,
                        image_path: r.get(SUMMARY_COL_COUNT + 2)?,
                    },
                    content_hash: r.get(SUMMARY_COL_COUNT + 3)?,
                })
            })
            .optional()?)
    }

    /// Set `pinned` on the clip with this hash, from sync (not journaled).
    /// Returns its id, or None when no (unmasked) clip has the hash.
    pub fn set_pinned_synced(&self, content_hash: &[u8], pinned: bool) -> Result<Option<String>> {
        self.quiet(|c| {
            Ok(c.query_row(
                "UPDATE clips SET pinned = ?2 WHERE content_hash = ?1 AND masked = 0 RETURNING id",
                params![content_hash, pinned],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Run `f` in an immediate transaction and drop every journal row it
    /// caused, so sync-applied changes are never re-broadcast.
    pub(crate) fn quiet<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let head: i64 =
            tx.query_row("SELECT coalesce(max(seq), 0) FROM clip_journal", [], |r| {
                r.get(0)
            })?;
        let out = f(&tx)?;
        tx.execute("DELETE FROM clip_journal WHERE seq > ?1", [head])?;
        tx.commit()?;
        Ok(out)
    }
}

impl Pipeline {
    /// Store a clip that came from another device. Dedups by content hash:
    /// an existing clip keeps its id and gets last_seen_at / copy_count =
    /// max of both. Never journaled, never touches the OS clipboard.
    /// Sensitive clips are refused.
    pub fn ingest_synced(&self, c: SyncedClip) -> Result<IngestOutcome> {
        if c.kind == Kind::Sensitive {
            return Ok(skipped("sensitive clips are never synced"));
        }
        if c.content_hash.len() != 32 {
            bail!("invalid content hash");
        }
        let max = self.config().max_text_bytes;
        if c.text.as_ref().is_some_and(|t| t.len() > max)
            || c.html.as_ref().is_some_and(|h| h.len() > max)
        {
            return Ok(skipped("synced text too large"));
        }
        if c.device_id.is_empty() || c.device_id.len() > 64 {
            bail!("invalid device id");
        }
        let store = self.store();

        // Known hash: merge, never duplicate.
        if let Some(id) = store.id_for_hash(&c.content_hash)? {
            store.quiet(|conn| {
                conn.execute(
                    "UPDATE clips SET \
                        device_id = CASE WHEN ?2 > last_seen_at THEN ?3 ELSE device_id END, \
                        source_app = CASE WHEN ?2 > last_seen_at THEN coalesce(?4, source_app) \
                                          ELSE source_app END, \
                        last_seen_at = max(last_seen_at, ?2), \
                        copy_count = max(copy_count, ?5), \
                        created_at = min(created_at, ?6) \
                     WHERE id = ?1",
                    params![
                        id,
                        c.last_seen_at,
                        c.device_id,
                        c.source_app,
                        c.copy_count,
                        c.created_at
                    ],
                )?;
                Ok(())
            })?;
            let kind = store
                .get(&id)?
                .map(|clip| clip.summary.kind)
                .unwrap_or(c.kind);
            return Ok(IngestOutcome::Bumped { id, kind });
        }

        let id = Ulid::generate().to_string();
        let mut thumb_path: Option<PathBuf> = None;
        let (mut width, mut height) = (None, None);
        if c.kind == Kind::Image {
            let Some(img) = &c.image else {
                return Ok(skipped("synced image without metadata"));
            };
            if img.width == 0
                || img.height == 0
                || img.width > MAX_SYNCED_IMAGE_SIDE
                || img.height > MAX_SYNCED_IMAGE_SIDE
            {
                return Ok(skipped("synced image has invalid size"));
            }
            width = Some(i64::from(img.width));
            height = Some(i64::from(img.height));
            if let Some(png) = img
                .thumb_png
                .as_deref()
                .filter(|b| b.len() <= MAX_SYNCED_THUMB_BYTES)
            {
                // Re-encode: only pixels we decoded ourselves hit the disk.
                match image::load_from_memory_with_format(png, ImageFormat::Png) {
                    Ok(decoded) => {
                        let path = self.thumbs_dir().join(format!("{id}.png"));
                        write_png(&decoded.to_rgba8(), &path)?;
                        thumb_path = Some(path);
                    }
                    Err(_) => tracing::debug!(%id, "synced thumbnail is not a valid PNG"),
                }
            }
        } else if c.text.as_deref().is_none_or(|t| t.trim().is_empty()) {
            return Ok(skipped("synced clip has no text"));
        }

        let text = c.text.filter(|_| c.kind != Kind::Image);
        let (char_count, line_count) = match (&text, c.kind) {
            (_, Kind::Image) | (None, _) => (0, 0),
            (Some(_), Kind::File) if !c.files.is_empty() => (
                text.as_deref().unwrap_or_default().chars().count() as i64,
                c.files.len() as i64,
            ),
            (Some(t), _) => (t.chars().count() as i64, t.lines().count().max(1) as i64),
        };
        let files = serde_json::to_string(&c.files)?;
        let thumb_str = thumb_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let inserted = store.quiet(|conn| {
            Ok(conn.execute(
                "INSERT INTO clips (id, content_hash, kind, subkind, text, html, files, \
                     image_path, thumb_path, width, height, char_count, line_count, \
                     source_app, device_id, created_at, last_seen_at, copy_count, masked, \
                     expires_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?12, ?13, ?14, \
                     ?15, ?16, ?17, 0, NULL) \
                 ON CONFLICT(content_hash) DO NOTHING",
                params![
                    id,
                    c.content_hash,
                    c.kind.as_str(),
                    c.subkind,
                    text,
                    c.html.filter(|h| !h.trim().is_empty()),
                    files,
                    thumb_str,
                    width,
                    height,
                    char_count,
                    line_count,
                    c.source_app,
                    c.device_id,
                    c.created_at.min(c.last_seen_at),
                    c.last_seen_at,
                    c.copy_count.max(1),
                ],
            )? == 1)
        })?;
        if !inserted {
            // Lost a race with an identical clip.
            remove_files(thumb_path.iter());
            let id = store
                .id_for_hash(&c.content_hash)?
                .context("clip vanished")?;
            return Ok(IngestOutcome::Bumped { id, kind: c.kind });
        }
        tracing::debug!(%id, kind = c.kind.as_str(), "synced clip stored");
        Ok(IngestOutcome::Stored { id, kind: c.kind })
    }

    /// Attach the full-size PNG of a synced image clip (fetched after the
    /// clip itself). Builds the thumbnail too when the sender sent none.
    pub fn attach_synced_image(&self, id: &str, png: &[u8]) -> Result<()> {
        if png.len() > MAX_SYNCED_IMAGE_BYTES {
            bail!("synced image too large ({} bytes)", png.len());
        }
        let Some(clip) = self.store().get(id)? else {
            bail!("no clip with id {id}");
        };
        if clip.summary.kind != Kind::Image {
            bail!("clip {id} is not an image");
        }
        let reader = ImageReader::with_format(Cursor::new(png), ImageFormat::Png);
        let (w, h) = reader
            .into_dimensions()
            .context("synced image is not a valid PNG")?;
        if w == 0 || h == 0 || w > MAX_SYNCED_IMAGE_SIDE || h > MAX_SYNCED_IMAGE_SIDE {
            bail!("synced image has invalid size");
        }

        let image_path = self.images_dir().join(format!("{id}.png"));
        write_bytes_atomic(&image_path, png)?;
        let mut new_thumb: Option<PathBuf> = None;
        if clip.summary.thumb_path.is_none() {
            let decoded = image::load_from_memory_with_format(png, ImageFormat::Png)
                .context("decode synced image")?;
            let path = self.thumbs_dir().join(format!("{id}.png"));
            write_png(&thumbnail(&decoded.to_rgba8()), &path)?;
            new_thumb = Some(path);
        }
        let image_str = image_path.to_string_lossy().into_owned();
        let thumb_str = new_thumb.as_ref().map(|p| p.to_string_lossy().into_owned());
        let updated = self.store().quiet(|conn| {
            Ok(conn.execute(
                "UPDATE clips SET image_path = ?2, thumb_path = coalesce(thumb_path, ?3), \
                     width = ?4, height = ?5 WHERE id = ?1",
                params![id, image_str, thumb_str, i64::from(w), i64::from(h)],
            )?)
        })?;
        if updated == 0 {
            // Deleted while we were writing.
            remove_files([image_path].iter().chain(new_thumb.iter()));
        }
        Ok(())
    }

    /// Delete the clip with this hash because another device deleted it at
    /// `deleted_at`. A clip copied again here after that time survives.
    /// Not journaled. Returns the deleted id.
    pub fn delete_synced(&self, content_hash: &[u8], deleted_at: Millis) -> Result<Option<String>> {
        let rows: Vec<(String, Option<String>, Option<String>)> = self.store().quiet(|conn| {
            let mut stmt = conn.prepare(
                "DELETE FROM clips WHERE content_hash = ?1 AND last_seen_at <= ?2 AND masked = 0 \
                 RETURNING id, image_path, thumb_path",
            )?;
            let rows = stmt.query_map(params![content_hash, deleted_at], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        let mut id = None;
        for (rid, a, b) in rows {
            remove_files(a.iter().chain(b.iter()));
            id = Some(rid);
        }
        Ok(id)
    }

    /// Delete a clip on this device only (retention, cleanup): other synced
    /// devices keep their copy.
    pub fn delete_local(&self, id: &str) -> Result<()> {
        let files = self.store().quiet(|conn| {
            let mut stmt =
                conn.prepare("DELETE FROM clips WHERE id = ?1 RETURNING image_path, thumb_path")?;
            let rows = stmt.query_map([id], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (a, b) = row?;
                out.extend(a);
                out.extend(b);
            }
            Ok(out)
        })?;
        remove_files(&files);
        Ok(())
    }

    fn images_dir(&self) -> PathBuf {
        self.config().data_dir.join(IMAGES_DIR)
    }

    fn thumbs_dir(&self) -> PathBuf {
        self.config().data_dir.join(THUMBS_DIR)
    }
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClearScope, PipelineConfig, RawClip, RawContent, now_millis};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> TempDir {
            let dir = std::env::temp_dir().join(format!("clipd-synced-{}", Ulid::generate()));
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

    fn text(p: &Pipeline, s: &str) -> String {
        match p
            .ingest(RawClip {
                content: RawContent::Text {
                    text: s.into(),
                    html: None,
                },
                source_app: Some("notepad.exe".into()),
                captured_at: now_millis(),
            })
            .unwrap()
        {
            IngestOutcome::Stored { id, .. } | IngestOutcome::Bumped { id, .. } => id,
            o => panic!("{o:?}"),
        }
    }

    fn remote(hash: u8, s: &str, at: Millis) -> SyncedClip {
        SyncedClip {
            content_hash: vec![hash; 32],
            kind: Kind::Text,
            subkind: None,
            text: Some(s.into()),
            html: None,
            files: vec![],
            source_app: Some("chrome.exe".into()),
            device_id: "PHONE".into(),
            created_at: at,
            last_seen_at: at,
            copy_count: 1,
            image: None,
        }
    }

    fn ops(p: &Pipeline) -> Vec<JournalOp> {
        p.store()
            .journal_since(0, 100)
            .unwrap()
            .into_iter()
            .map(|e| e.op)
            .collect()
    }

    #[test]
    fn journal_records_local_changes_only() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        let id = text(&p, "hello world");
        text(&p, "hello world"); // bump
        p.store().set_pinned(&id, true).unwrap();
        p.store().set_pinned(&id, true).unwrap(); // no change, no row
        p.delete(&id).unwrap();
        assert_eq!(
            ops(&p),
            [
                JournalOp::Upsert,
                JournalOp::Upsert,
                JournalOp::Pin,
                JournalOp::Delete
            ]
        );
        let head = p.store().journal_head().unwrap();
        assert_eq!(p.store().journal_since(head, 10).unwrap(), vec![]);

        // Sensitive clips are never journaled.
        text(&p, "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8");
        assert_eq!(p.store().journal_head().unwrap(), head);

        // Sync-applied changes are quiet.
        let r = p.ingest_synced(remote(7, "from phone", 1_000)).unwrap();
        let IngestOutcome::Stored { id: rid, .. } = r else {
            panic!("{r:?}")
        };
        p.store().set_pinned_synced(&[7; 32], true).unwrap();
        p.delete_synced(&[7; 32], 2_000).unwrap();
        let kept = text(&p, "kept elsewhere");
        let head2 = p.store().journal_head().unwrap();
        p.delete_local(&kept).unwrap();
        assert_eq!(p.store().journal_head().unwrap(), head2);
        assert!(p.store().get(&rid).unwrap().is_none());
        assert!(p.store().get(&kept).unwrap().is_none());

        assert_eq!(p.store().trim_journal(i64::MAX).unwrap(), 5);
    }

    #[test]
    fn clear_history_journal_behaviour() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        let keep = text(&p, "pinned stays");
        let a = text(&p, "clear me a");
        let b = text(&p, "clear me b");
        p.store().set_pinned(&keep, true).unwrap();
        // Masked clips are never journaled, so never tombstoned either.
        text(&p, "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8");

        // Local-only: like retention, nothing reaches the journal.
        let head = p.store().journal_head().unwrap();
        assert_eq!(p.clear_history(true, ClearScope::LocalOnly).unwrap(), 3);
        assert_eq!(p.store().journal_head().unwrap(), head);
        assert!(p.store().get(&a).unwrap().is_none());
        assert!(p.store().get(&b).unwrap().is_none());
        assert!(p.store().get(&keep).unwrap().is_some());

        // All devices: one delete tombstone per unmasked clip, none for the
        // pinned one or the masked one.
        let a2 = text(&p, "clear me a2");
        let b2 = text(&p, "clear me b2");
        text(&p, "ghp_Z9y8X7w6V5u4T3s2R1q0P9o8N7m6L5k4J3i2");
        let head2 = p.store().journal_head().unwrap();
        assert_eq!(p.clear_history(true, ClearScope::AllDevices).unwrap(), 3);
        let tombs = p.store().journal_since(head2, 100).unwrap();
        assert_eq!(tombs.len(), 2);
        assert!(tombs.iter().all(|e| e.op == JournalOp::Delete));
        let ids: Vec<_> = tombs.iter().map(|e| e.clip_id.clone()).collect();
        assert!(ids.contains(&a2) && ids.contains(&b2));
        assert!(p.store().get(&keep).unwrap().is_some());

        // Not keeping pinned: the pinned clip gets a tombstone too.
        let head3 = p.store().journal_head().unwrap();
        assert_eq!(p.clear_history(false, ClearScope::AllDevices).unwrap(), 1);
        let tombs = p.store().journal_since(head3, 10).unwrap();
        assert_eq!(tombs.len(), 1);
        assert_eq!(tombs[0].clip_id, keep);
    }

    #[test]
    fn synced_clips_merge_by_hash() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        let IngestOutcome::Stored { id, kind } = p.ingest_synced(remote(1, "abc", 500)).unwrap()
        else {
            panic!()
        };
        assert_eq!(kind, Kind::Text);
        let mut again = remote(1, "abc", 900);
        again.copy_count = 3;
        again.device_id = "LAPTOP2".into();
        assert_eq!(
            p.ingest_synced(again).unwrap(),
            IngestOutcome::Bumped {
                id: id.clone(),
                kind: Kind::Text
            }
        );
        let s = p.store().get(&id).unwrap().unwrap().summary;
        assert_eq!((s.last_seen_at, s.copy_count, s.created_at), (900, 3, 500));
        assert_eq!(s.device_id, "LAPTOP2");
        // Older event doesn't move it back.
        p.ingest_synced(remote(1, "abc", 100)).unwrap();
        let s = p.store().get(&id).unwrap().unwrap().summary;
        assert_eq!((s.last_seen_at, s.created_at), (900, 100));
        assert_eq!(s.device_id, "LAPTOP2");
        assert_eq!(
            p.store()
                .get_by_hash(&[1; 32])
                .unwrap()
                .unwrap()
                .clip
                .summary
                .id,
            id
        );

        // Searchable like a local clip.
        let hits = p
            .store()
            .list(&ListQuery {
                search: Some("abc".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 1);

        // Refused: sensitive, bad hash.
        let mut s = remote(2, "x", 1);
        s.kind = Kind::Sensitive;
        assert!(matches!(
            p.ingest_synced(s).unwrap(),
            IngestOutcome::Skipped { .. }
        ));
        let mut bad = remote(3, "x", 1);
        bad.content_hash = vec![1, 2];
        assert!(p.ingest_synced(bad).is_err());
    }

    #[test]
    fn remote_delete_respects_newer_local_copy() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        p.ingest_synced(remote(4, "abc", 5_000)).unwrap();
        assert_eq!(p.delete_synced(&[4; 32], 4_000).unwrap(), None);
        assert!(p.delete_synced(&[4; 32], 6_000).unwrap().is_some());
        assert!(p.store().get_by_hash(&[4; 32]).unwrap().is_none());
    }

    #[test]
    fn synced_images_get_thumb_then_full() {
        let dir = TempDir::new();
        let p = pipeline(&dir);
        let img = image::RgbaImage::from_fn(64, 32, |x, y| image::Rgba([x as u8, y as u8, 9, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let mut c = remote(5, "", 10);
        c.kind = Kind::Image;
        c.text = None;
        c.image = Some(SyncedImage {
            width: 64,
            height: 32,
            thumb_png: None,
        });
        let IngestOutcome::Stored { id, .. } = p.ingest_synced(c).unwrap() else {
            panic!()
        };
        let clip = p.store().get(&id).unwrap().unwrap();
        assert!(clip.image_path.is_none() && clip.summary.thumb_path.is_none());
        assert!(p.attach_synced_image(&id, b"not a png").is_err());
        p.attach_synced_image(&id, &png).unwrap();
        let clip = p.store().get(&id).unwrap().unwrap();
        let full = clip.image_path.unwrap();
        assert_eq!(fs::read(&full).unwrap(), png);
        assert_eq!(
            image::image_dimensions(clip.summary.thumb_path.unwrap()).unwrap(),
            (64, 32)
        );
        p.delete_synced(&[5; 32], i64::MAX).unwrap();
        assert!(!Path::new(&full).exists());
    }
}
