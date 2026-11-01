//! `relay.db`: everything the relay keeps. Only ciphertext and routing
//! metadata: opaque space ids, device ids with their *public* keys, sealed
//! envelopes (ids, sender, timestamp, nonce, ciphertext), the keyed
//! per-clip tag and event kind clients attach for compaction, sealed
//! blobs, sizes and times. Nothing here can be decrypted by the relay.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clipd_sync::envelope::{ENVELOPE_VERSION, Envelope, SeqEnvelope};
use clipd_sync::relay::{RelayDevice, RelayPushItem};
use clipd_sync::wire::Rejected;
use rusqlite::{Connection, OptionalExtension, params};

pub type Millis = i64;

const MIGRATIONS: &[&str] = &[SCHEMA_V1];

const SCHEMA_V1: &str = r#"
CREATE TABLE spaces (
    id          TEXT PRIMARY KEY,
    enroll_pub  TEXT    NOT NULL,
    head_seq    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    last_active INTEGER NOT NULL
);
CREATE TABLE devices (
    space_id    TEXT    NOT NULL,
    device_id   TEXT    NOT NULL,
    pub         TEXT    NOT NULL,
    enrolled_at INTEGER NOT NULL,
    last_seen   INTEGER,
    revoked_at  INTEGER,
    PRIMARY KEY (space_id, device_id)
);
CREATE TABLE envelopes (
    space_id    TEXT    NOT NULL,
    seq         INTEGER NOT NULL,
    event_id    TEXT    NOT NULL,
    device_id   TEXT    NOT NULL,
    ts          INTEGER NOT NULL,
    nonce       TEXT    NOT NULL,
    ct          TEXT    NOT NULL,
    tag         TEXT,
    kind        TEXT,
    blob_id     TEXT,
    size        INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    PRIMARY KEY (space_id, seq),
    UNIQUE (space_id, event_id)
);
CREATE INDEX envelopes_tag ON envelopes(space_id, tag);
CREATE INDEX envelopes_blob ON envelopes(space_id, blob_id);
CREATE INDEX envelopes_received ON envelopes(received_at);
CREATE TABLE blobs (
    space_id   TEXT    NOT NULL,
    blob_id    TEXT    NOT NULL,
    data       BLOB    NOT NULL,
    size       INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (space_id, blob_id)
);
"#;

/// Blobs uploaded before their event are kept this long unreferenced.
pub const ORPHAN_BLOB_GRACE_MS: Millis = 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRow {
    pub id: String,
    pub enroll_pub: String,
    pub head_seq: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRec {
    pub public: String,
    pub revoked_at: Option<Millis>,
    pub last_seen: Option<Millis>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enrolled {
    Ok,
    Revoked,
    Full,
    NoSpace,
}

#[derive(Debug, Default)]
pub struct PushOutcome {
    pub accepted: Vec<String>,
    pub rejected: Vec<Rejected>,
    pub fresh: Vec<SeqEnvelope>,
    pub head_seq: i64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub envelopes: i64,
    pub blobs: i64,
    pub bytes: i64,
}

pub struct Store {
    conn: Mutex<Connection>,
}

/// Bytes an envelope row counts against the quota.
pub fn envelope_size(env: &Envelope) -> i64 {
    (env.ct.len() + env.nonce.len() + env.event_id.len() + env.device_id.len() + 64) as i64
}

impl Store {
    pub fn open(dir: &Path) -> Result<Store> {
        std::fs::create_dir_all(dir).context("create data dir")?;
        let path = dir.join("relay.db");
        let conn = Connection::open(&path)
            .with_context(|| format!("open relay database {}", path.display()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.busy_timeout(Duration::from_secs(5))?;
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = OFF;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > MIGRATIONS.len() as i64 {
            bail!("relay database is newer than this build supports");
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)
                .with_context(|| format!("migrate relay schema to v{}", i + 1))?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- spaces and devices ------------------------------------------------

    pub fn space(&self, id: &str) -> Result<Option<SpaceRow>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id, enroll_pub, head_seq FROM spaces WHERE id = ?1",
                [id],
                |r| {
                    Ok(SpaceRow {
                        id: r.get(0)?,
                        enroll_pub: r.get(1)?,
                        head_seq: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn space_count(&self) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM spaces", [], |r| r.get(0))?)
    }

    /// Create a space with its first device. False when it already exists.
    pub fn register(
        &self,
        space: &str,
        device: &str,
        device_pub: &str,
        enroll_pub: &str,
        now: Millis,
    ) -> Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let n = tx.execute(
            "INSERT OR IGNORE INTO spaces (id, enroll_pub, created_at, last_active) \
             VALUES (?1, ?2, ?3, ?3)",
            params![space, enroll_pub, now],
        )?;
        if n == 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO devices (space_id, device_id, pub, enrolled_at, last_seen) \
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![space, device, device_pub, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn device(&self, space: &str, device: &str) -> Result<Option<DeviceRec>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT pub, revoked_at, last_seen FROM devices \
                 WHERE space_id = ?1 AND device_id = ?2",
                params![space, device],
                |r| {
                    Ok(DeviceRec {
                        public: r.get(0)?,
                        revoked_at: r.get(1)?,
                        last_seen: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Register (or re-key) a device. Revoked ids stay revoked.
    pub fn enroll(
        &self,
        space: &str,
        device: &str,
        device_pub: &str,
        max_devices: u32,
        now: Millis,
    ) -> Result<Enrolled> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let exists: Option<i64> = tx
            .query_row("SELECT 1 FROM spaces WHERE id = ?1", [space], |r| r.get(0))
            .optional()?;
        if exists.is_none() {
            return Ok(Enrolled::NoSpace);
        }
        let current: Option<Option<Millis>> = tx
            .query_row(
                "SELECT revoked_at FROM devices WHERE space_id = ?1 AND device_id = ?2",
                params![space, device],
                |r| r.get(0),
            )
            .optional()?;
        match current {
            Some(Some(_)) => return Ok(Enrolled::Revoked),
            Some(None) => {
                tx.execute(
                    "UPDATE devices SET pub = ?3, last_seen = ?4 \
                     WHERE space_id = ?1 AND device_id = ?2",
                    params![space, device, device_pub, now],
                )?;
            }
            None => {
                let active: i64 = tx.query_row(
                    "SELECT count(*) FROM devices WHERE space_id = ?1 AND revoked_at IS NULL",
                    [space],
                    |r| r.get(0),
                )?;
                if active >= i64::from(max_devices) {
                    return Ok(Enrolled::Full);
                }
                tx.execute(
                    "INSERT INTO devices (space_id, device_id, pub, enrolled_at, last_seen) \
                     VALUES (?1, ?2, ?3, ?4, ?4)",
                    params![space, device, device_pub, now],
                )?;
            }
        }
        tx.commit()?;
        Ok(Enrolled::Ok)
    }

    pub fn revoke(&self, space: &str, device: &str, now: Millis) -> Result<bool> {
        Ok(self.conn().execute(
            "UPDATE devices SET revoked_at = ?3 \
             WHERE space_id = ?1 AND device_id = ?2 AND revoked_at IS NULL",
            params![space, device, now],
        )? == 1)
    }

    /// Update last-seen (at most once a minute per device).
    pub fn touch(&self, space: &str, device: &str, now: Millis) -> Result<()> {
        self.conn().execute(
            "UPDATE devices SET last_seen = ?3 WHERE space_id = ?1 AND device_id = ?2 \
             AND (last_seen IS NULL OR last_seen < ?3 - 60000)",
            params![space, device, now],
        )?;
        Ok(())
    }

    pub fn set_enroll_pub(&self, space: &str, enroll_pub: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE spaces SET enroll_pub = ?2 WHERE id = ?1",
            params![space, enroll_pub],
        )?;
        Ok(())
    }

    pub fn devices(&self, space: &str) -> Result<Vec<RelayDevice>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT device_id, enrolled_at, last_seen, revoked_at FROM devices \
             WHERE space_id = ?1 ORDER BY enrolled_at, device_id",
        )?;
        let rows = stmt.query_map([space], |r| {
            Ok(RelayDevice {
                device_id: r.get(0)?,
                enrolled_at: r.get(1)?,
                last_seen: r.get(2)?,
                revoked_at: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_space(&self, space: &str) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for t in ["envelopes", "blobs", "devices"] {
            tx.execute(&format!("DELETE FROM {t} WHERE space_id = ?1"), [space])?;
        }
        tx.execute("DELETE FROM spaces WHERE id = ?1", [space])?;
        tx.commit()?;
        Ok(())
    }

    // ---- the log -------------------------------------------------------------

    /// Append envelopes (idempotent by event id), compact superseded ones
    /// per tag, then evict the oldest data while the space is over
    /// `max_bytes`.
    pub fn push(
        &self,
        space: &str,
        items: Vec<RelayPushItem>,
        now: Millis,
        max_bytes: i64,
    ) -> Result<PushOutcome> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut head: i64 =
            tx.query_row("SELECT head_seq FROM spaces WHERE id = ?1", [space], |r| {
                r.get(0)
            })?;
        let mut out = PushOutcome::default();
        for item in items {
            let env = item.env;
            let size = envelope_size(&env);
            if size > max_bytes {
                out.rejected.push(Rejected {
                    event_id: env.event_id,
                    reason: "event larger than this relay's space quota".into(),
                });
                continue;
            }
            let dup: Option<i64> = tx
                .query_row(
                    "SELECT seq FROM envelopes WHERE space_id = ?1 AND event_id = ?2",
                    params![space, env.event_id],
                    |r| r.get(0),
                )
                .optional()?;
            if dup.is_some() {
                out.accepted.push(env.event_id);
                continue;
            }
            head += 1;
            tx.execute(
                "INSERT INTO envelopes (space_id, seq, event_id, device_id, ts, nonce, ct, tag, \
                     kind, blob_id, size, received_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    space,
                    head,
                    env.event_id,
                    env.device_id,
                    env.ts,
                    env.nonce,
                    env.ct,
                    item.tag,
                    item.kind,
                    item.blob,
                    size,
                    now
                ],
            )?;
            if let (Some(tag), Some(kind)) = (&item.tag, &item.kind) {
                compact(&tx, space, tag, kind, head)?;
            }
            out.accepted.push(env.event_id.clone());
            out.fresh.push(SeqEnvelope { seq: head, env });
        }
        tx.execute(
            "UPDATE spaces SET head_seq = ?2, last_active = ?3 WHERE id = ?1",
            params![space, head, now],
        )?;
        let first_new = out.fresh.first().map_or(i64::MAX, |e| e.seq);
        evict(&tx, space, max_bytes, first_new, None)?;
        // Evicted rows can't be streamed any more.
        let kept: Vec<i64> = {
            let mut stmt =
                tx.prepare("SELECT seq FROM envelopes WHERE space_id = ?1 AND seq >= ?2")?;
            stmt.query_map(params![space, first_new], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        out.fresh.retain(|e| kept.contains(&e.seq));
        tx.commit()?;
        out.head_seq = head;
        Ok(out)
    }

    pub fn events_since(&self, space: &str, since: i64, limit: u32) -> Result<Vec<SeqEnvelope>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT seq, event_id, device_id, ts, nonce, ct FROM envelopes \
             WHERE space_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![space, since, limit], |r| {
            Ok(SeqEnvelope {
                seq: r.get(0)?,
                env: Envelope {
                    v: ENVELOPE_VERSION,
                    event_id: r.get(1)?,
                    device_id: r.get(2)?,
                    ts: r.get(3)?,
                    nonce: r.get(4)?,
                    ct: r.get(5)?,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn head_seq(&self, space: &str) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT head_seq FROM spaces WHERE id = ?1", [space], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or(0))
    }

    // ---- blobs -----------------------------------------------------------------

    /// Store (or replace) a sealed blob, evicting old data over quota.
    pub fn put_blob(
        &self,
        space: &str,
        blob_id: &str,
        data: &[u8],
        now: Millis,
        max_bytes: i64,
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO blobs (space_id, blob_id, data, size, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(space_id, blob_id) DO UPDATE SET data = excluded.data, \
                 size = excluded.size, created_at = excluded.created_at",
            params![space, blob_id, data, data.len() as i64, now],
        )?;
        tx.execute(
            "UPDATE spaces SET last_active = ?2 WHERE id = ?1",
            params![space, now],
        )?;
        evict(&tx, space, max_bytes, i64::MAX, Some(blob_id))?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_blob(&self, space: &str, blob_id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT data FROM blobs WHERE space_id = ?1 AND blob_id = ?2",
                params![space, blob_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn usage(&self, space: &str) -> Result<Usage> {
        let conn = self.conn();
        usage(&conn, space)
    }

    // ---- retention ---------------------------------------------------------------

    /// Drop envelopes received before `cutoff`, then blobs no envelope
    /// references (after a grace period). Returns (envelopes, blobs) removed.
    pub fn prune(&self, cutoff: Millis, now: Millis) -> Result<(usize, usize)> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let envs = tx.execute("DELETE FROM envelopes WHERE received_at < ?1", [cutoff])?;
        let blobs = tx.execute(
            "DELETE FROM blobs WHERE created_at < ?1 AND NOT EXISTS (\
                 SELECT 1 FROM envelopes e WHERE e.space_id = blobs.space_id \
                 AND e.blob_id = blobs.blob_id)",
            [now - ORPHAN_BLOB_GRACE_MS],
        )?;
        tx.commit()?;
        Ok((envs, blobs))
    }
}

/// Keep only the newest state per clip (same rules as the hub log): latest
/// add, latest pin change, latest config, and every tombstone (removing the
/// adds and pin changes before it).
fn compact(
    tx: &rusqlite::Transaction<'_>,
    space: &str,
    tag: &str,
    kind: &str,
    seq: i64,
) -> Result<()> {
    let superseded = match kind {
        "ClipAdded" => "('ClipAdded')",
        "ClipUpdated" => "('ClipUpdated')",
        "SpaceConfig" => "('SpaceConfig')",
        "ClipDeleted" => "('ClipAdded', 'ClipUpdated')",
        _ => return Ok(()),
    };
    let blobs: Vec<String> = {
        let mut stmt = tx.prepare(&format!(
            "SELECT blob_id FROM envelopes WHERE space_id = ?1 AND tag = ?2 AND seq < ?3 \
             AND kind IN {superseded} AND blob_id IS NOT NULL"
        ))?;
        stmt.query_map(params![space, tag, seq], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    tx.execute(
        &format!(
            "DELETE FROM envelopes WHERE space_id = ?1 AND tag = ?2 AND seq < ?3 \
             AND kind IN {superseded}"
        ),
        params![space, tag, seq],
    )?;
    for b in blobs {
        drop_blob_if_unreferenced(tx, space, &b)?;
    }
    Ok(())
}

fn drop_blob_if_unreferenced(
    tx: &rusqlite::Transaction<'_>,
    space: &str,
    blob: &str,
) -> Result<()> {
    tx.execute(
        "DELETE FROM blobs WHERE space_id = ?1 AND blob_id = ?2 AND NOT EXISTS (\
             SELECT 1 FROM envelopes WHERE space_id = ?1 AND blob_id = ?2)",
        params![space, blob],
    )?;
    Ok(())
}

fn usage(conn: &Connection, space: &str) -> Result<Usage> {
    let (envelopes, ebytes): (i64, i64) = conn.query_row(
        "SELECT count(*), coalesce(sum(size), 0) FROM envelopes WHERE space_id = ?1",
        [space],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (blobs, bbytes): (i64, i64) = conn.query_row(
        "SELECT count(*), coalesce(sum(size), 0) FROM blobs WHERE space_id = ?1",
        [space],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(Usage {
        envelopes,
        blobs,
        bytes: ebytes + bbytes,
    })
}

/// Over quota: delete the oldest envelopes (seq < `keep_from`) and blobs
/// (except `keep_blob`), oldest first, until the space fits.
fn evict(
    tx: &rusqlite::Transaction<'_>,
    space: &str,
    max_bytes: i64,
    keep_from: i64,
    keep_blob: Option<&str>,
) -> Result<()> {
    while usage(tx, space)?.bytes > max_bytes {
        let env: Option<(i64, i64, Option<String>)> = tx
            .query_row(
                "SELECT seq, received_at, blob_id FROM envelopes \
                 WHERE space_id = ?1 AND seq < ?2 ORDER BY seq LIMIT 1",
                params![space, keep_from],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let blob: Option<(String, i64)> = tx
            .query_row(
                "SELECT blob_id, created_at FROM blobs \
                 WHERE space_id = ?1 AND blob_id IS NOT ?2 ORDER BY created_at LIMIT 1",
                params![space, keep_blob],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match (env, blob) {
            (Some(e), Some(b)) if b.1 < e.1 => delete_blob(tx, space, &b.0)?,
            (Some((seq, _, blob_id)), _) => {
                tx.execute(
                    "DELETE FROM envelopes WHERE space_id = ?1 AND seq = ?2",
                    params![space, seq],
                )?;
                if let Some(b) = blob_id {
                    drop_blob_if_unreferenced(tx, space, &b)?;
                }
            }
            (None, Some(b)) => delete_blob(tx, space, &b.0)?,
            (None, None) => break,
        }
    }
    Ok(())
}

fn delete_blob(tx: &rusqlite::Transaction<'_>, space: &str, blob: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM blobs WHERE space_id = ?1 AND blob_id = ?2",
        params![space, blob],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        id: &str,
        tag: Option<&str>,
        kind: Option<&str>,
        blob: Option<&str>,
        ct_len: usize,
    ) -> RelayPushItem {
        RelayPushItem {
            env: Envelope {
                v: 1,
                event_id: id.into(),
                device_id: "D".into(),
                ts: 1,
                nonce: "n".repeat(32),
                ct: "c".repeat(ct_len),
            },
            tag: tag.map(Into::into),
            kind: kind.map(Into::into),
            blob: blob.map(Into::into),
        }
    }

    fn ids(s: &Store) -> Vec<String> {
        s.events_since("S", 0, 100)
            .unwrap()
            .into_iter()
            .map(|e| e.env.event_id)
            .collect()
    }

    const BIG: i64 = 1 << 40;

    #[test]
    fn register_enroll_revoke() {
        let s = Store::open_in_memory().unwrap();
        assert!(s.register("S", "D1", "P1", "E", 1).unwrap());
        assert!(!s.register("S", "D9", "P9", "E", 1).unwrap(), "exists");
        assert_eq!(s.enroll("S", "D2", "P2", 3, 2).unwrap(), Enrolled::Ok);
        assert_eq!(s.enroll("S", "D3", "P3", 3, 2).unwrap(), Enrolled::Ok);
        assert_eq!(s.enroll("S", "D4", "P4", 3, 2).unwrap(), Enrolled::Full);
        assert_eq!(s.enroll("X", "D4", "P4", 3, 2).unwrap(), Enrolled::NoSpace);
        // Re-key an active device.
        assert_eq!(s.enroll("S", "D2", "P2b", 3, 3).unwrap(), Enrolled::Ok);
        assert_eq!(s.device("S", "D2").unwrap().unwrap().public, "P2b");
        assert!(s.revoke("S", "D2", 4).unwrap());
        assert!(!s.revoke("S", "D2", 5).unwrap());
        assert_eq!(s.enroll("S", "D2", "P2c", 3, 6).unwrap(), Enrolled::Revoked);
        assert_eq!(s.device("S", "D2").unwrap().unwrap().revoked_at, Some(4));
        // A revoked device frees its slot.
        assert_eq!(s.enroll("S", "D4", "P4", 3, 7).unwrap(), Enrolled::Ok);
        assert_eq!(s.devices("S").unwrap().len(), 4);
        s.delete_space("S").unwrap();
        assert!(s.space("S").unwrap().is_none());
        assert!(s.devices("S").unwrap().is_empty());
    }

    #[test]
    fn push_is_idempotent_and_compacts_like_the_hub() {
        let s = Store::open_in_memory().unwrap();
        s.register("S", "D", "P", "E", 0).unwrap();
        let o = s
            .push(
                "S",
                vec![
                    item("A1", Some("h1"), Some("ClipAdded"), Some("b1"), 10),
                    item("U1", Some("h1"), Some("ClipUpdated"), None, 10),
                    item("A2", Some("h1"), Some("ClipAdded"), Some("b1"), 10),
                    item("B1", Some("h2"), Some("ClipAdded"), None, 10),
                ],
                1,
                BIG,
            )
            .unwrap();
        assert_eq!(o.accepted.len(), 4);
        assert_eq!(o.head_seq, 4);
        s.put_blob("S", "b1", b"sealed", 1, BIG).unwrap();
        assert_eq!(ids(&s), ["U1", "A2", "B1"]);
        // Duplicate: accepted, nothing new.
        let o = s
            .push(
                "S",
                vec![item("A2", Some("h1"), Some("ClipAdded"), None, 10)],
                2,
                BIG,
            )
            .unwrap();
        assert_eq!((o.accepted.len(), o.fresh.len(), o.head_seq), (1, 0, 4));
        // A tombstone removes add + pin and the image.
        s.push(
            "S",
            vec![item("D1", Some("h1"), Some("ClipDeleted"), None, 10)],
            3,
            BIG,
        )
        .unwrap();
        assert_eq!(ids(&s), ["B1", "D1"]);
        assert!(s.get_blob("S", "b1").unwrap().is_none(), "image wiped");
        // Configs keep the newest only; untagged events are never compacted.
        s.push(
            "S",
            vec![
                item("C1", Some("cfg"), Some("SpaceConfig"), None, 10),
                item("C2", Some("cfg"), Some("SpaceConfig"), None, 10),
                item("X1", None, None, None, 10),
                item("X2", None, None, None, 10),
            ],
            4,
            BIG,
        )
        .unwrap();
        assert_eq!(ids(&s), ["B1", "D1", "C2", "X1", "X2"]);
        assert_eq!(s.events_since("S", 6, 10).unwrap().len(), 3);
    }

    #[test]
    fn quota_evicts_oldest_and_rejects_oversized() {
        let s = Store::open_in_memory().unwrap();
        s.register("S", "D", "P", "E", 0).unwrap();
        let one = envelope_size(&item("E01", None, None, None, 1000).env);
        let max = one * 5 + 10;
        for i in 0..8 {
            s.push(
                "S",
                vec![item(&format!("E{i:02}"), None, None, None, 1000)],
                i,
                max,
            )
            .unwrap();
        }
        assert!(s.usage("S").unwrap().bytes <= max);
        assert_eq!(ids(&s), ["E03", "E04", "E05", "E06", "E07"]);
        // Blobs count too and the oldest data goes first.
        s.put_blob("S", "b1", &vec![0u8; (one * 2) as usize], 100, max)
            .unwrap();
        assert!(s.usage("S").unwrap().bytes <= max);
        assert_eq!(ids(&s), ["E05", "E06", "E07"]);
        assert!(s.get_blob("S", "b1").unwrap().is_some());
        let o = s
            .push(
                "S",
                vec![item("HUGE", None, None, None, (max * 2) as usize)],
                200,
                max,
            )
            .unwrap();
        assert_eq!(o.rejected.len(), 1);
        assert!(o.accepted.is_empty());
    }

    #[test]
    fn retention_prunes_old_events_and_orphan_blobs() {
        let s = Store::open_in_memory().unwrap();
        s.register("S", "D", "P", "E", 0).unwrap();
        s.push(
            "S",
            vec![item(
                "OLD",
                Some("t1"),
                Some("ClipAdded"),
                Some("b-old"),
                10,
            )],
            100,
            BIG,
        )
        .unwrap();
        s.put_blob("S", "b-old", b"x", 100, BIG).unwrap();
        s.push(
            "S",
            vec![item(
                "NEW",
                Some("t2"),
                Some("ClipAdded"),
                Some("b-new"),
                10,
            )],
            5_000_000,
            BIG,
        )
        .unwrap();
        s.put_blob("S", "b-new", b"y", 5_000_000, BIG).unwrap();
        // Uploaded just now, its event not pushed yet: kept (grace period).
        s.put_blob("S", "b-pending", b"z", 5_000_000, BIG).unwrap();
        let (e, b) = s.prune(1_000_000, 5_000_001).unwrap();
        assert_eq!((e, b), (1, 1));
        assert_eq!(ids(&s), ["NEW"]);
        assert!(s.get_blob("S", "b-old").unwrap().is_none());
        assert!(s.get_blob("S", "b-new").unwrap().is_some());
        assert!(s.get_blob("S", "b-pending").unwrap().is_some());
        assert_eq!(s.head_seq("S").unwrap(), 2, "seq never goes back");
    }
}
