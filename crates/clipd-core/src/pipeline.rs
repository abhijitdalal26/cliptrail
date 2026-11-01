//! normalize → hash → dedup → classify → rules → persist.
//! STUB: signatures are the contract, bodies to be implemented.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::model::*;
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Root for `clips.db`, `images/`, `thumbs/`.
    pub data_dir: PathBuf,
    /// Stable id of this device (persisted in `data_dir/device_id`).
    pub device_id: String,
    /// Lowercased executable names whose copies are never stored
    /// (password managers by default).
    pub excluded_apps: Vec<String>,
    /// How long sensitive clips live before pruning (ms).
    pub sensitive_ttl_ms: Millis,
    /// Texts longer than this (bytes) are skipped.
    pub max_text_bytes: usize,
}

impl PipelineConfig {
    /// Defaults rooted at `data_dir`; creates the dir and device id file.
    pub fn with_defaults(data_dir: PathBuf) -> Result<PipelineConfig> {
        let _ = data_dir;
        todo!()
    }
}

pub struct Pipeline {
    _private: (),
}

impl Pipeline {
    /// Open `data_dir/clips.db` and build a pipeline.
    pub fn open(config: PipelineConfig) -> Result<Pipeline> {
        let _ = config;
        todo!()
    }

    pub fn store(&self) -> &Arc<Store> {
        todo!()
    }

    pub fn config(&self) -> &PipelineConfig {
        todo!()
    }

    pub fn ingest(&self, raw: RawClip) -> Result<IngestOutcome> {
        let _ = raw;
        todo!()
    }

    /// Delete a clip and its files on disk.
    pub fn delete(&self, id: &str) -> Result<()> {
        let _ = id;
        todo!()
    }

    /// Remove expired clips and their files. Returns how many were removed.
    pub fn prune(&self) -> Result<usize> {
        todo!()
    }
}
