//! Where the MasterKey and the device token live.
//!
//! Desktop: the OS keychain (Windows Credential Manager, macOS Keychain).
//! Elsewhere: a file in the data dir readable only by the user. Android
//! plugs its Keystore-backed storage in by implementing [`KeyStore`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use crate::crypto::{b64, unb64};

/// Name of the space MasterKey entry.
pub const MASTER_KEY: &str = "master-key";
/// Name of this device's token (clients only).
pub const DEVICE_TOKEN: &str = "device-token";

/// Small secret storage. Names are short ASCII identifiers; values are
/// raw bytes (32-byte keys). Implementations must never log values.
pub trait KeyStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>>;
    fn set(&self, name: &str, value: &[u8]) -> Result<()>;
    fn delete(&self, name: &str) -> Result<()>;
    /// Shown in `sync status`, e.g. "Windows Credential Manager".
    fn describe(&self) -> String;
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemoryKeyStore(Mutex<std::collections::HashMap<String, Vec<u8>>>);

impl KeyStore for MemoryKeyStore {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned())
    }
    fn set(&self, name: &str, value: &[u8]) -> Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_owned(), value.to_vec());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        Ok(())
    }
    fn describe(&self) -> String {
        "memory".into()
    }
}

/// One file per key under `<dir>/keys/`, base64url, mode 0600 on Unix.
/// (On Windows the data dir under %APPDATA% is already private to the
/// user.)
pub struct FileKeyStore {
    dir: PathBuf,
}

impl FileKeyStore {
    pub fn new(data_dir: &Path) -> FileKeyStore {
        FileKeyStore {
            dir: data_dir.join("keys"),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        let safe: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        self.dir.join(format!("{safe}.key"))
    }
}

impl KeyStore for FileKeyStore {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read_to_string(self.path(name)) {
            Ok(s) => Ok(Some(unb64(&s).context("key file is corrupt")?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("read key file"),
        }
    }

    fn set(&self, name: &str, value: &[u8]) -> Result<()> {
        std::fs::create_dir_all(&self.dir).context("create key dir")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        }
        let path = self.path(name);
        let tmp = path.with_extension("tmp");
        write_private(&tmp, b64(value).as_bytes()).context("write key file")?;
        std::fs::rename(&tmp, &path).context("replace key file")?;
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        match std::fs::remove_file(self.path(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).context("delete key file"),
        }
    }

    fn describe(&self) -> String {
        format!("file ({})", self.dir.display())
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// OS keychain entries named `ClipTrail` / `<scope>/<name>`. `scope` keeps
/// two data dirs on one machine (e.g. a test hub and client) apart.
#[cfg(all(
    feature = "os-keychain",
    any(target_os = "windows", target_os = "macos")
))]
pub struct OsKeyStore {
    scope: String,
}

#[cfg(all(
    feature = "os-keychain",
    any(target_os = "windows", target_os = "macos")
))]
impl OsKeyStore {
    pub const SERVICE: &'static str = "ClipTrail";

    pub fn new(scope: &str) -> Result<OsKeyStore> {
        static INIT: std::sync::OnceLock<std::result::Result<(), String>> =
            std::sync::OnceLock::new();
        INIT.get_or_init(|| {
            #[cfg(target_os = "windows")]
            let store = windows_native_keyring_store::Store::new();
            #[cfg(target_os = "macos")]
            let store = apple_native_keyring_store::keychain::Store::new();
            match store {
                Ok(s) => {
                    keyring_core::set_default_store(s);
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            }
        })
        .clone()
        .map_err(|e| anyhow::anyhow!("OS keychain unavailable: {e}"))?;
        Ok(OsKeyStore {
            scope: scope.to_owned(),
        })
    }

    fn entry(&self, name: &str) -> Result<keyring_core::Entry> {
        keyring_core::Entry::new(Self::SERVICE, &format!("{}/{name}", self.scope))
            .map_err(|e| anyhow::anyhow!("keychain entry: {e}"))
    }
}

#[cfg(all(
    feature = "os-keychain",
    any(target_os = "windows", target_os = "macos")
))]
impl KeyStore for OsKeyStore {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        match self.entry(name)?.get_password() {
            Ok(s) => Ok(Some(unb64(&s).context("keychain value is corrupt")?)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("keychain read failed: {e}")),
        }
    }

    fn set(&self, name: &str, value: &[u8]) -> Result<()> {
        self.entry(name)?
            .set_password(&b64(value))
            .map_err(|e| anyhow::anyhow!("keychain write failed: {e}"))
    }

    fn delete(&self, name: &str) -> Result<()> {
        match self.entry(name)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("keychain delete failed: {e}")),
        }
    }

    fn describe(&self) -> String {
        #[cfg(target_os = "windows")]
        return "Windows Credential Manager".into();
        #[cfg(target_os = "macos")]
        return "macOS Keychain".into();
    }
}

/// Name of the key that encrypts this device's local databases.
pub const DB_KEY: &str = "db-key";

/// Databases in a data dir that are encrypted with [`DB_KEY`].
const LOCAL_DBS: &[&str] = &[clipd_core::pipeline::DB_FILE, crate::engine::SYNC_DB_FILE];

/// Unlock the local databases of `data_dir` for this process: fetch the
/// database key from `ks` (creating it on first use) and register it with
/// [`clipd_core::dbkey::set_db_key`]. Call before opening the pipeline.
///
/// A key is only ever created while no database here is encrypted yet, so
/// a keychain that's unavailable can't lead to a second key and an
/// unreadable history; that case is an error instead.
pub fn unlock_data_dir(data_dir: &Path, ks: &dyn KeyStore) -> Result<()> {
    let key = match ks.get(DB_KEY)? {
        Some(k) => k,
        // Created while the keychain was unavailable (see default_keystore).
        None => match FileKeyStore::new(data_dir).get(DB_KEY)? {
            Some(k) => k,
            None => {
                if clipd_core::dbkey::any_encrypted(data_dir, LOCAL_DBS) {
                    anyhow::bail!(
                        "your clip history is encrypted but its key isn't in {}",
                        ks.describe()
                    );
                }
                let k = crate::crypto::random_bytes::<32>();
                ks.set(DB_KEY, &k).context("save the database key")?;
                k.to_vec()
            }
        },
    };
    let key: clipd_core::dbkey::DbKey = key
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("the database key in {} is corrupt", ks.describe()))?;
    clipd_core::dbkey::set_db_key(data_dir, key);
    Ok(())
}

/// The platform's preferred store for a data dir whose device id is
/// `device_id`: OS keychain where available, else [`FileKeyStore`].
/// `CLIPTRAIL_KEYSTORE=file` forces the file store.
pub fn default_keystore(data_dir: &Path, device_id: &str) -> Arc<dyn KeyStore> {
    let force_file = std::env::var("CLIPTRAIL_KEYSTORE").is_ok_and(|v| v == "file");
    #[cfg(all(
        feature = "os-keychain",
        any(target_os = "windows", target_os = "macos")
    ))]
    if !force_file {
        match OsKeyStore::new(device_id) {
            Ok(s) => return Arc::new(s),
            Err(e) => tracing::warn!("{e:#}; falling back to a key file"),
        }
    }
    let _ = (force_file, device_id);
    Arc::new(FileKeyStore::new(data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_roundtrip() {
        let dir = std::env::temp_dir().join(format!("clipd-keys-{}", ulid::Ulid::generate()));
        let ks = FileKeyStore::new(&dir);
        assert_eq!(ks.get(MASTER_KEY).unwrap(), None);
        ks.set(MASTER_KEY, &[1, 2, 3]).unwrap();
        assert_eq!(ks.get(MASTER_KEY).unwrap(), Some(vec![1, 2, 3]));
        ks.delete(MASTER_KEY).unwrap();
        ks.delete(MASTER_KEY).unwrap();
        assert_eq!(ks.get(MASTER_KEY).unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unlock_creates_one_key_and_never_replaces_it() {
        let dir = std::env::temp_dir().join(format!("clipd-unlock-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(&dir).unwrap();
        let ks = MemoryKeyStore::default();
        unlock_data_dir(&dir, &ks).unwrap();
        let key = ks.get(DB_KEY).unwrap().expect("key created");
        // An encrypted history now exists.
        let store = clipd_core::Store::open(&dir.join(clipd_core::pipeline::DB_FILE)).unwrap();
        drop(store);
        unlock_data_dir(&dir, &ks).unwrap();
        assert_eq!(ks.get(DB_KEY).unwrap(), Some(key));

        // Key gone (e.g. keychain wiped): refuse instead of making a new one.
        let empty = MemoryKeyStore::default();
        let err = unlock_data_dir(&dir, &empty).unwrap_err().to_string();
        assert!(err.contains("encrypted"), "{err}");
        assert_eq!(empty.get(DB_KEY).unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
