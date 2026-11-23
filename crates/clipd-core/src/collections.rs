//! Collections: folders the user makes for clips ("Work", "Recipes").
//!
//! A clip is in at most one collection (`clips.collection_id`); moving it
//! to another one takes it out of the first. Clips in a collection are kept
//! like pinned ones: retention, expiry and "Clear history (keep pinned)"
//! leave them alone. Deleting a collection only takes its clips out of it;
//! they stay in the history.
//!
//! Sync: which collection a clip is in travels with its pin state (journal
//! op `pin`, one last-writer-wins clock per clip). Collection definitions
//! journal as op `collection` (`clip_id` = the collection id) and stay as
//! tombstones after a delete, so a delete reaches devices that were off.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, Row, params};
use ulid::Ulid;

use crate::model::{Collection, Millis};
use crate::now_millis;
use crate::pipeline::Pipeline;
use crate::store::Store;

/// Longest collection name, in characters.
pub const MAX_COLLECTION_NAME: usize = 40;
/// Most collections one history can have.
pub const MAX_COLLECTIONS: usize = 100;

/// Migration v4 (see `store::MIGRATIONS`).
pub(crate) const SCHEMA_V4: &str = r#"
ALTER TABLE clips ADD COLUMN collection_id TEXT;
ALTER TABLE trash ADD COLUMN collection_id TEXT;
CREATE INDEX clips_collection ON clips(collection_id, last_seen_at DESC)
    WHERE collection_id IS NOT NULL;

CREATE TABLE collections (
    id          TEXT    NOT NULL PRIMARY KEY,
    name        TEXT    NOT NULL,
    icon        TEXT,
    color       TEXT,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    -- Last-writer-wins clock for sync: (updated_at, updated_dev).
    updated_at  INTEGER NOT NULL,
    updated_dev TEXT    NOT NULL DEFAULT '',
    deleted_at  INTEGER
);

-- Moving a clip between collections is user state, synced like a pin.
CREATE TRIGGER clip_journal_au_collection AFTER UPDATE OF collection_id ON clips
WHEN new.masked = 0 AND new.collection_id IS NOT old.collection_id BEGIN
    INSERT INTO clip_journal(op, clip_id, content_hash, at)
    VALUES ('pin', new.id, new.content_hash, CAST(unixepoch('subsec') * 1000 AS INTEGER));
END;
-- A clip restored from the trash brings its pin and collection back.
CREATE TRIGGER clip_journal_ai_state AFTER INSERT ON clips
WHEN new.masked = 0 AND (new.pinned = 1 OR new.collection_id IS NOT NULL) BEGIN
    INSERT INTO clip_journal(op, clip_id, content_hash, at)
    VALUES ('pin', new.id, new.content_hash, CAST(unixepoch('subsec') * 1000 AS INTEGER));
END;
CREATE TRIGGER collections_journal_ai AFTER INSERT ON collections BEGIN
    INSERT INTO clip_journal(op, clip_id, content_hash, at)
    VALUES ('collection', new.id, x'', new.updated_at);
END;
CREATE TRIGGER collections_journal_au AFTER UPDATE ON collections BEGIN
    INSERT INTO clip_journal(op, clip_id, content_hash, at)
    VALUES ('collection', new.id, x'', new.updated_at);
END;
"#;

/// A collection as sync sees it: tombstones included, with its clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionState {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub position: i64,
    pub created_at: Millis,
    pub updated_at: Millis,
    pub updated_dev: String,
    pub deleted: bool,
}

/// What the user can set on a collection.
#[derive(Debug, Clone, Default)]
pub struct CollectionFields {
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
}

const STATE_COLS: &str =
    "id, name, icon, color, position, created_at, updated_at, updated_dev, deleted_at";

fn state_from_row(r: &Row<'_>) -> rusqlite::Result<CollectionState> {
    Ok(CollectionState {
        id: r.get(0)?,
        name: r.get(1)?,
        icon: r.get(2)?,
        color: r.get(3)?,
        position: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        updated_dev: r.get(7)?,
        deleted: r.get::<_, Option<Millis>>(8)?.is_some(),
    })
}

/// Trimmed, single-line, 1..=40 characters.
pub fn clean_name(name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        bail!("Give the collection a name");
    }
    if name.chars().count() > MAX_COLLECTION_NAME {
        bail!("Keep the name under {MAX_COLLECTION_NAME} characters");
    }
    Ok(name)
}

/// One short emoji (a grapheme can be several code points); empty = none.
fn clean_icon(icon: Option<String>) -> Result<Option<String>> {
    let Some(icon) = icon.map(|i| i.trim().to_owned()).filter(|i| !i.is_empty()) else {
        return Ok(None);
    };
    if icon.len() > 32
        || icon
            .chars()
            .any(|c| c.is_control() || c.is_ascii_alphanumeric())
    {
        bail!("Pick one emoji as the icon");
    }
    Ok(Some(icon))
}

/// A palette key like "blue"; empty = default.
fn clean_color(color: Option<String>) -> Result<Option<String>> {
    let Some(color) = color
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty())
    else {
        return Ok(None);
    };
    if color.len() > 16 || !color.chars().all(|c| c.is_ascii_lowercase()) {
        bail!("Unknown color");
    }
    Ok(Some(color))
}

/// A clock value newer than `prev`, even when the system clock is behind.
fn stamp(prev: Millis) -> Millis {
    now_millis().max(prev + 1)
}

fn live_state(conn: &Connection, id: &str) -> Result<Option<CollectionState>> {
    Ok(conn
        .query_row(
            &format!("SELECT {STATE_COLS} FROM collections WHERE id = ?1 AND deleted_at IS NULL"),
            [id],
            state_from_row,
        )
        .optional()?)
}

/// Another live collection already uses this name (case-insensitive).
fn name_taken(conn: &Connection, name: &str, except: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM collections WHERE deleted_at IS NULL AND id <> ?2 \
         AND lower(name) = lower(?1))",
        params![name, except],
        |r| r.get(0),
    )?)
}

impl Store {
    /// Live collections in sidebar order, with how many clips each holds.
    pub fn collections(&self) -> Result<Vec<Collection>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT co.id, co.name, co.icon, co.color, co.position, co.created_at, \
                 (SELECT count(*) FROM clips c WHERE c.collection_id = co.id) \
             FROM collections co WHERE co.deleted_at IS NULL \
             ORDER BY co.position, co.created_at, co.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Collection {
                id: r.get(0)?,
                name: r.get(1)?,
                icon: r.get(2)?,
                color: r.get(3)?,
                position: r.get(4)?,
                created_at: r.get(5)?,
                count: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A live collection by id.
    pub fn collection(&self, id: &str) -> Result<Option<Collection>> {
        Ok(self.collections()?.into_iter().find(|c| c.id == id))
    }

    /// One collection for sync, tombstone or not.
    pub fn collection_state(&self, id: &str) -> Result<Option<CollectionState>> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {STATE_COLS} FROM collections WHERE id = ?1"),
                [id],
                state_from_row,
            )
            .optional()?)
    }

    /// Every live collection for sync (backfill on pairing).
    pub fn collection_states(&self) -> Result<Vec<CollectionState>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {STATE_COLS} FROM collections WHERE deleted_at IS NULL ORDER BY position"
        ))?;
        let rows = stmt.query_map([], state_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Put a clip in a collection (None = take it out). Errors when the
    /// clip or the collection doesn't exist.
    pub fn set_collection(&self, clip_id: &str, collection_id: Option<&str>) -> Result<()> {
        let conn = self.conn();
        if let Some(col) = collection_id
            && live_state(&conn, col)?.is_none()
        {
            bail!("That collection no longer exists");
        }
        let n = conn.execute(
            "UPDATE clips SET collection_id = ?2 WHERE id = ?1",
            params![clip_id, collection_id],
        )?;
        if n == 0 {
            bail!("no clip with id {clip_id}");
        }
        Ok(())
    }

    /// Set the collection of the clip with this hash, from sync (not
    /// journaled). Returns its id, or None when no (unmasked) clip has the
    /// hash. The collection may not have arrived yet; lists only show live
    /// collections.
    pub fn set_collection_synced(
        &self,
        content_hash: &[u8],
        collection_id: Option<&str>,
    ) -> Result<Option<String>> {
        self.quiet(|c| {
            Ok(c.query_row(
                "UPDATE clips SET collection_id = ?2 WHERE content_hash = ?1 AND masked = 0 \
                 RETURNING id",
                params![content_hash, collection_id],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Apply a collection from another device when its clock is newer
    /// (later `updated_at`, ties broken by device id). A tombstone also
    /// takes every clip out of it. Not journaled. Returns true if applied.
    pub fn apply_collection_synced(&self, st: &CollectionState) -> Result<bool> {
        if st.id.is_empty() || st.id.len() > 64 {
            bail!("invalid collection id");
        }
        let name = if st.deleted {
            st.name.chars().take(MAX_COLLECTION_NAME).collect()
        } else {
            clean_name(&st.name)?
        };
        let icon = clean_icon(st.icon.clone()).unwrap_or(None);
        let color = clean_color(st.color.clone()).unwrap_or(None);
        self.quiet(|c| {
            let cur: Option<(Millis, String)> = c
                .query_row(
                    "SELECT updated_at, updated_dev FROM collections WHERE id = ?1",
                    [&st.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((at, dev)) = cur
                && (st.updated_at, st.updated_dev.as_str()) <= (at, dev.as_str())
            {
                return Ok(false);
            }
            let deleted_at = st.deleted.then_some(st.updated_at);
            c.execute(
                "INSERT INTO collections (id, name, icon, color, position, created_at, \
                     updated_at, updated_dev, deleted_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                 ON CONFLICT(id) DO UPDATE SET name = ?2, icon = ?3, color = ?4, \
                     position = ?5, updated_at = ?7, updated_dev = ?8, deleted_at = ?9",
                params![
                    st.id,
                    name,
                    icon,
                    color,
                    st.position,
                    st.created_at,
                    st.updated_at,
                    st.updated_dev,
                    deleted_at
                ],
            )?;
            if st.deleted {
                c.execute(
                    "UPDATE clips SET collection_id = NULL WHERE collection_id = ?1",
                    [&st.id],
                )?;
            }
            Ok(true)
        })
    }
}

impl Pipeline {
    /// Make a new collection at the end of the list.
    pub fn create_collection(&self, f: CollectionFields) -> Result<Collection> {
        let name = clean_name(&f.name)?;
        let icon = clean_icon(f.icon)?;
        let color = clean_color(f.color)?;
        let id = Ulid::generate().to_string();
        let dev = self.config().device_id.clone();
        {
            let mut conn = self.store().conn();
            let tx = conn.transaction()?;
            let live: i64 = tx.query_row(
                "SELECT count(*) FROM collections WHERE deleted_at IS NULL",
                [],
                |r| r.get(0),
            )?;
            if live as usize >= MAX_COLLECTIONS {
                bail!("You can have up to {MAX_COLLECTIONS} collections");
            }
            if name_taken(&tx, &name, "")? {
                bail!("You already have a collection called \"{name}\"");
            }
            let position: i64 = tx.query_row(
                "SELECT coalesce(max(position), -1) + 1 FROM collections WHERE deleted_at IS NULL",
                [],
                |r| r.get(0),
            )?;
            let now = now_millis();
            tx.execute(
                "INSERT INTO collections (id, name, icon, color, position, created_at, \
                     updated_at, updated_dev) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)",
                params![id, name, icon, color, position, now, dev],
            )?;
            tx.commit()?;
        }
        self.store()
            .collection(&id)?
            .context("collection vanished after insert")
    }

    /// Rename a collection or change its icon / color.
    pub fn update_collection(&self, id: &str, f: CollectionFields) -> Result<Collection> {
        let name = clean_name(&f.name)?;
        let icon = clean_icon(f.icon)?;
        let color = clean_color(f.color)?;
        let dev = self.config().device_id.clone();
        {
            let mut conn = self.store().conn();
            let tx = conn.transaction()?;
            let cur = live_state(&tx, id)?.context("That collection no longer exists")?;
            if name_taken(&tx, &name, id)? {
                bail!("You already have a collection called \"{name}\"");
            }
            if (cur.name.as_str(), &cur.icon, &cur.color) != (name.as_str(), &icon, &color) {
                tx.execute(
                    "UPDATE collections SET name = ?2, icon = ?3, color = ?4, updated_at = ?5, \
                         updated_dev = ?6 WHERE id = ?1",
                    params![id, name, icon, color, stamp(cur.updated_at), dev],
                )?;
            }
            tx.commit()?;
        }
        self.store()
            .collection(id)?
            .context("That collection no longer exists")
    }

    /// Put collections in this order (ids not listed keep their place
    /// after the listed ones).
    pub fn reorder_collections(&self, ids: &[String]) -> Result<()> {
        let dev = self.config().device_id.clone();
        let mut conn = self.store().conn();
        let tx = conn.transaction()?;
        let mut order: Vec<String> = ids.to_vec();
        let mut stmt = tx.prepare(
            "SELECT id FROM collections WHERE deleted_at IS NULL ORDER BY position, created_at, id",
        )?;
        let all: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        order.retain(|id| all.contains(id));
        order.dedup();
        for id in all {
            if !order.contains(&id) {
                order.push(id);
            }
        }
        for (pos, id) in order.iter().enumerate() {
            let cur = live_state(&tx, id)?.context("collection vanished")?;
            if cur.position != pos as i64 {
                tx.execute(
                    "UPDATE collections SET position = ?2, updated_at = ?3, updated_dev = ?4 \
                     WHERE id = ?1",
                    params![id, pos as i64, stamp(cur.updated_at), dev],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete a collection. Its clips stay in the history, outside any
    /// collection. Returns how many clips were in it.
    pub fn delete_collection(&self, id: &str) -> Result<usize> {
        let dev = self.config().device_id.clone();
        let mut conn = self.store().conn();
        let tx = conn.transaction()?;
        let cur = live_state(&tx, id)?.context("That collection no longer exists")?;
        let at = stamp(cur.updated_at);
        let released = tx.execute(
            "UPDATE clips SET collection_id = NULL WHERE collection_id = ?1",
            [id],
        )?;
        tx.execute(
            "UPDATE collections SET deleted_at = ?2, updated_at = ?2, updated_dev = ?3 \
             WHERE id = ?1",
            params![id, at, dev],
        )?;
        tx.commit()?;
        Ok(released)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{IngestOutcome, ListQuery, RawClip, RawContent};
    use crate::pipeline::PipelineConfig;
    use crate::synced::JournalOp;

    fn pipeline() -> (Pipeline, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("clipd-col-{}", Ulid::generate()));
        let p = Pipeline::open(PipelineConfig::with_defaults(dir.clone()).unwrap()).unwrap();
        (p, dir)
    }

    fn text(p: &Pipeline, s: &str) -> String {
        match p
            .ingest(RawClip {
                content: RawContent::Text {
                    text: s.into(),
                    html: None,
                },
                source_app: None,
                captured_at: now_millis(),
                source_url: None,
            })
            .unwrap()
        {
            IngestOutcome::Stored { id, .. } | IngestOutcome::Bumped { id, .. } => id,
            o => panic!("{o:?}"),
        }
    }

    fn fields(name: &str) -> CollectionFields {
        CollectionFields {
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn create_rename_reorder_delete() {
        let (p, dir) = pipeline();
        let work = p
            .create_collection(CollectionFields {
                name: "  Work   stuff ".into(),
                icon: Some("💼".into()),
                color: Some("Blue".into()),
            })
            .unwrap();
        assert_eq!(work.name, "Work stuff");
        assert_eq!(work.color.as_deref(), Some("blue"));
        let home = p.create_collection(fields("Home")).unwrap();
        assert_eq!((work.position, home.position), (0, 1));

        // Names are unique (case-insensitive) and checked.
        assert!(p.create_collection(fields("home")).is_err());
        assert!(p.create_collection(fields("   ")).is_err());
        assert!(p.create_collection(fields(&"x".repeat(41))).is_err());
        assert!(
            p.create_collection(CollectionFields {
                name: "Odd".into(),
                icon: Some("ab".into()),
                color: None
            })
            .is_err()
        );

        let renamed = p.update_collection(&home.id, fields("Personal")).unwrap();
        assert_eq!(renamed.name, "Personal");
        assert!(p.update_collection(&home.id, fields("work stuff")).is_err());

        p.reorder_collections(std::slice::from_ref(&home.id))
            .unwrap();
        let names: Vec<_> = p
            .store()
            .collections()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["Personal", "Work stuff"]);

        // Deleting keeps the clips, outside any collection.
        let a = text(&p, "alpha");
        p.store().set_collection(&a, Some(&work.id)).unwrap();
        assert_eq!(p.store().collection(&work.id).unwrap().unwrap().count, 1);
        assert_eq!(p.delete_collection(&work.id).unwrap(), 1);
        assert!(p.store().collection(&work.id).unwrap().is_none());
        assert!(
            p.store()
                .collection_state(&work.id)
                .unwrap()
                .unwrap()
                .deleted
        );
        let clip = p.store().get(&a).unwrap().unwrap();
        assert_eq!(clip.summary.collection_id, None);
        assert!(p.store().set_collection(&a, Some(&work.id)).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn move_filter_and_keep() {
        let (p, dir) = pipeline();
        let work = p.create_collection(fields("Work")).unwrap();
        let home = p.create_collection(fields("Home")).unwrap();
        let a = text(&p, "alpha");
        let _b = text(&p, "beta");
        p.store().set_collection(&a, Some(&work.id)).unwrap();
        let in_work = |p: &Pipeline, id: &str| {
            p.store()
                .list(&ListQuery {
                    collection_id: Some(id.into()),
                    ..Default::default()
                })
                .unwrap()
                .len()
        };
        assert_eq!((in_work(&p, &work.id), in_work(&p, &home.id)), (1, 0));
        // Moving takes it out of the first collection.
        p.store().set_collection(&a, Some(&home.id)).unwrap();
        assert_eq!((in_work(&p, &work.id), in_work(&p, &home.id)), (0, 1));

        // Kept by "clear history (keep pinned)" and by expiry.
        assert_eq!(p.store().count_clearable(true).unwrap(), 1);
        p.store().clear(true).unwrap();
        assert!(p.store().get(&a).unwrap().is_some());
        p.store()
            .conn()
            .execute("UPDATE clips SET expires_at = 1", [])
            .unwrap();
        p.store().prune_expired(now_millis()).unwrap();
        assert!(p.store().get(&a).unwrap().is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn changes_are_journaled_and_synced_ones_are_quiet() {
        let (p, dir) = pipeline();
        let head = p.store().journal_head().unwrap();
        let work = p.create_collection(fields("Work")).unwrap();
        let a = text(&p, "alpha");
        p.store().set_collection(&a, Some(&work.id)).unwrap();
        p.store().set_collection(&a, Some(&work.id)).unwrap(); // no change, no row
        let ops: Vec<_> = p
            .store()
            .journal_since(head, 100)
            .unwrap()
            .into_iter()
            .map(|e| (e.op, e.clip_id))
            .collect();
        assert_eq!(
            ops,
            [
                (JournalOp::Collection, work.id.clone()),
                (JournalOp::Upsert, a.clone()),
                (JournalOp::Pin, a.clone()),
            ]
        );

        let head = p.store().journal_head().unwrap();
        let mut st = p.store().collection_state(&work.id).unwrap().unwrap();
        // Older or equal clocks lose.
        assert!(!p.store().apply_collection_synced(&st).unwrap());
        st.updated_at += 10;
        st.updated_dev = "other".into();
        st.name = "Office".into();
        assert!(p.store().apply_collection_synced(&st).unwrap());
        assert_eq!(
            p.store().collection(&work.id).unwrap().unwrap().name,
            "Office"
        );
        // A remote tombstone takes clips out, quietly.
        st.updated_at += 10;
        st.deleted = true;
        assert!(p.store().apply_collection_synced(&st).unwrap());
        assert_eq!(
            p.store().get(&a).unwrap().unwrap().summary.collection_id,
            None
        );
        let hash = p.store().content_hash(&a).unwrap().unwrap();
        p.store().set_collection_synced(&hash, Some("X")).unwrap();
        assert!(p.store().journal_since(head, 100).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn trash_keeps_the_collection() {
        let (p, dir) = pipeline();
        let work = p.create_collection(fields("Work")).unwrap();
        let a = text(&p, "alpha");
        p.store().set_collection(&a, Some(&work.id)).unwrap();
        p.trash(&a).unwrap();
        let head = p.store().journal_head().unwrap();
        p.restore(&a).unwrap();
        let clip = p.store().get(&a).unwrap().unwrap();
        assert_eq!(
            clip.summary.collection_id.as_deref(),
            Some(work.id.as_str())
        );
        // The restore syncs the collection too.
        assert!(
            p.store()
                .journal_since(head, 100)
                .unwrap()
                .iter()
                .any(|e| e.op == JournalOp::Pin)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
