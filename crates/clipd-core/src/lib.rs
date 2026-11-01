//! ClipTrail core. Platform-independent: no UI, no OS clipboard access.
//!
//! Flow: a platform watcher builds a [`RawClip`] → [`Pipeline::ingest`]
//! (normalize → hash → dedup → classify → rules → persist) → [`Store`].
//! UIs read through [`Store`].
//!
//! Rule: never log or put clip content into errors. Log ids, kinds, sizes.

pub mod classify;
pub mod model;
pub mod pipeline;
pub mod store;
pub mod synced;
pub mod transform;

pub use classify::classify_text;
pub use model::*;
pub use pipeline::{Pipeline, PipelineConfig};
pub use store::Store;
pub use synced::{ClipWithHash, JournalEntry, JournalOp, SyncedClip, SyncedImage};
pub use transform::{
    Transform, apply as apply_transform, apply_for as apply_transform_for,
    available as available_transforms,
};

use std::path::PathBuf;

/// Default data directory: `%APPDATA%\ClipTrail` on Windows,
/// `~/Library/Application Support/ClipTrail` on macOS, `~/.local/share/ClipTrail`
/// on Linux. Override with the `CLIPTRAIL_DATA_DIR` environment variable.
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CLIPTRAIL_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ClipTrail")
}

/// Current Unix time in milliseconds.
pub fn now_millis() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(0)
}
