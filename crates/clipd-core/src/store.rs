//! SQLite storage + full-text search.
//!
//! One `clips` table plus an FTS5 trigram index (`clips_fts`) over text,
//! file paths and source app, kept in sync by triggers. Masked (sensitive)
//! clips are never written to the index. Schema changes go through
//! [`MIGRATIONS`], tracked with `PRAGMA user_version`.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::model::*;

/// A clip ready to persist, built by the pipeline.
#[derive(Debug, Clone)]
pub struct NewClip {
    pub id: String,
    /// SHA-256 of the normalized content (dedup key).
    pub content_hash: Vec<u8>,
    pub kind: Kind,
    pub subkind: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub files: Vec<String>,
    pub image_path: Option<String>,
    pub thumb_path: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub char_count: i64,
    pub line_count: i64,
    pub source_app: Option<String>,
    pub device_id: String,
    pub created_at: Millis,
    pub masked: bool,
    pub expires_at: Option<Millis>,
}

/// Placeholder shown instead of the body of a masked clip.
pub const MASKED_PREVIEW: &str = "••••••";
/// Characters of text kept in [`ClipSummary::preview`].
pub const PREVIEW_CHARS: usize = 400;
/// Default and maximum row counts for [`Store::list`].
pub const DEFAULT_LIST_LIMIT: u32 = 100;
pub const MAX_LIST_LIMIT: u32 = 1000;

/// Ordered schema migrations; entry `i` upgrades `user_version` i → i+1.
const MIGRATIONS: &[&str] = &[SCHEMA_V1];

const SCHEMA_V1: &str = r#"
CREATE TABLE clips (
    pk           INTEGER PRIMARY KEY,
    id           TEXT    NOT NULL UNIQUE,
    content_hash BLOB    NOT NULL UNIQUE,
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
CREATE INDEX clips_last_seen ON clips(last_seen_at DESC);
CREATE INDEX clips_kind_last_seen ON clips(kind, last_seen_at DESC);
CREATE INDEX clips_expires ON clips(expires_at) WHERE expires_at IS NOT NULL;

CREATE VIRTUAL TABLE clips_fts USING fts5(
    text, files, source_app,
    content = 'clips', content_rowid = 'pk',
    tokenize = 'trigram'
);

-- Only unmasked rows are indexed; deletes mirror that condition so the
-- external-content index never sees a 'delete' for a row it doesn't hold.
CREATE TRIGGER clips_fts_ai AFTER INSERT ON clips WHEN new.masked = 0 BEGIN
    INSERT INTO clips_fts(rowid, text, files, source_app)
    VALUES (new.pk, new.text, new.files, new.source_app);
END;
CREATE TRIGGER clips_fts_ad AFTER DELETE ON clips WHEN old.masked = 0 BEGIN
    INSERT INTO clips_fts(clips_fts, rowid, text, files, source_app)
    VALUES ('delete', old.pk, old.text, old.files, old.source_app);
END;
CREATE TRIGGER clips_fts_au AFTER UPDATE OF text, files, source_app, masked ON clips BEGIN
    INSERT INTO clips_fts(clips_fts, rowid, text, files, source_app)
    SELECT 'delete', old.pk, old.text, old.files, old.source_app WHERE old.masked = 0;
    INSERT INTO clips_fts(rowid, text, files, source_app)
    SELECT new.pk, new.text, new.files, new.source_app WHERE new.masked = 0;
END;
"#;

/// Columns read into a [`ClipSummary`] (see [`summary_from_row`]). Masked
/// rows never have their text read for lists.
const SUMMARY_COLS: &str = "c.id, c.kind, c.subkind, \
     CASE WHEN c.masked THEN NULL ELSE substr(c.text, 1, 401) END, \
     c.char_count, c.line_count, c.files, c.thumb_path, c.width, c.height, \
     c.source_app, c.device_id, c.created_at, c.last_seen_at, c.copy_count, \
     c.pinned, c.masked, c.expires_at";
const SUMMARY_COL_COUNT: usize = 18;

/// What a repeat copy updates on the existing row.
const BUMP_SET: &str = "last_seen_at = ?2, \
     copy_count = copy_count + 1, \
     source_app = coalesce(?3, source_app), \
     device_id = ?4, \
     expires_at = CASE WHEN expires_at IS NULL OR ?5 IS NULL THEN expires_at \
                       ELSE max(expires_at, ?5) END";

/// Thread-safe handle (internally a mutex-guarded connection), cheap to share
/// behind an `Arc`.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Open (creating + migrating if needed) the database at `path`.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create directory {}", parent.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("open database {}", path.display()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.busy_timeout(Duration::from_secs(5))?;
        // In-memory databases answer "memory"; that's fine.
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;")?;
        migrate(&conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock can't leave SQLite half-written
        // (statements are atomic), so a poisoned lock is safe to reuse.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Schema version of the open database.
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Insert, or if `content_hash` already exists bump that row's
    /// last_seen_at = clip.created_at and copy_count += 1.
    /// Returns (id, inserted?).
    pub fn upsert(&self, clip: NewClip) -> Result<(String, bool)> {
        let files = serde_json::to_string(&clip.files)?;
        // In the DO UPDATE clause unqualified columns are the existing row, so
        // BUMP_SET (?1..=?5) is shared with `bump_existing`.
        let sql = format!(
            "INSERT INTO clips (id, content_hash, kind, subkind, text, html, files, \
                 image_path, thumb_path, width, height, char_count, line_count, \
                 source_app, device_id, created_at, last_seen_at, masked, expires_at) \
             VALUES (?6, ?1, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
                 ?3, ?4, ?2, ?2, ?18, ?5) \
             ON CONFLICT(content_hash) DO UPDATE SET {BUMP_SET} \
             RETURNING id"
        );
        let id: String = self.conn().query_row(
            &sql,
            params![
                clip.content_hash,
                clip.created_at,
                clip.source_app,
                clip.device_id,
                clip.expires_at,
                clip.id,
                clip.kind.as_str(),
                clip.subkind,
                clip.text,
                clip.html,
                files,
                clip.image_path,
                clip.thumb_path,
                clip.width,
                clip.height,
                clip.char_count,
                clip.line_count,
                clip.masked,
            ],
            |r| r.get(0),
        )?;
        let inserted = id == clip.id;
        Ok((id, inserted))
    }

    /// Id of the clip with this content hash, if any.
    pub fn id_for_hash(&self, content_hash: &[u8]) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id FROM clips WHERE content_hash = ?1",
                [content_hash],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Bump an existing clip (same effect as an `upsert` that hits a
    /// duplicate) without building a full [`NewClip`]. Returns its id, or
    /// None if no clip has this hash.
    pub(crate) fn bump_existing(
        &self,
        content_hash: &[u8],
        seen_at: Millis,
        source_app: Option<&str>,
        device_id: &str,
        expires_at: Option<Millis>,
    ) -> Result<Option<String>> {
        let sql = format!("UPDATE clips SET {BUMP_SET} WHERE content_hash = ?1 RETURNING id");
        Ok(self
            .conn()
            .query_row(
                &sql,
                params![content_hash, seen_at, source_app, device_id, expires_at],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn list(&self, q: &ListQuery) -> Result<Vec<ClipSummary>> {
        let mut sql = format!("SELECT {SUMMARY_COLS} FROM clips c WHERE 1 = 1");
        let mut args: Vec<Value> = Vec::new();

        if let Some(kind) = q.kind {
            sql.push_str(" AND c.kind = ?");
            args.push(Value::Text(kind.as_str().to_owned()));
        }
        if q.pinned_only {
            sql.push_str(" AND c.pinned = 1");
        }
        if let Some(app) = &q.source_app {
            sql.push_str(" AND c.source_app = ?");
            args.push(Value::Text(app.clone()));
        }
        if let Some(from) = q.from {
            sql.push_str(" AND c.last_seen_at >= ?");
            args.push(Value::Integer(from));
        }
        if let Some(to) = q.to {
            sql.push_str(" AND c.last_seen_at < ?");
            args.push(Value::Integer(to));
        }
        match (q.before, q.before_id.as_deref()) {
            (Some(before), Some(before_id)) => {
                // Exact keyset: also continue past rows sharing the timestamp.
                sql.push_str(" AND (c.last_seen_at < ? OR (c.last_seen_at = ? AND c.id < ?))");
                args.push(Value::Integer(before));
                args.push(Value::Integer(before));
                args.push(Value::Text(before_id.to_owned()));
            }
            (Some(before), None) => {
                sql.push_str(" AND c.last_seen_at < ?");
                args.push(Value::Integer(before));
            }
            (None, _) => {}
        }
        if let Some(search) = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            // Belt and braces: masked rows aren't indexed, and LIKE must skip them too.
            sql.push_str(" AND c.masked = 0");
            if search.chars().count() >= 3 {
                sql.push_str(" AND c.pk IN (SELECT rowid FROM clips_fts WHERE clips_fts MATCH ?)");
                args.push(Value::Text(fts_phrase(search)));
            } else {
                // Trigrams can't match fewer than 3 chars: scan instead.
                sql.push_str(" AND (c.text LIKE ? ESCAPE '\\' OR c.source_app LIKE ? ESCAPE '\\')");
                let pattern = like_pattern(search);
                args.push(Value::Text(pattern.clone()));
                args.push(Value::Text(pattern));
            }
        }
        // Timeline order even for searches: results line up with the history
        // the user scrolls, and keyset pagination via `before` stays valid.
        sql.push_str(" ORDER BY c.last_seen_at DESC, c.id DESC LIMIT ?");
        let limit = q.limit.unwrap_or(DEFAULT_LIST_LIMIT).min(MAX_LIST_LIMIT);
        args.push(Value::Integer(i64::from(limit)));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args), summary_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get(&self, id: &str) -> Result<Option<Clip>> {
        let sql = format!(
            "SELECT {SUMMARY_COLS}, c.text, c.html, c.image_path FROM clips c WHERE c.id = ?1"
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

    /// Ids starting with `prefix` (case-insensitive, ULIDs are uppercase),
    /// newest first, at most `limit`. Lets CLI users type a short id.
    pub fn ids_with_prefix(&self, prefix: &str, limit: u32) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id FROM clips WHERE id LIKE ?1 ESCAPE '\\' \
             ORDER BY last_seen_at DESC LIMIT ?2",
        )?;
        let pattern = format!("{}%", like_escape(&prefix.to_uppercase()));
        let rows = stmt.query_map(params![pattern, limit], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Errors if no clip has this id.
    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE clips SET pinned = ?2 WHERE id = ?1",
            params![id, pinned],
        )?;
        if n == 0 {
            bail!("no clip with id {id}");
        }
        Ok(())
    }

    /// Delete a clip. Returns paths of image/thumbnail files the caller
    /// should remove from disk. Deleting an unknown id is a no-op.
    pub fn delete(&self, id: &str) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("DELETE FROM clips WHERE id = ?1 RETURNING image_path, thumb_path")?;
        let rows = stmt.query_map([id], file_pair)?;
        collect_paths(rows)
    }

    pub fn stats(&self) -> Result<Stats> {
        let conn = self.conn();
        let total = conn.query_row("SELECT count(*) FROM clips", [], |r| r.get(0))?;
        let pinned = conn.query_row("SELECT count(*) FROM clips WHERE pinned = 1", [], |r| {
            r.get(0)
        })?;
        let by_kind = conn
            .prepare("SELECT kind, count(*) AS n FROM clips GROUP BY kind ORDER BY n DESC, kind")?
            .query_map([], |r| {
                Ok((kind_from_db(&r.get::<_, String>(0)?), r.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let top_apps = conn
            .prepare(
                "SELECT source_app, count(*) AS n FROM clips WHERE source_app IS NOT NULL \
                 GROUP BY source_app ORDER BY n DESC, source_app LIMIT 10",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Stats {
            total,
            by_kind,
            pinned,
            top_apps,
        })
    }

    /// Per-local-day counts over last_seen_at in [from, to).
    pub fn day_counts(&self, from: Millis, to: Millis) -> Result<Vec<DayCount>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT date(last_seen_at / 1000, 'unixepoch', 'localtime') AS day, count(*) \
             FROM clips WHERE last_seen_at >= ?1 AND last_seen_at < ?2 \
             GROUP BY day ORDER BY day",
        )?;
        let rows = stmt.query_map([from, to], |r| {
            Ok(DayCount {
                day: r.get(0)?,
                count: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Delete clips whose expires_at <= now. Returns file paths to remove.
    pub fn prune_expired(&self, now: Millis) -> Result<Vec<String>> {
        Ok(self.prune_expired_counted(now)?.1)
    }

    /// Like [`Store::prune_expired`], also returning how many rows went.
    /// Pinned clips are never pruned.
    pub fn prune_expired_counted(&self, now: Millis) -> Result<(usize, Vec<String>)> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "DELETE FROM clips WHERE expires_at IS NOT NULL AND expires_at <= ?1 AND pinned = 0 \
             RETURNING image_path, thumb_path",
        )?;
        let pairs = stmt
            .query_map([now], file_pair)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let count = pairs.len();
        let paths = pairs
            .into_iter()
            .flat_map(|(a, b)| [a, b])
            .flatten()
            .collect();
        Ok((count, paths))
    }

    /// Distinct source apps, most used first.
    pub fn source_apps(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT source_app FROM clips WHERE source_app IS NOT NULL \
             GROUP BY source_app ORDER BY count(*) DESC, source_app",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let latest = MIGRATIONS.len() as i64;
    if version > latest {
        bail!("database schema v{version} is newer than this build supports (v{latest})");
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)
            .with_context(|| format!("migrate schema to v{}", i + 1))?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

fn summary_from_row(r: &Row<'_>) -> rusqlite::Result<ClipSummary> {
    let masked: bool = r.get(16)?;
    let (preview, truncated) = if masked {
        (MASKED_PREVIEW.to_owned(), false)
    } else {
        make_preview(r.get::<_, Option<String>>(3)?.unwrap_or_default())
    };
    let files: String = r.get(6)?;
    Ok(ClipSummary {
        id: r.get(0)?,
        kind: kind_from_db(&r.get::<_, String>(1)?),
        subkind: r.get(2)?,
        preview,
        truncated,
        char_count: r.get(4)?,
        line_count: r.get(5)?,
        files: serde_json::from_str(&files).unwrap_or_default(),
        thumb_path: r.get(7)?,
        width: r.get(8)?,
        height: r.get(9)?,
        source_app: r.get(10)?,
        device_id: r.get(11)?,
        created_at: r.get(12)?,
        last_seen_at: r.get(13)?,
        copy_count: r.get(14)?,
        pinned: r.get(15)?,
        masked,
        expires_at: r.get(17)?,
    })
}

/// First [`PREVIEW_CHARS`] chars of `text` (char-boundary safe).
fn make_preview(mut text: String) -> (String, bool) {
    match text.char_indices().nth(PREVIEW_CHARS) {
        Some((cut, _)) => {
            text.truncate(cut);
            (text, true)
        }
        None => (text, false),
    }
}

fn kind_from_db(s: &str) -> Kind {
    Kind::parse(s).unwrap_or(Kind::Text)
}

type FilePair = (Option<String>, Option<String>);

fn file_pair(r: &Row<'_>) -> rusqlite::Result<FilePair> {
    Ok((r.get(0)?, r.get(1)?))
}

fn collect_paths(rows: impl Iterator<Item = rusqlite::Result<FilePair>>) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for row in rows {
        let (a, b) = row?;
        out.extend(a);
        out.extend(b);
    }
    Ok(out)
}

/// User text as a single FTS5 phrase. Inside double quotes everything is
/// literal except `"`, which is escaped by doubling, so no query syntax
/// (operators, column filters, `*`, `NEAR`) can be injected.
fn fts_phrase(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// `%s%` for LIKE with `\` as the escape character.
fn like_pattern(s: &str) -> String {
    format!("%{}%", like_escape(s))
}

/// Escape LIKE wildcards (`%`, `_`) and the escape char itself.
fn like_escape(s: &str) -> String {
    let mut p = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            p.push('\\');
        }
        p.push(ch);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_clip(id: &str, text: &str, at: Millis) -> NewClip {
        NewClip {
            id: id.to_owned(),
            content_hash: format!("hash:{text}").into_bytes(),
            kind: Kind::Text,
            subkind: None,
            text: Some(text.to_owned()),
            html: None,
            files: vec![],
            image_path: None,
            thumb_path: None,
            width: None,
            height: None,
            char_count: text.chars().count() as i64,
            line_count: text.lines().count() as i64,
            source_app: Some("notepad.exe".to_owned()),
            device_id: "dev1".to_owned(),
            created_at: at,
            masked: false,
            expires_at: None,
        }
    }

    fn ids(rows: &[ClipSummary]) -> Vec<&str> {
        rows.iter().map(|r| r.id.as_str()).collect()
    }

    fn search(store: &Store, s: &str) -> Vec<String> {
        store
            .list(&ListQuery {
                search: Some(s.to_owned()),
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect()
    }

    #[test]
    fn migrates_to_latest_and_reopens() {
        let dir = std::env::temp_dir().join(format!("clipd-store-{}", ulid::Ulid::generate()));
        let path = dir.join("clips.db");
        {
            let store = Store::open(&path).unwrap();
            assert_eq!(store.schema_version().unwrap(), MIGRATIONS.len() as i64);
            store.upsert(text_clip("a", "hello", 1)).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store.get("a").unwrap().unwrap().text.as_deref(),
            Some("hello")
        );
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn upsert_dedups_and_bumps() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(
            store.upsert(text_clip("a", "hello", 100)).unwrap(),
            ("a".to_owned(), true)
        );
        let mut again = text_clip("b", "hello", 200);
        again.source_app = Some("code.exe".to_owned());
        again.device_id = "dev2".to_owned();
        assert_eq!(store.upsert(again).unwrap(), ("a".to_owned(), false));

        let clip = store.get("a").unwrap().unwrap().summary;
        assert_eq!(clip.copy_count, 2);
        assert_eq!(clip.created_at, 100);
        assert_eq!(clip.last_seen_at, 200);
        assert_eq!(clip.source_app.as_deref(), Some("code.exe"));
        assert_eq!(clip.device_id, "dev2");
        assert!(store.get("b").unwrap().is_none());
        assert_eq!(store.list(&ListQuery::default()).unwrap().len(), 1);
        // The re-indexed row is still searchable under its new source app.
        assert_eq!(search(&store, "code.exe"), vec!["a"]);
        assert!(search(&store, "notepad").is_empty());
    }

    #[test]
    fn bump_existing_only_touches_known_hashes() {
        let store = Store::open_in_memory().unwrap();
        store.upsert(text_clip("a", "x", 1)).unwrap();
        let hash = b"hash:x".to_vec();
        assert_eq!(
            store.bump_existing(&hash, 5, None, "dev1", None).unwrap(),
            Some("a".to_owned())
        );
        assert_eq!(
            store.bump_existing(b"nope", 5, None, "d", None).unwrap(),
            None
        );
        let s = store.get("a").unwrap().unwrap().summary;
        assert_eq!((s.copy_count, s.last_seen_at), (2, 5));
        assert_eq!(s.source_app.as_deref(), Some("notepad.exe"));
        assert_eq!(store.id_for_hash(&hash).unwrap().as_deref(), Some("a"));
    }

    #[test]
    fn list_filters_and_orders() {
        let store = Store::open_in_memory().unwrap();
        store.upsert(text_clip("t1", "one", 10)).unwrap();
        let mut link = text_clip("l1", "https://example.com", 20);
        link.kind = Kind::Link;
        link.subkind = Some("example.com".into());
        link.source_app = Some("chrome.exe".into());
        store.upsert(link).unwrap();
        store.upsert(text_clip("t2", "two", 30)).unwrap();
        store.set_pinned("t1", true).unwrap();

        let all = store.list(&ListQuery::default()).unwrap();
        assert_eq!(ids(&all), ["t2", "l1", "t1"]);

        let q = |q: ListQuery| ids(&store.list(&q).unwrap()).join(",");
        assert_eq!(
            q(ListQuery {
                kind: Some(Kind::Link),
                ..Default::default()
            }),
            "l1"
        );
        assert_eq!(
            q(ListQuery {
                pinned_only: true,
                ..Default::default()
            }),
            "t1"
        );
        assert_eq!(
            q(ListQuery {
                source_app: Some("chrome.exe".into()),
                ..Default::default()
            }),
            "l1"
        );
        assert_eq!(
            q(ListQuery {
                from: Some(10),
                to: Some(30),
                ..Default::default()
            }),
            "l1,t1"
        );
        assert_eq!(
            q(ListQuery {
                limit: Some(1),
                ..Default::default()
            }),
            "t2"
        );
        assert_eq!(store.source_apps().unwrap(), ["notepad.exe", "chrome.exe"]);

        store.set_pinned("t1", false).unwrap();
        assert!(store.set_pinned("missing", true).is_err());
    }

    #[test]
    fn keyset_pagination() {
        let store = Store::open_in_memory().unwrap();
        for i in 0..25 {
            store
                .upsert(text_clip(&format!("c{i:02}"), &format!("clip {i}"), i))
                .unwrap();
        }
        let mut seen = Vec::new();
        let mut before = None;
        loop {
            let page = store
                .list(&ListQuery {
                    before,
                    limit: Some(10),
                    ..Default::default()
                })
                .unwrap();
            if page.is_empty() {
                break;
            }
            before = Some(page.last().unwrap().last_seen_at);
            seen.extend(page.into_iter().map(|r| r.id));
        }
        let expected: Vec<String> = (0..25).rev().map(|i| format!("c{i:02}")).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn keyset_pagination_with_equal_timestamps() {
        let store = Store::open_in_memory().unwrap();
        // 12 clips copied in the same millisecond must not be skipped at a page edge.
        for i in 0..12 {
            store
                .upsert(text_clip(&format!("c{i:02}"), &format!("same ms {i}"), 7))
                .unwrap();
        }
        let mut seen = Vec::new();
        let (mut before, mut before_id) = (None, None);
        loop {
            let page = store
                .list(&ListQuery {
                    before,
                    before_id: before_id.clone(),
                    limit: Some(5),
                    ..Default::default()
                })
                .unwrap();
            let Some(last) = page.last() else { break };
            before = Some(last.last_seen_at);
            before_id = Some(last.id.clone());
            seen.extend(page.into_iter().map(|r| r.id));
        }
        let expected: Vec<String> = (0..12).rev().map(|i| format!("c{i:02}")).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn limit_is_capped() {
        let store = Store::open_in_memory().unwrap();
        for i in 0..1005 {
            store
                .upsert(text_clip(&format!("c{i}"), &format!("n{i}"), i))
                .unwrap();
        }
        let q = |limit| {
            store
                .list(&ListQuery {
                    limit,
                    ..Default::default()
                })
                .unwrap()
                .len()
        };
        assert_eq!(q(None), 100);
        assert_eq!(q(Some(5000)), 1000);
    }

    #[test]
    fn preview_is_truncated_on_char_boundary() {
        let store = Store::open_in_memory().unwrap();
        let long: String = "é".repeat(450);
        store.upsert(text_clip("long", &long, 1)).unwrap();
        store.upsert(text_clip("short", "short", 2)).unwrap();
        let rows = store.list(&ListQuery::default()).unwrap();
        let long_row = rows.iter().find(|r| r.id == "long").unwrap();
        assert!(long_row.truncated);
        assert_eq!(long_row.preview.chars().count(), PREVIEW_CHARS);
        let short_row = rows.iter().find(|r| r.id == "short").unwrap();
        assert!(!short_row.truncated);
        assert_eq!(short_row.preview, "short");
        assert_eq!(store.get("long").unwrap().unwrap().text.unwrap(), long);
    }

    #[test]
    fn search_substring_case_insensitive() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert(text_clip("a", "Hello World from Rust", 1))
            .unwrap();
        store
            .upsert(text_clip("b", "https://github.com/rust-lang/rust", 2))
            .unwrap();
        store
            .upsert(text_clip("c", "fn main() { println!(\"hi\"); }", 3))
            .unwrap();

        assert_eq!(search(&store, "world"), ["a"]);
        assert_eq!(search(&store, "RUST"), ["b", "a"]);
        assert_eq!(search(&store, "ust-la"), ["b"]);
        assert_eq!(search(&store, "  main()  "), ["c"]);
        assert_eq!(search(&store, "println!(\"hi\")"), ["c"]);
        assert!(search(&store, "nothing here").is_empty());
        // Empty search = no filter.
        assert_eq!(search(&store, "   ").len(), 3);
    }

    #[test]
    fn search_short_queries_use_like() {
        let store = Store::open_in_memory().unwrap();
        store.upsert(text_clip("a", "50% off_today", 1)).unwrap();
        store.upsert(text_clip("b", "5000 offtoday", 2)).unwrap();
        store.upsert(text_clip("c", "Go is fun", 3)).unwrap();
        store.upsert(text_clip("d", r"C:\temp", 4)).unwrap();
        assert_eq!(search(&store, "%"), ["a"]);
        assert_eq!(search(&store, "_"), ["a"]);
        assert_eq!(search(&store, "go"), ["c"]);
        assert_eq!(search(&store, "GO"), ["c"]);
        assert_eq!(search(&store, r"\"), ["d"]);
        assert_eq!(search(&store, "0%"), ["a"]);
    }

    #[test]
    fn search_special_characters_are_literal() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert(text_clip("q", r#"say "hello" now"#, 1))
            .unwrap();
        store.upsert(text_clip("s", "SELECT * FROM t", 2)).unwrap();
        store.upsert(text_clip("o", "cats OR dogs", 3)).unwrap();
        store.upsert(text_clip("p", "100% sure", 4)).unwrap();
        store.upsert(text_clip("x", "it's fine", 5)).unwrap();

        assert_eq!(search(&store, r#""hello""#), ["q"]);
        assert_eq!(search(&store, r#"""""#), Vec::<String>::new());
        assert_eq!(search(&store, "* FROM"), ["s"]);
        assert_eq!(search(&store, "s OR d"), ["o"]);
        assert_eq!(search(&store, "0% s"), ["p"]);
        assert_eq!(search(&store, "it's"), ["x"]);
        // Would be FTS syntax or SQL if not quoted properly; must not error.
        for nasty in [
            "NEAR(a b)",
            "text:foo",
            "a AND b",
            "foo*",
            "^start",
            "'); DROP TABLE clips; --",
            "\"unbalanced",
            "{text files}: x",
        ] {
            assert!(search(&store, nasty).is_empty(), "{nasty}");
        }
        assert_eq!(store.list(&ListQuery::default()).unwrap().len(), 5);
    }

    #[test]
    fn masked_clips_are_hidden_from_search_and_previews() {
        let store = Store::open_in_memory().unwrap();
        let mut secret = text_clip("s", "ghp_secretsecretsecret123", 1);
        secret.kind = Kind::Sensitive;
        secret.masked = true;
        secret.expires_at = Some(1000);
        store.upsert(secret).unwrap();
        store
            .upsert(text_clip("p", "public secret note", 2))
            .unwrap();

        assert_eq!(search(&store, "secret"), ["p"]);
        assert!(search(&store, "ghp").is_empty());
        assert!(search(&store, "gh").is_empty());
        let row = store.get("s").unwrap().unwrap();
        assert_eq!(row.summary.preview, MASKED_PREVIEW);
        assert!(row.summary.masked);
        // The full body is still available for pasting back.
        assert_eq!(row.text.as_deref(), Some("ghp_secretsecretsecret123"));

        // Deleting a masked row must not corrupt the index.
        store.delete("s").unwrap();
        store.delete("p").unwrap();
        assert!(search(&store, "secret").is_empty());
        let conn = store.conn();
        conn.execute(
            "INSERT INTO clips_fts(clips_fts) VALUES ('integrity-check')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn delete_returns_files() {
        let store = Store::open_in_memory().unwrap();
        let mut img = text_clip("i", "", 1);
        img.kind = Kind::Image;
        img.text = None;
        img.image_path = Some("/x/images/i.png".into());
        img.thumb_path = Some("/x/thumbs/i.png".into());
        store.upsert(img).unwrap();
        assert_eq!(
            store.delete("i").unwrap(),
            ["/x/images/i.png", "/x/thumbs/i.png"]
        );
        assert!(store.get("i").unwrap().is_none());
        assert!(store.delete("i").unwrap().is_empty());
    }

    #[test]
    fn id_prefix_lookup() {
        let store = Store::open_in_memory().unwrap();
        store.upsert(text_clip("01ABC", "a", 1)).unwrap();
        store.upsert(text_clip("01ABD", "b", 2)).unwrap();
        store.upsert(text_clip("01%X", "c", 3)).unwrap();
        assert_eq!(
            store.ids_with_prefix("01ab", 10).unwrap(),
            ["01ABD", "01ABC"]
        );
        assert_eq!(store.ids_with_prefix("01ABC", 10).unwrap(), ["01ABC"]);
        assert_eq!(store.ids_with_prefix("01%", 10).unwrap(), ["01%X"]);
        assert!(store.ids_with_prefix("01_", 10).unwrap().is_empty());
        assert_eq!(store.ids_with_prefix("", 2).unwrap().len(), 2);
    }

    #[test]
    fn prune_removes_expired_unpinned_only() {
        let store = Store::open_in_memory().unwrap();
        for (id, exp) in [("old", Some(100)), ("later", Some(1000)), ("never", None)] {
            let mut c = text_clip(id, id, 1);
            c.expires_at = exp;
            store.upsert(c).unwrap();
        }
        let mut pinned = text_clip("pinned", "pinned", 1);
        pinned.expires_at = Some(50);
        store.upsert(pinned).unwrap();
        store.set_pinned("pinned", true).unwrap();

        assert_eq!(store.prune_expired_counted(500).unwrap().0, 1);
        assert!(store.get("old").unwrap().is_none());
        assert!(store.get("pinned").unwrap().is_some());
        assert!(store.prune_expired(500).unwrap().is_empty());
        assert_eq!(store.prune_expired_counted(1000).unwrap().0, 1);
        assert_eq!(store.list(&ListQuery::default()).unwrap().len(), 2);
    }

    #[test]
    fn repeat_sensitive_copy_extends_expiry() {
        let store = Store::open_in_memory().unwrap();
        let mut c = text_clip("s", "123456", 1);
        c.masked = true;
        c.expires_at = Some(100);
        store.upsert(c.clone()).unwrap();
        c.id = "s2".into();
        c.expires_at = Some(500);
        store.upsert(c).unwrap();
        assert_eq!(
            store.get("s").unwrap().unwrap().summary.expires_at,
            Some(500)
        );
    }

    #[test]
    fn stats_and_day_counts() {
        let store = Store::open_in_memory().unwrap();
        // Noon UTC; any local offset keeps t and t+1s on the same local day.
        let t = 1_718_452_800_000; // 2024-06-15T12:00:00Z
        store.upsert(text_clip("a", "a", t)).unwrap();
        store.upsert(text_clip("b", "b", t + 1000)).unwrap();
        let mut c = text_clip("c", "c", t + 2 * 86_400_000);
        c.kind = Kind::Code;
        c.source_app = None;
        store.upsert(c).unwrap();
        store.set_pinned("a", true).unwrap();

        let days = store
            .day_counts(t - 86_400_000, t + 3 * 86_400_000)
            .unwrap();
        assert_eq!(days.iter().map(|d| d.count).collect::<Vec<_>>(), [2, 1]);
        assert!(days[0].day < days[1].day);
        assert_eq!(days[0].day.len(), 10);
        assert!(store.day_counts(t + 5000, t + 10_000).unwrap().is_empty());

        let s = store.stats().unwrap();
        assert_eq!(s.total, 3);
        assert_eq!(s.pinned, 1);
        assert_eq!(s.by_kind, [(Kind::Text, 2), (Kind::Code, 1)]);
        assert_eq!(s.top_apps, [("notepad.exe".to_owned(), 2)]);
    }

    #[test]
    fn store_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Store>();
    }
}
