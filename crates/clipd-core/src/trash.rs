//! Trash: deleted clips you can restore for a while.
//!
//! [`Pipeline::trash`] moves a row from `clips` into the `trash` table (same
//! columns plus `deleted_at`) in one transaction. Leaving `clips` fires the
//! usual delete triggers, so search forgets it and paired devices delete it;
//! [`Pipeline::restore`] moves it back, which syncs as a new copy. Image files
//! stay on disk until the clip is purged. Masked (sensitive) clips skip the
//! trash and are deleted right away.
//!
//! Only the desktop dashboard uses the trash; other delete paths (sync,
//! retention, CLI, Android) delete permanently.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::model::{Clip, ClipSummary, Millis};
use crate::pipeline::{Pipeline, remove_files};
use crate::store::{SUMMARY_COL_COUNT, SUMMARY_COLS, Store, summary_from_row};

/// Trashed clips older than this are purged by [`Pipeline::purge_old_trash`].
pub const TRASH_KEEP_MS: Millis = 30 * 24 * 60 * 60 * 1000;

/// Every column of `clips` except `pk`, in the order both tables use.
const CLIP_COLS: &str = "id, content_hash, kind, subkind, text, html, files, image_path, \
     thumb_path, width, height, char_count, line_count, source_app, device_id, created_at, \
     last_seen_at, copy_count, pinned, masked, expires_at";

/// Migration v3 (see `store::MIGRATIONS`).
pub(crate) const SCHEMA_V3: &str = r#"
CREATE TABLE trash (
    id           TEXT    NOT NULL PRIMARY KEY,
    deleted_at   INTEGER NOT NULL,
    content_hash BLOB    NOT NULL,
    kind         TEXT    NOT NULL,
    subkind      TEXT,
    text         TEXT,
    html         TEXT,
    files        TEXT    NOT NULL DEFAULT '[]',
    image_path   TEXT,
    thumb_path   TEXT,
    width        INTEGER,
    height       INTEGER,
    char_count   INTEGER NOT NULL DEFAULT 0,
    line_count   INTEGER NOT NULL DEFAULT 0,
    source_app   TEXT,
    device_id    TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    copy_count   INTEGER NOT NULL DEFAULT 1,
    pinned       INTEGER NOT NULL DEFAULT 0,
    masked       INTEGER NOT NULL DEFAULT 0,
    expires_at   INTEGER
);
CREATE INDEX trash_deleted ON trash(deleted_at DESC);
"#;

/// A clip in the trash.
#[derive(Debug, Clone, Serialize)]
pub struct TrashedClip {
    #[serde(flatten)]
    pub summary: ClipSummary,
    pub deleted_at: Millis,
}

/// What [`Pipeline::trash`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrashOutcome {
    Trashed,
    /// Sensitive clips aren't kept: deleted for good.
    Deleted,
    Missing,
}

/// Image/thumbnail paths of trash rows matching `where_sql`, then delete them.
fn purge_where(store: &Store, where_sql: &str, arg: rusqlite::types::Value) -> Result<Vec<String>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(&format!(
        "DELETE FROM trash WHERE {where_sql} RETURNING image_path, thumb_path"
    ))?;
    let rows = stmt.query_map([arg], |r| {
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
}

impl Store {
    /// Trashed clips, most recently deleted first.
    pub fn list_trash(&self, limit: u32, before: Option<Millis>) -> Result<Vec<TrashedClip>> {
        let sql = format!(
            "SELECT {SUMMARY_COLS}, c.deleted_at FROM trash c \
             WHERE ?1 IS NULL OR c.deleted_at < ?1 ORDER BY c.deleted_at DESC, c.id DESC LIMIT ?2"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![before, limit.clamp(1, 500)], |r| {
            Ok(TrashedClip {
                summary: summary_from_row(r)?,
                deleted_at: r.get(SUMMARY_COL_COUNT)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A clip in the trash, in full.
    pub fn get_trashed(&self, id: &str) -> Result<Option<Clip>> {
        let sql = format!(
            "SELECT {SUMMARY_COLS}, c.text, c.html, c.image_path FROM trash c WHERE c.id = ?1"
        );
        Ok(self
            .conn()
            .query_row(&sql, [id], |r| {
                Ok(Clip {
                    summary: summary_from_row(r)?,
                    text: r.get(SUMMARY_COL_COUNT)?,
                    html: r.get(SUMMARY_COL_COUNT + 1)?,
                    image_path: r.get(SUMMARY_COL_COUNT + 2)?,
                })
            })
            .optional()?)
    }

    pub fn trash_count(&self) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM trash", [], |r| r.get(0))?)
    }
}

impl Pipeline {
    /// Move a clip to the trash (sensitive clips are deleted instead).
    pub fn trash(&self, id: &str) -> Result<TrashOutcome> {
        let store = self.store();
        let masked: Option<bool> = store
            .conn()
            .query_row("SELECT masked FROM clips WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        match masked {
            None => return Ok(TrashOutcome::Missing),
            Some(true) => {
                self.delete(id)?;
                return Ok(TrashOutcome::Deleted);
            }
            Some(false) => {}
        }
        let mut conn = store.conn();
        let tx = conn.transaction()?;
        tx.execute(
            &format!(
                "INSERT OR REPLACE INTO trash (deleted_at, {CLIP_COLS}) \
                 SELECT ?2, {CLIP_COLS} FROM clips WHERE id = ?1"
            ),
            params![id, crate::now_millis()],
        )?;
        tx.execute("DELETE FROM clips WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(TrashOutcome::Trashed)
    }

    /// Put a trashed clip back. Returns false if it's no longer in the trash.
    /// If the same content was copied again meanwhile, the trashed copy is
    /// dropped (the live one already has it).
    pub fn restore(&self, id: &str) -> Result<bool> {
        let store = self.store();
        let mut stale_files = Vec::new();
        {
            let mut conn = store.conn();
            let tx = conn.transaction()?;
            let Some(hash): Option<Vec<u8>> = tx
                .query_row("SELECT content_hash FROM trash WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?
            else {
                return Ok(false);
            };
            let live: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM clips WHERE content_hash = ?1 OR id = ?2)",
                params![hash, id],
                |r| r.get(0),
            )?;
            if live {
                let (a, b): (Option<String>, Option<String>) = tx.query_row(
                    "SELECT image_path, thumb_path FROM trash WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                stale_files.extend(a);
                stale_files.extend(b);
            } else {
                tx.execute(
                    &format!("INSERT INTO clips ({CLIP_COLS}) SELECT {CLIP_COLS} FROM trash WHERE id = ?1"),
                    [id],
                )?;
            }
            tx.execute("DELETE FROM trash WHERE id = ?1", [id])?;
            tx.commit()?;
        }
        remove_files(&stale_files);
        Ok(true)
    }

    /// Delete one trashed clip for good.
    pub fn purge(&self, id: &str) -> Result<()> {
        let files = purge_where(self.store(), "id = ?1", id.to_owned().into())?;
        remove_files(&files);
        Ok(())
    }

    /// Empty the trash. Returns how many clips were removed.
    pub fn empty_trash(&self) -> Result<usize> {
        let n = self.store().trash_count()? as usize;
        let files = purge_where(self.store(), "?1 = 1", 1i64.into())?;
        remove_files(&files);
        Ok(n)
    }

    /// Purge clips trashed more than [`TRASH_KEEP_MS`] ago.
    pub fn purge_old_trash(&self) -> Result<()> {
        let cutoff = crate::now_millis() - TRASH_KEEP_MS;
        let files = purge_where(self.store(), "deleted_at < ?1", cutoff.into())?;
        remove_files(&files);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{RawClip, RawContent};
    use crate::{IngestOutcome, Pipeline, PipelineConfig};

    use super::*;

    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn setup() -> (TempDir, Pipeline) {
        let dir = std::env::temp_dir().join(format!("clipd-trash-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = Pipeline::open(PipelineConfig::with_defaults(dir.clone()).unwrap()).unwrap();
        (TempDir(dir), p)
    }

    fn ingest(p: &Pipeline, content: RawContent) -> String {
        match p
            .ingest(RawClip {
                content,
                source_app: Some("test.exe".into()),
                captured_at: crate::now_millis(),
            })
            .unwrap()
        {
            IngestOutcome::Stored { id, .. } | IngestOutcome::Bumped { id, .. } => id,
            other => panic!("{other:?}"),
        }
    }

    fn text(p: &Pipeline, t: &str) -> String {
        ingest(
            p,
            RawContent::Text {
                text: t.into(),
                html: None,
            },
        )
    }

    fn search(p: &Pipeline, q: &str) -> usize {
        p.store()
            .list(&crate::ListQuery {
                search: Some(q.into()),
                ..Default::default()
            })
            .unwrap()
            .len()
    }

    #[test]
    fn trash_restore_purge() {
        let (_d, p) = setup();
        let id = text(&p, "the quick brown fox");
        assert_eq!(p.trash(&id).unwrap(), TrashOutcome::Trashed);
        assert!(p.store().get(&id).unwrap().is_none());
        assert_eq!(search(&p, "brown"), 0);
        let t = p.store().list_trash(50, None).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].summary.id, id);
        assert!(p.store().get_trashed(&id).unwrap().unwrap().text.is_some());

        assert!(p.restore(&id).unwrap());
        assert!(p.store().get(&id).unwrap().is_some());
        assert_eq!(search(&p, "brown"), 1);
        assert_eq!(p.store().trash_count().unwrap(), 0);
        assert!(!p.restore(&id).unwrap());

        p.trash(&id).unwrap();
        p.purge(&id).unwrap();
        assert_eq!(p.store().trash_count().unwrap(), 0);
        assert_eq!(p.trash("missing").unwrap(), TrashOutcome::Missing);
    }

    #[test]
    fn image_files_live_until_purge_and_recopy_wins() {
        let (_d, p) = setup();
        let rgba = vec![200u8; 8 * 6 * 4];
        let img = || RawContent::Image {
            width: 8,
            height: 6,
            rgba: rgba.clone(),
        };
        let id = ingest(&p, img());
        let thumb = p
            .store()
            .get(&id)
            .unwrap()
            .unwrap()
            .summary
            .thumb_path
            .unwrap();
        p.trash(&id).unwrap();
        assert!(std::path::Path::new(&thumb).exists());

        // Copied again while in the trash: restore keeps the live copy.
        let again = ingest(&p, img());
        assert_ne!(again, id);
        assert!(p.restore(&id).unwrap());
        assert!(!std::path::Path::new(&thumb).exists());
        assert!(p.store().get(&again).unwrap().is_some());

        p.trash(&again).unwrap();
        assert_eq!(p.empty_trash().unwrap(), 1);
        assert_eq!(std::fs::read_dir(_d.0.join("thumbs")).unwrap().count(), 0);
    }
}
