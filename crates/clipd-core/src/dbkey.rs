//! At-rest encryption of the local databases.
//!
//! The bundled SQLite is SQLite3 Multiple Ciphers (see `third_party/`);
//! databases are encrypted page by page with ChaCha20-Poly1305, the same
//! cipher family sync uses. A process unlocks a data dir once with
//! [`set_db_key`] (the key lives in the OS keychain, see
//! `clipd_sync::unlock_data_dir`); every database opened under that dir
//! with [`open_connection`] then uses it. A plaintext database from before
//! encryption is encrypted in place the first time it's opened with a key.
//!
//! Without a registered key, plaintext databases open as before (tests,
//! the relay) and an encrypted one fails with a clear error: it is never
//! overwritten or recreated.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

/// 256-bit key for the databases of one data dir.
pub type DbKey = [u8; 32];

static KEYS: Mutex<Vec<(PathBuf, DbKey)>> = Mutex::new(Vec::new());

/// Plain SQLite files start with this; encrypted ones start with a random salt.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

fn normalize(dir: &Path) -> PathBuf {
    std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf())
}

/// Use `key` for every database in `data_dir` opened by this process.
pub fn set_db_key(data_dir: &Path, key: DbKey) {
    let dir = normalize(data_dir);
    let mut keys = KEYS.lock().unwrap_or_else(|e| e.into_inner());
    keys.retain(|(d, _)| *d != dir);
    keys.push((dir, key));
}

/// True once [`set_db_key`] ran for `data_dir`.
pub fn has_db_key(data_dir: &Path) -> bool {
    let dir = normalize(data_dir);
    KEYS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|(d, _)| *d == dir)
}

fn key_for(db_path: &Path) -> Option<DbKey> {
    key_for_dir(db_path.parent()?)
}

/// The key registered for `data_dir`, if it's unlocked.
pub(crate) fn key_for_dir(data_dir: &Path) -> Option<DbKey> {
    let dir = normalize(data_dir);
    KEYS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(d, _)| *d == dir)
        .map(|(_, k)| *k)
}

/// What's on disk at a database path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbFile {
    /// No file, or an empty one: a new database.
    Missing,
    Plain,
    Encrypted,
}

pub fn db_file_state(path: &Path) -> Result<DbFile> {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(DbFile::Missing),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    let mut head = [0u8; 16];
    let mut n = 0;
    while n < head.len() {
        match f.read(&mut head[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(match n {
        0 => DbFile::Missing,
        16 if &head == SQLITE_MAGIC => DbFile::Plain,
        _ => DbFile::Encrypted,
    })
}

/// True if any database in `data_dir` named in `files` is encrypted.
pub fn any_encrypted(data_dir: &Path, files: &[&str]) -> bool {
    files
        .iter()
        .any(|f| matches!(db_file_state(&data_dir.join(f)), Ok(DbFile::Encrypted)))
}

fn hex(key: &DbKey) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

/// Key an open connection. Must run before anything else touches it.
fn apply_key(conn: &Connection, key: &DbKey, pragma: &str) -> Result<()> {
    // The cipher is pinned rather than left to the library default.
    conn.execute_batch(&format!(
        "PRAGMA cipher = 'chacha20'; PRAGMA {pragma} = '{}';",
        hex(key)
    ))
    .context("set database key")
}

/// Fails with "not a database" when the key is wrong.
fn verify_readable(conn: &Connection) -> rusqlite::Result<()> {
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|_| ())
}

/// Open the database at `path`, keyed when its data dir is unlocked.
/// Callers then set their own journal mode and run migrations.
pub fn open_connection(path: &Path) -> Result<Connection> {
    let state = db_file_state(path)?;
    let Some(key) = key_for(path) else {
        if state == DbFile::Encrypted {
            bail!("it's encrypted and this process has no key for it");
        }
        return Connection::open(path).with_context(|| format!("open {}", path.display()));
    };
    if state == DbFile::Plain {
        // A failure leaves the plaintext file as it was; try again next
        // open (e.g. another process held it).
        match encrypt_in_place(path, &key) {
            Ok(()) => tracing::info!(db = %path.display(), "database encrypted"),
            // Another process may have just encrypted it: then open keyed.
            Err(_) if db_file_state(path)? == DbFile::Encrypted => {}
            Err(e) => {
                tracing::warn!(db = %path.display(), "couldn't encrypt database yet: {e:#}");
                return Connection::open(path).with_context(|| format!("open {}", path.display()));
            }
        }
    }
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))?;
    apply_key(&conn, &key, "hexkey")?;
    if let Err(e) = verify_readable(&conn) {
        if state == DbFile::Missing {
            return Err(e).context("create encrypted database");
        }
        bail!("can't unlock it: the key doesn't match ({e})");
    }
    Ok(conn)
}

/// Encrypt a plaintext database file in place.
fn encrypt_in_place(path: &Path, key: &DbKey) -> Result<()> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    // Rekeying needs a rollback journal: fold the WAL back in first.
    let mode: String = conn.query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("delete") {
        bail!("database is busy (journal mode stayed {mode})");
    }
    // Drop free pages (old plaintext) before the rewrite.
    conn.execute_batch("VACUUM")?;
    apply_key(&conn, key, "hexrekey")?;
    drop(conn);

    if db_file_state(path)? != DbFile::Encrypted {
        bail!("rekey left the file unencrypted");
    }
    let check = Connection::open(path)?;
    apply_key(&check, key, "hexkey")?;
    verify_readable(&check).context("verify encrypted database")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> TempDir {
            let d = std::env::temp_dir().join(format!("clipd-dbkey-{}", ulid::Ulid::generate()));
            std::fs::create_dir_all(&d).unwrap();
            TempDir(d)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write_plain(path: &Path, secret: &str) {
        let c = Connection::open(path).unwrap();
        c.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))
            .unwrap();
        c.execute_batch("CREATE TABLE t(v TEXT)").unwrap();
        c.execute("INSERT INTO t VALUES (?1)", [secret]).unwrap();
    }

    fn read_back(c: &Connection) -> String {
        c.query_row("SELECT v FROM t", [], |r| r.get(0)).unwrap()
    }

    fn file_contains(path: &Path, needle: &str) -> bool {
        let bytes = std::fs::read(path).unwrap();
        bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    #[test]
    fn new_database_is_encrypted_and_reopens() {
        let dir = TempDir::new();
        set_db_key(&dir.0, [7; 32]);
        let path = dir.0.join("a.db");
        {
            let c = open_connection(&path).unwrap();
            c.execute_batch("CREATE TABLE t(v TEXT); INSERT INTO t VALUES ('hunter2-secret');")
                .unwrap();
        }
        assert_eq!(db_file_state(&path).unwrap(), DbFile::Encrypted);
        assert!(!file_contains(&path, "hunter2-secret"));
        let c = open_connection(&path).unwrap();
        assert_eq!(read_back(&c), "hunter2-secret");
    }

    #[test]
    fn plaintext_database_is_migrated() {
        let dir = TempDir::new();
        let path = dir.0.join("b.db");
        write_plain(&path, "plain-secret-text");
        assert_eq!(db_file_state(&path).unwrap(), DbFile::Plain);

        set_db_key(&dir.0, [9; 32]);
        let c = open_connection(&path).unwrap();
        assert_eq!(read_back(&c), "plain-secret-text");
        c.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))
            .unwrap();
        c.execute("INSERT INTO t VALUES ('after')", []).unwrap();
        drop(c);
        assert_eq!(db_file_state(&path).unwrap(), DbFile::Encrypted);
        assert!(!file_contains(&path, "plain-secret-text"));
        assert!(
            !dir.0.join("b.db-wal").exists() || !file_contains(&dir.0.join("b.db-wal"), "after")
        );
    }

    #[test]
    fn wrong_or_missing_key_fails_without_touching_the_file() {
        let dir = TempDir::new();
        let path = dir.0.join("c.db");
        set_db_key(&dir.0, [1; 32]);
        open_connection(&path)
            .unwrap()
            .execute_batch("CREATE TABLE t(v TEXT)")
            .unwrap();
        let before = std::fs::read(&path).unwrap();

        set_db_key(&dir.0, [2; 32]);
        let err = open_connection(&path).unwrap_err().to_string();
        assert!(err.contains("key doesn't match"), "{err}");

        KEYS.lock()
            .unwrap()
            .retain(|(d, _)| *d != normalize(&dir.0));
        let err = open_connection(&path).unwrap_err().to_string();
        assert!(err.contains("no key"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
