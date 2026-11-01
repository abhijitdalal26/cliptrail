//! `sync.db`: everything sync keeps, separate from the clip store.
//!
//! Holds no plaintext clip content: the hub log and the client outbox are
//! sealed envelopes, device tokens are sealed under the space key, and
//! hashes only appear as keyed tags or as the raw dedup hash the clip
//! store already has.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clipd_core::Millis;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::envelope::{Envelope, SeqEnvelope};

const MIGRATIONS: &[&str] = &[SCHEMA_V1];

const SCHEMA_V1: &str = r#"
CREATE TABLE state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Hub: every paired device (and itself). Client: mirror of the hub's list.
CREATE TABLE devices (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    platform     TEXT NOT NULL,
    token_sealed BLOB,
    paired_at    INTEGER NOT NULL,
    revoked_at   INTEGER,
    last_seen    INTEGER,
    is_self      INTEGER NOT NULL DEFAULT 0
);
-- Hub: the space log. seq is the order every device applies events in.
CREATE TABLE envelopes (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id    TEXT    NOT NULL UNIQUE,
    device_id   TEXT    NOT NULL,
    ts          INTEGER NOT NULL,
    nonce       TEXT    NOT NULL,
    ct          TEXT    NOT NULL,
    hash_tag    TEXT,
    ev_kind     TEXT,
    received_at INTEGER NOT NULL
);
CREATE INDEX envelopes_tag ON envelopes(hash_tag);
-- Hub: which clip an opaque blob id refers to (blobs are sealed on demand).
CREATE TABLE blob_index (
    blob_id      TEXT PRIMARY KEY,
    content_hash BLOB NOT NULL
);
-- Last-writer-wins clocks per clip.
CREATE TABLE clocks (
    content_hash BLOB PRIMARY KEY,
    pin_at       INTEGER,
    pin_dev      TEXT,
    deleted_at   INTEGER
);
-- Client: sealed events not yet accepted by the hub.
CREATE TABLE outbox (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id   TEXT    NOT NULL UNIQUE,
    envelope   TEXT    NOT NULL,
    blob_id    TEXT,
    blob_hash  BLOB,
    created_at INTEGER NOT NULL
);
-- Full images still to fetch (client) or to receive (hub).
CREATE TABLE pending_blobs (
    blob_id      TEXT PRIMARY KEY,
    content_hash BLOB    NOT NULL,
    attempts     INTEGER NOT NULL DEFAULT 0,
    next_try     INTEGER NOT NULL DEFAULT 0
);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Hub,
    Client,
}

/// Identity and cursors of this data dir in its space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceState {
    pub role: Role,
    pub space_id: String,
    pub device_id: String,
    pub device_name: String,
    pub hub_device_id: String,
    pub hub_name: String,
    /// Client: `host:port` candidates for the hub, last working first.
    pub hub_hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub paired_at: Millis,
    pub revoked_at: Option<Millis>,
    pub last_seen: Option<Millis>,
    pub is_self: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clock {
    pub pin_at: Option<Millis>,
    pub pin_dev: Option<String>,
    pub deleted_at: Option<Millis>,
}

#[derive(Debug, Clone)]
pub struct OutboxRow {
    pub env: Envelope,
    pub blob: Option<(String, Vec<u8>)>,
}

pub struct SyncDb {
    conn: Mutex<Connection>,
}

impl SyncDb {
    pub fn open(path: &Path) -> Result<SyncDb> {
        let conn = Connection::open(path)
            .with_context(|| format!("open sync database {}", path.display()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<SyncDb> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<SyncDb> {
        conn.busy_timeout(Duration::from_secs(5))?;
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > MIGRATIONS.len() as i64 {
            bail!("sync database is newer than this build supports");
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)
                .with_context(|| format!("migrate sync schema to v{}", i + 1))?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(SyncDb {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- state -------------------------------------------------------

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT value FROM state WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO state(key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_i64(&self, key: &str) -> Result<i64> {
        Ok(self.get(key)?.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    pub fn set_i64(&self, key: &str, v: i64) -> Result<()> {
        self.set(key, &v.to_string())
    }

    pub fn space(&self) -> Result<Option<SpaceState>> {
        let Some(role) = self.get("role")? else {
            return Ok(None);
        };
        let role = match role.as_str() {
            "hub" => Role::Hub,
            "client" => Role::Client,
            _ => return Ok(None),
        };
        let g = |k: &str| -> Result<String> { Ok(self.get(k)?.unwrap_or_default()) };
        Ok(Some(SpaceState {
            role,
            space_id: g("space_id")?,
            device_id: g("device_id")?,
            device_name: g("device_name")?,
            hub_device_id: g("hub_device_id")?,
            hub_name: g("hub_name")?,
            hub_hosts: serde_json::from_str(&g("hub_hosts")?).unwrap_or_default(),
        }))
    }

    pub fn save_space(&self, s: &SpaceState) -> Result<()> {
        let role = match s.role {
            Role::Hub => "hub",
            Role::Client => "client",
        };
        self.set("space_id", &s.space_id)?;
        self.set("device_id", &s.device_id)?;
        self.set("device_name", &s.device_name)?;
        self.set("hub_device_id", &s.hub_device_id)?;
        self.set("hub_name", &s.hub_name)?;
        self.set("hub_hosts", &serde_json::to_string(&s.hub_hosts)?)?;
        // Last: a half-written space never looks configured.
        self.set("role", role)?;
        Ok(())
    }

    /// Forget the space entirely (leave / reset).
    pub fn wipe(&self) -> Result<()> {
        self.conn().execute_batch(
            "DELETE FROM state; DELETE FROM devices; DELETE FROM envelopes; \
             DELETE FROM blob_index; DELETE FROM clocks; DELETE FROM outbox; \
             DELETE FROM pending_blobs;",
        )?;
        Ok(())
    }

    // ---- devices -----------------------------------------------------

    pub fn upsert_device(&self, d: &DeviceRow, token_sealed: Option<&[u8]>) -> Result<()> {
        self.conn().execute(
            "INSERT INTO devices (id, name, platform, token_sealed, paired_at, revoked_at, \
                 last_seen, is_self) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, platform = excluded.platform, \
                 token_sealed = coalesce(excluded.token_sealed, devices.token_sealed), \
                 paired_at = excluded.paired_at, revoked_at = excluded.revoked_at, \
                 last_seen = coalesce(excluded.last_seen, devices.last_seen), \
                 is_self = excluded.is_self",
            params![
                d.id,
                d.name,
                d.platform,
                token_sealed,
                d.paired_at,
                d.revoked_at,
                d.last_seen,
                d.is_self
            ],
        )?;
        Ok(())
    }

    /// Sealed token of an active (not revoked) device.
    pub fn active_token(&self, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT token_sealed FROM devices WHERE id = ?1 AND revoked_at IS NULL \
                 AND token_sealed IS NOT NULL",
                [id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn is_revoked(&self, id: &str) -> Result<bool> {
        Ok(self
            .conn()
            .query_row(
                "SELECT revoked_at IS NOT NULL FROM devices WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(true))
    }

    pub fn devices(&self) -> Result<Vec<DeviceRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, platform, paired_at, revoked_at, last_seen, is_self FROM devices \
             ORDER BY is_self DESC, paired_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(DeviceRow {
                id: r.get(0)?,
                name: r.get(1)?,
                platform: r.get(2)?,
                paired_at: r.get(3)?,
                revoked_at: r.get(4)?,
                last_seen: r.get(5)?,
                is_self: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Revoke a paired device: its token stops working at once. Returns
    /// false when there is no such (non-self, active) device.
    pub fn revoke_device(&self, id: &str, at: Millis) -> Result<bool> {
        Ok(self.conn().execute(
            "UPDATE devices SET revoked_at = ?2, token_sealed = NULL \
             WHERE id = ?1 AND revoked_at IS NULL AND is_self = 0",
            params![id, at],
        )? == 1)
    }

    pub fn touch_device(&self, id: &str, at: Millis) -> Result<()> {
        self.conn().execute(
            "UPDATE devices SET last_seen = ?2 WHERE id = ?1",
            params![id, at],
        )?;
        Ok(())
    }

    /// Client: replace the mirrored device list.
    pub fn replace_devices(&self, devices: &[DeviceRow]) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM devices", [])?;
        for d in devices {
            tx.execute(
                "INSERT INTO devices (id, name, platform, paired_at, revoked_at, last_seen, \
                     is_self) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    d.id,
                    d.name,
                    d.platform,
                    d.paired_at,
                    d.revoked_at,
                    d.last_seen,
                    d.is_self
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ---- hub log -----------------------------------------------------

    /// Append an envelope and compact superseded ones for the same clip.
    /// Returns the assigned seq, or the existing seq for a duplicate
    /// `event_id` (idempotent) with `false`.
    pub fn append_envelope(
        &self,
        env: &Envelope,
        hash_tag: &str,
        ev_kind: &str,
        now: Millis,
    ) -> Result<(i64, bool)> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if let Some(seq) = tx
            .query_row(
                "SELECT seq FROM envelopes WHERE event_id = ?1",
                [&env.event_id],
                |r| r.get(0),
            )
            .optional()?
        {
            return Ok((seq, false));
        }
        tx.execute(
            "INSERT INTO envelopes (event_id, device_id, ts, nonce, ct, hash_tag, ev_kind, \
                 received_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                env.event_id,
                env.device_id,
                env.ts,
                env.nonce,
                env.ct,
                hash_tag,
                ev_kind,
                now
            ],
        )?;
        let seq = tx.last_insert_rowid();
        // Keep only the newest state per clip: latest add, latest pin
        // change, and every tombstone (a stale add after a delete must not
        // resurrect the clip on a device that catches up later).
        let superseded = match ev_kind {
            "ClipAdded" => "('ClipAdded')",
            "ClipUpdated" => "('ClipUpdated')",
            _ => "('ClipAdded', 'ClipUpdated')",
        };
        tx.execute(
            &format!(
                "DELETE FROM envelopes WHERE hash_tag = ?1 AND seq < ?2 AND ev_kind IN {superseded}"
            ),
            params![hash_tag, seq],
        )?;
        tx.commit()?;
        Ok((seq, true))
    }

    pub fn envelopes_since(&self, since: i64, limit: u32) -> Result<Vec<SeqEnvelope>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT seq, event_id, device_id, ts, nonce, ct FROM envelopes \
             WHERE seq > ?1 ORDER BY seq LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![since, limit], |r| {
            Ok(SeqEnvelope {
                seq: r.get(0)?,
                env: Envelope {
                    v: crate::envelope::ENVELOPE_VERSION,
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

    pub fn head_seq(&self) -> Result<i64> {
        Ok(self.conn().query_row(
            "SELECT coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'envelopes'), 0)",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn envelope_count(&self) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM envelopes", [], |r| r.get(0))?)
    }

    pub fn put_blob_index(&self, blob_id: &str, content_hash: &[u8]) -> Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO blob_index (blob_id, content_hash) VALUES (?1, ?2)",
            params![blob_id, content_hash],
        )?;
        Ok(())
    }

    pub fn blob_hash(&self, blob_id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT content_hash FROM blob_index WHERE blob_id = ?1",
                [blob_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    // ---- clocks ------------------------------------------------------

    pub fn clock(&self, hash: &[u8]) -> Result<Clock> {
        Ok(self
            .conn()
            .query_row(
                "SELECT pin_at, pin_dev, deleted_at FROM clocks WHERE content_hash = ?1",
                [hash],
                |r| {
                    Ok(Clock {
                        pin_at: r.get(0)?,
                        pin_dev: r.get(1)?,
                        deleted_at: r.get(2)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn set_pin_clock(&self, hash: &[u8], at: Millis, dev: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO clocks (content_hash, pin_at, pin_dev) VALUES (?1, ?2, ?3) \
             ON CONFLICT(content_hash) DO UPDATE SET pin_at = ?2, pin_dev = ?3",
            params![hash, at, dev],
        )?;
        Ok(())
    }

    pub fn set_deleted_clock(&self, hash: &[u8], at: Millis) -> Result<()> {
        self.conn().execute(
            "INSERT INTO clocks (content_hash, deleted_at) VALUES (?1, ?2) \
             ON CONFLICT(content_hash) DO UPDATE SET \
                 deleted_at = max(coalesce(deleted_at, 0), ?2)",
            params![hash, at],
        )?;
        Ok(())
    }

    // ---- outbox ------------------------------------------------------

    pub fn push_outbox(
        &self,
        env: &Envelope,
        blob: Option<(&str, &[u8])>,
        now: Millis,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO outbox (event_id, envelope, blob_id, blob_hash, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                env.event_id,
                serde_json::to_string(env)?,
                blob.map(|b| b.0),
                blob.map(|b| b.1),
                now
            ],
        )?;
        Ok(())
    }

    pub fn outbox(&self, limit: u32) -> Result<Vec<OutboxRow>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT envelope, blob_id, blob_hash FROM outbox ORDER BY id LIMIT ?1")?;
        let rows = stmt.query_map([limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<Vec<u8>>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (env, blob_id, blob_hash) = row?;
            out.push(OutboxRow {
                env: serde_json::from_str(&env).context("corrupt outbox row")?,
                blob: blob_id.zip(blob_hash),
            });
        }
        Ok(out)
    }

    pub fn remove_outbox(&self, event_ids: &[String]) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for id in event_ids {
            tx.execute("DELETE FROM outbox WHERE event_id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn outbox_len(&self) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))?)
    }

    // ---- pending blobs -------------------------------------------------

    pub fn add_pending_blob(&self, blob_id: &str, hash: &[u8]) -> Result<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO pending_blobs (blob_id, content_hash) VALUES (?1, ?2)",
            params![blob_id, hash],
        )?;
        Ok(())
    }

    pub fn due_pending_blobs(
        &self,
        now: Millis,
        limit: u32,
    ) -> Result<Vec<(String, Vec<u8>, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT blob_id, content_hash, attempts FROM pending_blobs WHERE next_try <= ?1 \
             ORDER BY next_try LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![now, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn pending_blob_hash(&self, blob_id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT content_hash FROM pending_blobs WHERE blob_id = ?1",
                [blob_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn retry_pending_blob(&self, blob_id: &str, next_try: Millis) -> Result<()> {
        self.conn().execute(
            "UPDATE pending_blobs SET attempts = attempts + 1, next_try = ?2 WHERE blob_id = ?1",
            params![blob_id, next_try],
        )?;
        Ok(())
    }

    pub fn remove_pending_blob(&self, blob_id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM pending_blobs WHERE blob_id = ?1", [blob_id])?;
        Ok(())
    }

    pub fn pending_blob_count(&self) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM pending_blobs", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(id: &str) -> Envelope {
        Envelope {
            v: 1,
            event_id: id.into(),
            device_id: "D".into(),
            ts: 1,
            nonce: "n".repeat(32),
            ct: "c".into(),
        }
    }

    #[test]
    fn log_appends_idempotently_and_compacts() {
        let db = SyncDb::open_in_memory().unwrap();
        assert_eq!(
            db.append_envelope(&env("A1"), "h1", "ClipAdded", 0)
                .unwrap(),
            (1, true)
        );
        assert_eq!(
            db.append_envelope(&env("U1"), "h1", "ClipUpdated", 0)
                .unwrap(),
            (2, true)
        );
        assert_eq!(
            db.append_envelope(&env("A2"), "h1", "ClipAdded", 0)
                .unwrap(),
            (3, true)
        );
        assert_eq!(
            db.append_envelope(&env("A2"), "h1", "ClipAdded", 0)
                .unwrap(),
            (3, false)
        );
        assert_eq!(
            db.append_envelope(&env("B1"), "h2", "ClipAdded", 0)
                .unwrap(),
            (4, true)
        );
        let ids = |db: &SyncDb| -> Vec<String> {
            db.envelopes_since(0, 100)
                .unwrap()
                .into_iter()
                .map(|e| e.env.event_id)
                .collect()
        };
        assert_eq!(ids(&db), ["U1", "A2", "B1"]);
        db.append_envelope(&env("D1"), "h1", "ClipDeleted", 0)
            .unwrap();
        assert_eq!(ids(&db), ["B1", "D1"]);
        // A later add keeps the tombstone.
        db.append_envelope(&env("A3"), "h1", "ClipAdded", 0)
            .unwrap();
        assert_eq!(ids(&db), ["B1", "D1", "A3"]);
        assert_eq!(db.head_seq().unwrap(), 6);
        assert_eq!(db.envelopes_since(5, 10).unwrap().len(), 1);
    }

    #[test]
    fn devices_and_revoke() {
        let db = SyncDb::open_in_memory().unwrap();
        let d = DeviceRow {
            id: "P1".into(),
            name: "Phone".into(),
            platform: "android".into(),
            paired_at: 1,
            revoked_at: None,
            last_seen: None,
            is_self: false,
        };
        db.upsert_device(&d, Some(b"sealed")).unwrap();
        assert_eq!(db.active_token("P1").unwrap(), Some(b"sealed".to_vec()));
        assert!(!db.is_revoked("P1").unwrap());
        assert!(db.revoke_device("P1", 5).unwrap());
        assert!(!db.revoke_device("P1", 6).unwrap());
        assert_eq!(db.active_token("P1").unwrap(), None);
        assert!(db.is_revoked("P1").unwrap());
        assert!(db.is_revoked("nobody").unwrap());
        assert_eq!(db.devices().unwrap()[0].revoked_at, Some(5));
    }

    #[test]
    fn outbox_and_state() {
        let db = SyncDb::open_in_memory().unwrap();
        assert!(db.space().unwrap().is_none());
        db.push_outbox(&env("E1"), Some(("b1", b"h")), 0).unwrap();
        db.push_outbox(&env("E2"), None, 0).unwrap();
        db.push_outbox(&env("E2"), None, 0).unwrap();
        let rows = db.outbox(10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].blob, Some(("b1".to_owned(), b"h".to_vec())));
        db.remove_outbox(&["E1".into()]).unwrap();
        assert_eq!(db.outbox_len().unwrap(), 1);
        db.set_i64("last_seq", 9).unwrap();
        assert_eq!(db.get_i64("last_seq").unwrap(), 9);
        db.set_deleted_clock(b"h", 10).unwrap();
        db.set_deleted_clock(b"h", 5).unwrap();
        assert_eq!(db.clock(b"h").unwrap().deleted_at, Some(10));
    }
}
