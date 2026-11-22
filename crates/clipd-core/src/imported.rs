//! Files the user imported to the phone from a copied-files clip.
//!
//! A copied file only syncs as a path; the phone fetches the file when the
//! user taps Import. This remembers which (clip, file) pairs were imported and
//! where they ended up (an Android content URI), so the clip can show Open and
//! Share instead of Import. It holds no file contents and no clip text.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "imported-files.json";
/// Oldest entries are dropped past this many.
const KEEP: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedFile {
    pub uri: String,
    pub mime: String,
    pub size: u64,
    pub at: i64,
}

/// A small JSON index in the data dir, keyed by `<clip id>:<file index>`.
pub struct ImportedFiles {
    path: PathBuf,
}

fn key(clip_id: &str, index: usize) -> String {
    format!("{clip_id}:{index}")
}

impl ImportedFiles {
    pub fn open(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(FILE_NAME),
        }
    }

    fn load(&self) -> BTreeMap<String, ImportedFile> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, map: &BTreeMap<String, ImportedFile>) -> Result<()> {
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(map)?).context("write imported-files index")?;
        std::fs::rename(&tmp, &self.path).context("replace imported-files index")
    }

    pub fn get(&self, clip_id: &str, index: usize) -> Option<ImportedFile> {
        self.load().remove(&key(clip_id, index))
    }

    pub fn put(&self, clip_id: &str, index: usize, file: ImportedFile) -> Result<()> {
        let mut map = self.load();
        map.insert(key(clip_id, index), file);
        if map.len() > KEEP {
            let mut by_age: Vec<_> = map.iter().map(|(k, v)| (v.at, k.clone())).collect();
            by_age.sort();
            for (_, k) in by_age.into_iter().take(map.len() - KEEP) {
                map.remove(&k);
            }
        }
        self.save(&map)
    }

    pub fn forget(&self, clip_id: &str, index: usize) -> Result<()> {
        let mut map = self.load();
        if map.remove(&key(clip_id, index)).is_some() {
            self.save(&map)?;
        }
        Ok(())
    }
}

/// The file name to save an imported file under: the last component of a
/// Windows or Unix path, with characters Android and Windows both reject
/// removed, never empty, never a dot-name.
pub fn safe_file_name(path: &str) -> String {
    let base = path.rsplit(['\\', '/']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_owned();
    if cleaned.is_empty() {
        "file".into()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_the_last_path_component() {
        assert_eq!(
            safe_file_name(r"C:\Users\me\Docs\Report 2026.pdf"),
            "Report 2026.pdf"
        );
        assert_eq!(safe_file_name("/home/me/a.txt"), "a.txt");
        assert_eq!(safe_file_name(r"C:\x\..\..\evil.pdf"), "evil.pdf");
        assert_eq!(safe_file_name(r"C:\x\a:b*c?.pdf"), "abc.pdf");
        assert_eq!(safe_file_name(r"C:\dir\"), "file");
        assert_eq!(safe_file_name(".."), "file");
        assert_eq!(safe_file_name(""), "file");
    }

    #[test]
    fn index_remembers_and_forgets() {
        let dir = std::env::temp_dir().join(format!("cliptrail-imp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let idx = ImportedFiles::open(&dir);
        assert!(idx.get("c1", 0).is_none());
        let f = ImportedFile {
            uri: "content://media/external/downloads/42".into(),
            mime: "application/pdf".into(),
            size: 1234,
            at: 7,
        };
        idx.put("c1", 0, f.clone()).unwrap();
        idx.put("c1", 1, ImportedFile { at: 8, ..f.clone() })
            .unwrap();
        assert_eq!(idx.get("c1", 0), Some(f));
        // A new handle reads what the old one wrote.
        assert!(ImportedFiles::open(&dir).get("c1", 1).is_some());
        idx.forget("c1", 0).unwrap();
        assert!(idx.get("c1", 0).is_none());
        assert!(idx.get("c1", 1).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_is_capped() {
        let dir = std::env::temp_dir().join(format!("cliptrail-imp-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let idx = ImportedFiles::open(&dir);
        for i in 0..(KEEP as i64 + 5) {
            let f = ImportedFile {
                uri: format!("u{i}"),
                mime: "x/y".into(),
                size: 1,
                at: i,
            };
            idx.put(&format!("c{i}"), 0, f).unwrap();
        }
        assert!(idx.get("c0", 0).is_none());
        assert!(idx.get(&format!("c{}", KEEP + 4), 0).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
