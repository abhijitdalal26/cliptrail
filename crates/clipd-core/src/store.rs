//! SQLite storage + full-text search. STUB: signatures are the contract,
//! bodies to be implemented.

use std::path::Path;

use anyhow::Result;

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

/// Thread-safe handle (internally a mutex-guarded connection), cheap to share
/// behind an `Arc`.
pub struct Store {
    _private: (),
}

impl Store {
    /// Open (creating + migrating if needed) the database at `path`.
    pub fn open(path: &Path) -> Result<Store> {
        let _ = path;
        todo!()
    }

    pub fn open_in_memory() -> Result<Store> {
        todo!()
    }

    /// Insert, or if `content_hash` already exists bump that row's
    /// last_seen_at = clip.created_at and copy_count += 1.
    /// Returns (id, inserted?).
    pub fn upsert(&self, clip: NewClip) -> Result<(String, bool)> {
        let _ = clip;
        todo!()
    }

    pub fn list(&self, q: &ListQuery) -> Result<Vec<ClipSummary>> {
        let _ = q;
        todo!()
    }

    pub fn get(&self, id: &str) -> Result<Option<Clip>> {
        let _ = id;
        todo!()
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        let _ = (id, pinned);
        todo!()
    }

    /// Delete a clip. Returns paths of image/thumbnail files the caller
    /// should remove from disk.
    pub fn delete(&self, id: &str) -> Result<Vec<String>> {
        let _ = id;
        todo!()
    }

    pub fn stats(&self) -> Result<Stats> {
        todo!()
    }

    /// Per-local-day counts over last_seen_at in [from, to).
    pub fn day_counts(&self, from: Millis, to: Millis) -> Result<Vec<DayCount>> {
        let _ = (from, to);
        todo!()
    }

    /// Delete clips whose expires_at <= now. Returns file paths to remove.
    pub fn prune_expired(&self, now: Millis) -> Result<Vec<String>> {
        let _ = now;
        todo!()
    }

    /// Distinct source apps, most used first.
    pub fn source_apps(&self) -> Result<Vec<String>> {
        todo!()
    }
}
