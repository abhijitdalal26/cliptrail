//! Logic shared by hub and client: local journal → events, remote events →
//! the clip store, image blobs. Synchronous (SQLite); async callers wrap
//! calls in `spawn_blocking`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clipd_core::{
    ClipWithHash, IngestOutcome, JournalOp, Kind, ListQuery, Pipeline, PipelineConfig, SyncedClip,
    SyncedImage, now_millis,
};
use serde::Serialize;

use crate::crypto::{SecretKey, SpaceKeys, b64, unb64};
use crate::db::{Role, SpaceState, SyncDb};
use crate::envelope::{ClipAdded, Envelope, Event, ImageRef, blob_aad};
use crate::keystore::{KeyStore, MASTER_KEY};

/// Thumbnails up to this size travel inline in `ClipAdded`.
pub const INLINE_THUMB_MAX: usize = 256 * 1024;
/// How many recent clips a new space / newly paired device shares.
pub const BACKFILL_LIMIT: u32 = 1000;
/// Journal entries turned into events per pass.
pub const JOURNAL_BATCH: u32 = 200;

pub const SYNC_DB_FILE: &str = "sync.db";
/// Hub: sealed blobs uploaded before their event arrived.
pub const BLOB_DIR: &str = "sync-blobs";

/// What sync tells its embedder (UI, CLI). Never carries clip content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncNotice {
    /// Clips arrived from (or were changed by) other devices.
    ClipsChanged { count: usize },
    /// Connection / device list changed.
    StatusChanged,
    /// Hub: a device just paired.
    Paired {
        device_id: String,
        name: String,
        platform: String,
        check_code: String,
    },
    /// An event was applied (metadata only).
    Event {
        seq: i64,
        kind: String,
        device_id: String,
        event_id: String,
        applied: bool,
    },
}

/// A local change ready to seal.
#[derive(Debug, Clone)]
pub struct LocalEvent {
    pub event: Event,
    /// (blob_id, content hash) when the event references a full image.
    pub blob: Option<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, Default)]
pub struct Applied {
    pub changed: bool,
    /// Full image still needed: (blob_id, content hash).
    pub blob: Option<(String, Vec<u8>)>,
}

pub struct Engine {
    pub pipeline: Pipeline,
    pub db: SyncDb,
    pub keys: SpaceKeys,
    pub space: SpaceState,
    pub data_dir: PathBuf,
}

pub fn sync_db_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SYNC_DB_FILE)
}

pub fn open_db(data_dir: &Path) -> Result<SyncDb> {
    std::fs::create_dir_all(data_dir).context("create data dir")?;
    SyncDb::open(&sync_db_path(data_dir))
}

pub fn open_pipeline(data_dir: &Path) -> Result<Pipeline> {
    Pipeline::open(PipelineConfig::with_defaults(data_dir.to_path_buf())?)
}

impl Engine {
    /// Open the engine for a configured data dir. Errors when sync isn't
    /// set up or the key is missing from the key store.
    pub fn open(data_dir: &Path, keystore: &dyn KeyStore) -> Result<Engine> {
        let db = open_db(data_dir)?;
        let space = db
            .space()?
            .context("sync is not set up for this data dir")?;
        let master = keystore
            .get(MASTER_KEY)?
            .context("the sync key is missing from the key store; reset sync and pair again")?;
        let master = SecretKey::from_slice(&master).context("sync key is corrupt")?;
        let pipeline = open_pipeline(data_dir)?;
        if pipeline.config().device_id != space.device_id {
            bail!("this data dir's device id changed; reset sync and pair again");
        }
        Ok(Engine {
            pipeline,
            db,
            keys: SpaceKeys::new(master),
            space,
            data_dir: data_dir.to_path_buf(),
        })
    }

    pub fn device_id(&self) -> &str {
        &self.space.device_id
    }

    pub fn role(&self) -> Role {
        self.space.role
    }

    pub fn seal(&self, event: &Event) -> Result<Envelope> {
        Ok(Envelope::seal(
            &self.keys,
            &self.space.space_id,
            &self.space.device_id,
            now_millis(),
            event,
        )?)
    }

    /// `ClipAdded` for a stored clip (None for masked clips).
    fn clip_event(&self, c: &ClipWithHash) -> Option<LocalEvent> {
        let s = &c.clip.summary;
        if s.masked || s.kind == Kind::Sensitive {
            return None;
        }
        let hash = b64(&c.content_hash);
        let mut blob = None;
        let image = (s.kind == Kind::Image).then(|| {
            let blob_id = self.keys.blob_id(&c.content_hash);
            blob = Some((blob_id.clone(), c.content_hash.clone()));
            let thumb = s
                .thumb_path
                .as_deref()
                .and_then(|p| std::fs::read(p).ok())
                .filter(|b| b.len() <= INLINE_THUMB_MAX)
                .map(|b| b64(&b));
            ImageRef {
                blob_id,
                w: s.width.unwrap_or(0).max(0) as u32,
                h: s.height.unwrap_or(0).max(0) as u32,
                thumb,
            }
        });
        Some(LocalEvent {
            event: Event::ClipAdded(Box::new(ClipAdded {
                hash,
                kind: s.kind,
                subkind: s.subkind.clone(),
                text: if s.kind == Kind::Image {
                    None
                } else {
                    c.clip.text.clone()
                },
                html: c.clip.html.clone(),
                files: s.files.clone(),
                source_app: s.source_app.clone(),
                origin: s.device_id.clone(),
                created_at: s.created_at,
                last_seen_at: s.last_seen_at,
                copy_count: s.copy_count,
                image,
            })),
            blob,
        })
    }

    /// New local changes since the journal cursor, plus the cursor to
    /// store once they are safely sealed and queued.
    pub fn collect_local(&self) -> Result<(Vec<LocalEvent>, i64)> {
        let cursor = self.db.get_i64("journal_cursor")?;
        let entries = self.pipeline.store().journal_since(cursor, JOURNAL_BATCH)?;
        let mut new_cursor = cursor;
        let mut out = Vec::new();
        for e in entries {
            new_cursor = e.seq;
            match e.op {
                JournalOp::Upsert => {
                    if let Some(c) = self.pipeline.store().get_by_hash(&e.content_hash)?
                        && let Some(ev) = self.clip_event(&c)
                    {
                        if let Some((blob_id, hash)) = &ev.blob {
                            self.db.put_blob_index(blob_id, hash)?;
                        }
                        out.push(ev);
                    }
                }
                JournalOp::Pin => {
                    if let Some(c) = self.pipeline.store().get_by_hash(&e.content_hash)? {
                        self.db
                            .set_pin_clock(&e.content_hash, e.at, self.device_id())?;
                        out.push(LocalEvent {
                            event: Event::ClipUpdated {
                                hash: b64(&e.content_hash),
                                pinned: c.clip.summary.pinned,
                                at: e.at,
                            },
                            blob: None,
                        });
                    }
                }
                JournalOp::Delete => {
                    self.db.set_deleted_clock(&e.content_hash, e.at)?;
                    out.push(LocalEvent {
                        event: Event::ClipDeleted {
                            hash: b64(&e.content_hash),
                            at: e.at,
                        },
                        blob: None,
                    });
                }
            }
        }
        Ok((out, new_cursor))
    }

    pub fn commit_cursor(&self, cursor: i64) -> Result<()> {
        self.db.set_i64("journal_cursor", cursor)
    }

    /// The newest `limit` shareable clips as events, oldest first (pinned
    /// ones followed by their pin state). Used when a space is created or a
    /// device pairs, so history shows up on the other side.
    pub fn backfill(&self, limit: u32) -> Result<Vec<LocalEvent>> {
        let rows = self.pipeline.store().list(&ListQuery {
            limit: Some(limit),
            ..Default::default()
        })?;
        let mut out = Vec::new();
        for s in rows.iter().rev().filter(|s| !s.masked) {
            let Some(hash) = self.pipeline.store().content_hash(&s.id)? else {
                continue;
            };
            let Some(c) = self.pipeline.store().get_by_hash(&hash)? else {
                continue;
            };
            if let Some(ev) = self.clip_event(&c) {
                if let Some((blob_id, h)) = &ev.blob {
                    self.db.put_blob_index(blob_id, h)?;
                }
                out.push(ev);
                if s.pinned {
                    out.push(LocalEvent {
                        event: Event::ClipUpdated {
                            hash: b64(&hash),
                            pinned: true,
                            at: s.last_seen_at,
                        },
                        blob: None,
                    });
                }
            }
        }
        Ok(out)
    }

    /// Decrypt and apply an envelope from another device. Own envelopes
    /// (echoes) are skipped. Returns the event kind for logging.
    pub fn apply_envelope(&self, env: &Envelope) -> Result<(&'static str, Applied)> {
        if env.device_id == self.space.device_id {
            return Ok(("own", Applied::default()));
        }
        let event = env.open(&self.keys, &self.space.space_id)?;
        let kind = event.kind_name();
        let applied = self.apply_event(&event, &env.device_id)?;
        Ok((kind, applied))
    }

    pub fn apply_event(&self, event: &Event, from_device: &str) -> Result<Applied> {
        let hash = event.hash()?;
        let clock = self.db.clock(&hash)?;
        match event {
            Event::ClipAdded(c) => {
                if clock.deleted_at.is_some_and(|d| d >= c.last_seen_at) {
                    // Deleted after this copy: stale add.
                    return Ok(Applied::default());
                }
                let image = c.image.as_ref().map(|i| SyncedImage {
                    width: i.w,
                    height: i.h,
                    thumb_png: i.thumb.as_deref().and_then(|t| unb64(t).ok()),
                });
                let origin = if c.origin.is_empty() {
                    from_device.to_owned()
                } else {
                    c.origin.clone()
                };
                let outcome = self.pipeline.ingest_synced(SyncedClip {
                    content_hash: hash.clone(),
                    kind: c.kind,
                    subkind: c.subkind.clone(),
                    text: c.text.clone(),
                    html: c.html.clone(),
                    files: c.files.clone(),
                    source_app: c.source_app.clone(),
                    device_id: origin,
                    created_at: c.created_at,
                    last_seen_at: c.last_seen_at,
                    copy_count: c.copy_count,
                    image,
                })?;
                let id = match outcome {
                    IngestOutcome::Stored { id, .. } | IngestOutcome::Bumped { id, .. } => id,
                    IngestOutcome::Skipped { reason } => {
                        tracing::debug!(%reason, "synced clip skipped");
                        return Ok(Applied::default());
                    }
                };
                let mut blob = None;
                if let Some(img) = &c.image {
                    self.db.put_blob_index(&img.blob_id, &hash)?;
                    let has_full = self
                        .pipeline
                        .store()
                        .get(&id)?
                        .is_some_and(|clip| clip.image_path.is_some());
                    if !has_full {
                        self.db.add_pending_blob(&img.blob_id, &hash)?;
                        blob = Some((img.blob_id.clone(), hash));
                    }
                }
                Ok(Applied {
                    changed: true,
                    blob,
                })
            }
            Event::ClipUpdated { pinned, at, .. } => {
                let newer = match (clock.pin_at, clock.pin_dev.as_deref()) {
                    (None, _) => true,
                    (Some(p), dev) => *at > p || (*at == p && Some(from_device) > dev),
                };
                if !newer {
                    return Ok(Applied::default());
                }
                self.db.set_pin_clock(&hash, *at, from_device)?;
                let changed = self
                    .pipeline
                    .store()
                    .set_pinned_synced(&hash, *pinned)?
                    .is_some();
                Ok(Applied {
                    changed,
                    blob: None,
                })
            }
            Event::ClipDeleted { at, .. } => {
                self.db.set_deleted_clock(&hash, *at)?;
                let changed = self.pipeline.delete_synced(&hash, *at)?.is_some();
                Ok(Applied {
                    changed,
                    blob: None,
                })
            }
        }
    }

    /// Seal the full image of the clip with this hash for upload/serving.
    pub fn seal_blob(&self, blob_id: &str, hash: &[u8]) -> Result<Option<Vec<u8>>> {
        let Some(c) = self.pipeline.store().get_by_hash(hash)? else {
            return Ok(None);
        };
        let Some(path) = c.clip.image_path else {
            return Ok(None);
        };
        let png = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("read image file"),
        };
        Ok(Some(self.keys.blobs.seal_packed(
            &blob_aad(&self.space.space_id, blob_id),
            &png,
        )?))
    }

    /// Open a sealed blob and attach it to its clip. Ok(false) when the
    /// clip isn't (or no longer) here.
    pub fn attach_blob(&self, blob_id: &str, hash: &[u8], sealed: &[u8]) -> Result<bool> {
        let png = self
            .keys
            .blobs
            .open_packed(&blob_aad(&self.space.space_id, blob_id), sealed)
            .context("image blob does not decrypt")?;
        let Some(id) = self.pipeline.store().id_for_hash(hash)? else {
            return Ok(false);
        };
        self.pipeline.attach_synced_image(&id, &png)?;
        Ok(true)
    }

    /// Check a sealed blob decrypts under the space key (hub uploads).
    pub fn verify_blob(&self, blob_id: &str, sealed: &[u8]) -> bool {
        self.keys
            .blobs
            .open_packed(&blob_aad(&self.space.space_id, blob_id), sealed)
            .is_ok()
    }

    pub fn blob_dir(&self) -> PathBuf {
        self.data_dir.join(BLOB_DIR)
    }
}

pub type SharedEngine = Arc<Engine>;

/// Resolves once `rx` reads true (or its sender is gone). Returns `()` so
/// it can sit in a `select!` of a `Send` future.
pub(crate) async fn stopped(rx: &mut tokio::sync::watch::Receiver<bool>) {
    let _ = rx.wait_for(|s| *s).await;
}

/// Valid opaque blob id (22 base64url chars).
pub fn valid_blob_id(id: &str) -> bool {
    id.len() == 22
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
