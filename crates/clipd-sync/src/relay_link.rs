//! Devices ↔ relays: the HTTP/WebSocket client for a `clipd-relay`, the
//! per-relay sync loops, and the API the CLI and UIs call (add / remove /
//! check / status). No server dependencies (Android builds this).
//!
//! Topology. The LAN hub stays the fast path. Every relay a space knows is
//! kept in step by two kinds of loops:
//!
//! - **Hub bridge** (inside the hub process): forwards the whole hub log to
//!   the relay (so a phone on mobile data sees laptop clips even after the
//!   laptop sleeps) and pulls what other devices posted there into the hub
//!   log (so LAN devices see it too).
//! - **Client link** (phone, second desktop): used only while the LAN hub
//!   is unreachable. It pushes the outbox to the relay and pulls/streams
//!   from it; once the hub answers again it goes back to standby.
//!
//! Envelopes are idempotent by `event_id` and every device remembers which
//! ids it has seen, so an event that arrives through both paths is applied
//! once.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clipd_core::{Millis, now_millis};
use futures_util::{SinkExt, StreamExt};
use reqwest::Method;
use serde::Serialize;
use tokio::sync::{Notify, broadcast, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use crate::crypto::b64;
use crate::db::{Role, SyncDb};
use crate::engine::{Engine, SyncNotice};
use crate::envelope::{Envelope, Event, SeqEnvelope};
use crate::keystore::KeyStore;
use crate::relay::{
    self, DeviceKey, EnrollRequest, RELAY_KEY, RELAY_PROTO, RegisterRequest, RelayHello,
    RelayPushItem, RelayPushRequest, RotateEnrollRequest, SpaceInfo,
};
use crate::wire::{EventsPage, PushResponse, StreamFrame};
use crate::{client_enqueue, hub_append};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const PUSH_BATCH: u32 = 100;
const PAGE: u32 = 200;
const STREAM_IDLE: Duration = Duration::from_secs(75);
const SUPERVISE_EVERY: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(120);
/// How long seen event ids are remembered.
const SEEN_TTL_MS: Millis = 90 * 24 * 3600 * 1000;
/// Client: give the LAN hub this long after start before using a relay.
const LAN_GRACE: Duration = Duration::from_secs(4);
const BLOB_GIVE_UP: i64 = 12;

// ---- TLS / HTTP --------------------------------------------------------------

/// rustls with the ring provider and Mozilla's root store (webpki-roots),
/// built once. Used for every HTTP client in this crate, so nothing
/// depends on a process-wide default provider.
pub fn tls_config() -> rustls::ClientConfig {
    static CFG: OnceLock<rustls::ClientConfig> = OnceLock::new();
    CFG.get_or_init(|| {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring supports TLS 1.2 and 1.3")
        .with_root_certificates(roots)
        .with_no_client_auth()
    })
    .clone()
}

/// A reqwest builder with our TLS config (plain HTTP still works).
pub fn http_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls_config())
        .no_proxy()
        .user_agent(concat!("ClipTrail-sync/", env!("CARGO_PKG_VERSION")))
}

fn relay_http() -> Result<reqwest::Client> {
    http_builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("build HTTP client")
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| anyhow!("sync worker panicked"))?
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

async fn error_message(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.bytes().await.unwrap_or_default();
    serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_owned))
        .map(|e| e.chars().take(200).collect())
        .unwrap_or_else(|| format!("HTTP {status}"))
}

/// The relay refused this device's key (revoked, or never enrolled).
#[derive(Debug, thiserror::Error)]
#[error("the relay does not accept this device: {0}")]
pub struct RelayDenied(pub String);

// ---- keys ----------------------------------------------------------------------

/// This device's relay key, created on first use.
pub fn device_key(keystore: &dyn KeyStore) -> Result<DeviceKey> {
    if let Some(seed) = keystore.get(RELAY_KEY)? {
        return DeviceKey::from_seed(&seed).context("relay key in the key store is corrupt");
    }
    let key = DeviceKey::generate();
    keystore
        .set(RELAY_KEY, &key.seed())
        .context("store the relay key")?;
    Ok(key)
}

// ---- one relay connection ------------------------------------------------------

/// A signed client for one relay, as one device of one space.
pub struct RelayConn {
    pub base: String,
    pub rsid: String,
    device_id: String,
    key: DeviceKey,
    http: reqwest::Client,
}

/// `GET /v1/hello` of a relay (no auth). Also tells a relay from a LAN hub.
pub async fn hello(http: &reqwest::Client, base: &str) -> Result<RelayHello> {
    let resp = http
        .get(format!("{base}/v1/hello"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .with_context(|| format!("relay at {base} unreachable"))?;
    if !resp.status().is_success() {
        bail!("relay at {base}: {}", error_message(resp).await);
    }
    let body = resp.bytes().await.context("read relay hello")?;
    let h: RelayHello =
        serde_json::from_slice(&body).map_err(|_| anyhow!("{base} is not a ClipTrail relay"))?;
    if h.kind != "clipd-relay" {
        bail!("{base} is not a ClipTrail relay");
    }
    if h.proto != RELAY_PROTO {
        bail!(
            "relay at {base} speaks protocol v{} (this app: v{RELAY_PROTO}); update one of them",
            h.proto
        );
    }
    Ok(h)
}

impl RelayConn {
    pub fn new(base: &str, rsid: &str, device_id: &str, key: DeviceKey) -> Result<RelayConn> {
        Ok(RelayConn {
            base: relay::normalize_url(base)?,
            rsid: rsid.to_owned(),
            device_id: device_id.to_owned(),
            key,
            http: relay_http()?,
        })
    }

    pub fn public_key(&self) -> String {
        self.key.public_b64()
    }

    fn sign(&self, method: &str, pq: &str, body: &[u8]) -> String {
        relay::sign_request(
            &self.rsid,
            &self.device_id,
            &self.key,
            method,
            pq,
            now_millis(),
            body,
        )
    }

    /// Signed request; 401/403 become [`RelayDenied`].
    async fn send(&self, method: Method, pq: &str, body: Vec<u8>) -> Result<reqwest::Response> {
        let sig = self.sign(method.as_str(), pq, &body);
        let mut req = self
            .http
            .request(method, format!("{}{pq}", self.base))
            .header("authorization", sig);
        if !body.is_empty() {
            req = req
                .header("content-type", "application/octet-stream")
                .body(body);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("relay at {} unreachable", self.base))?;
        let st = resp.status();
        if st == reqwest::StatusCode::UNAUTHORIZED || st == reqwest::StatusCode::FORBIDDEN {
            let msg = error_message(resp).await;
            if msg.contains("clock") {
                bail!("relay refused the request: {msg} (check this device's clock)");
            }
            return Err(RelayDenied(msg).into());
        }
        Ok(resp)
    }

    async fn ok_bytes(
        &self,
        method: Method,
        pq: &str,
        body: Vec<u8>,
        what: &str,
    ) -> Result<Vec<u8>> {
        let resp = self.send(method, pq, body).await?;
        if !resp.status().is_success() {
            bail!("relay {what} failed: {}", error_message(resp).await);
        }
        Ok(resp.bytes().await.context(what.to_owned())?.to_vec())
    }

    /// Create the space on the relay with this device's key and the space
    /// enroll key. Err with "already registered" when it exists.
    pub async fn register(&self, enroll: &DeviceKey, invite: Option<&str>) -> Result<()> {
        let body = serde_json::to_vec(&RegisterRequest {
            space_id: self.rsid.clone(),
            device_id: self.device_id.clone(),
            device_pub: self.public_key(),
            enroll_pub: enroll.public_b64(),
            invite: invite.map(str::to_owned),
        })?;
        let pq = "/v1/spaces";
        let resp = self
            .http
            .post(format!("{}{pq}", self.base))
            .header("authorization", self.sign("POST", pq, &body))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .with_context(|| format!("relay at {} unreachable", self.base))?;
        match resp.status() {
            s if s.is_success() => Ok(()),
            reqwest::StatusCode::CONFLICT => {
                bail!(
                    "this space is already registered on that relay (another of your devices may have added it)"
                )
            }
            _ => bail!("relay refused registration: {}", error_message(resp).await),
        }
    }

    /// Register this device's key with the space enroll key.
    pub async fn enroll(&self, enroll: &DeviceKey) -> Result<()> {
        let ts = now_millis();
        let device_pub = self.public_key();
        let proof = enroll.sign(&relay::enroll_message(
            &self.rsid,
            &self.device_id,
            &device_pub,
            ts,
        ));
        let body = serde_json::to_vec(&EnrollRequest {
            space_id: self.rsid.clone(),
            device_id: self.device_id.clone(),
            device_pub,
            ts,
            proof: b64(&proof),
        })?;
        let pq = "/v1/enroll";
        let resp = self
            .http
            .post(format!("{}{pq}", self.base))
            .header("authorization", self.sign("POST", pq, &body))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .with_context(|| format!("relay at {} unreachable", self.base))?;
        if resp.status().is_success() {
            return Ok(());
        }
        let st = resp.status();
        let msg = error_message(resp).await;
        if st == reqwest::StatusCode::FORBIDDEN || st == reqwest::StatusCode::UNAUTHORIZED {
            return Err(RelayDenied(msg).into());
        }
        bail!("relay refused enrollment: {msg}")
    }

    pub async fn push(&self, items: Vec<RelayPushItem>) -> Result<PushResponse> {
        let body = serde_json::to_vec(&RelayPushRequest { events: items })?;
        let bytes = self
            .ok_bytes(Method::POST, "/v1/events", body, "push")
            .await?;
        serde_json::from_slice(&bytes).context("malformed relay push reply")
    }

    pub async fn events(&self, since: i64, limit: u32) -> Result<EventsPage> {
        let bytes = self
            .ok_bytes(
                Method::GET,
                &format!("/v1/events?since={since}&limit={limit}"),
                Vec::new(),
                "catch-up",
            )
            .await?;
        serde_json::from_slice(&bytes).context("malformed relay events page")
    }

    pub async fn put_blob(&self, blob_id: &str, sealed: Vec<u8>) -> Result<()> {
        self.ok_bytes(
            Method::PUT,
            &format!("/v1/blobs/{blob_id}"),
            sealed,
            "image upload",
        )
        .await
        .map(|_| ())
    }

    /// Ok(None) when the relay doesn't have it.
    pub async fn get_blob(&self, blob_id: &str) -> Result<Option<Vec<u8>>> {
        let resp = self
            .send(Method::GET, &format!("/v1/blobs/{blob_id}"), Vec::new())
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            bail!("relay image download failed: {}", error_message(resp).await);
        }
        Ok(Some(resp.bytes().await.context("image download")?.to_vec()))
    }

    pub async fn revoke(&self, device_id: &str) -> Result<()> {
        let resp = self
            .send(
                Method::DELETE,
                &format!("/v1/devices/{device_id}"),
                Vec::new(),
            )
            .await?;
        if resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        bail!("relay revoke failed: {}", error_message(resp).await)
    }

    pub async fn leave(&self) -> Result<()> {
        self.ok_bytes(Method::DELETE, "/v1/devices/me", Vec::new(), "leave")
            .await
            .map(|_| ())
    }

    pub async fn rotate_enroll(&self, new_key: &DeviceKey) -> Result<()> {
        let body = serde_json::to_vec(&RotateEnrollRequest {
            enroll_pub: new_key.public_b64(),
        })?;
        self.ok_bytes(Method::PUT, "/v1/enroll-key", body, "enroll key rotation")
            .await
            .map(|_| ())
    }

    pub async fn space_info(&self) -> Result<SpaceInfo> {
        let bytes = self
            .ok_bytes(Method::GET, "/v1/space", Vec::new(), "space info")
            .await?;
        serde_json::from_slice(&bytes).context("malformed space info")
    }

    /// Delete the space and everything it stores from the relay.
    pub async fn delete_space(&self) -> Result<()> {
        self.ok_bytes(Method::DELETE, "/v1/space", Vec::new(), "delete space")
            .await
            .map(|_| ())
    }

    async fn stream(
        &self,
        since: i64,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    > {
        let pq = format!("/v1/stream?since={since}");
        let mut req = relay::ws_url(&self.base, &pq).into_client_request()?;
        req.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&self.sign("GET", &pq, b""))?,
        );
        let connector = tokio_tungstenite::Connector::Rustls(Arc::new(tls_config()));
        match tokio::time::timeout(
            Duration::from_secs(15),
            tokio_tungstenite::connect_async_tls_with_config(req, None, false, Some(connector)),
        )
        .await
        {
            Ok(Ok((ws, _))) => Ok(ws),
            Ok(Err(tokio_tungstenite::tungstenite::Error::Http(resp)))
                if resp.status() == 401 || resp.status() == 403 =>
            {
                Err(RelayDenied("stream refused".into()).into())
            }
            Ok(Err(e)) => bail!("relay stream connect failed: {e}"),
            Err(_) => bail!("relay stream connect timed out"),
        }
    }
}

// ---- views (UI / CLI) ------------------------------------------------------------

/// A relay as shown in settings screens and `clipd sync relay status`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RelayView {
    pub url: String,
    /// "connected" (streaming), "standby" (LAN hub in use; relay idle),
    /// "connecting", "offline", "denied" (revoked / not accepted) or
    /// "stopped" (no sync loop running in this process).
    pub state: String,
    pub connected: bool,
    /// This device's key is registered on the relay.
    pub enrolled: bool,
    pub added_at: Millis,
    pub last_ok: Option<Millis>,
    pub last_error: Option<String>,
    /// Relay log position applied here.
    pub pull_seq: i64,
    /// Hub: hub log position forwarded to the relay.
    pub push_seq: i64,
}

fn view_from_row(r: &crate::db::RelayRow, live: Option<&LiveRelay>) -> RelayView {
    let (state, connected, err) = match live {
        Some(l) => (
            l.state.clone(),
            l.state == "connected",
            l.error.clone().or_else(|| r.last_error.clone()),
        ),
        None => ("stopped".into(), false, r.last_error.clone()),
    };
    RelayView {
        url: r.url.clone(),
        state,
        connected,
        enrolled: r.enrolled,
        added_at: r.added_at,
        last_ok: r.last_ok,
        last_error: err,
        pull_seq: r.pull_seq,
        push_seq: r.push_seq,
    }
}

/// Relays of a data dir, from disk (nothing running).
pub fn relay_views(db: &SyncDb) -> Result<Vec<RelayView>> {
    Ok(db
        .relays()?
        .iter()
        .map(|r| view_from_row(r, None))
        .collect())
}

/// Check a relay URL without changing anything (for a "Test" button).
pub async fn relay_check(url: &str) -> Result<RelayHello> {
    let base = relay::normalize_url(url)?;
    hello(&relay_http()?, &base).await
}

fn open_engine_blocking(data_dir: &Path, ks: &Arc<dyn KeyStore>) -> Result<Engine> {
    Engine::open(data_dir, &**ks)
}

/// Add an off-LAN relay to this device's space: registers the space on the
/// relay (this device's key + a fresh enroll key) and shares the relay URL
/// and enroll key with the other devices in a sealed `SpaceConfig` event.
/// `invite` is the relay operator's registration code, if it needs one.
///
/// Works from the hub or any client. A running hub/client picks the relay
/// up within a few seconds (call `relay_changed()` on its handle to make it
/// immediate).
pub async fn relay_add(
    data_dir: &Path,
    keystore: Arc<dyn KeyStore>,
    url: &str,
    invite: Option<&str>,
) -> Result<RelayView> {
    let base = relay::normalize_url(url)?;
    let (dd, ks) = (data_dir.to_path_buf(), keystore.clone());
    let b = base.clone();
    let (rsid, device_id, key, exists) = blocking(move || {
        let engine = open_engine_blocking(&dd, &ks)?;
        let key = device_key(&*ks)?;
        let exists = engine.db.relay(&b)?.is_some();
        Ok((
            relay::relay_space_id(&engine.keys, &engine.space.space_id),
            engine.space.device_id.clone(),
            key,
            exists,
        ))
    })
    .await?;
    if exists {
        bail!("{base} is already one of this space's relays");
    }
    let conn = RelayConn::new(&base, &rsid, &device_id, key)?;
    let h = hello(&conn.http, &base).await?;
    if h.registration == "closed" {
        bail!("that relay does not accept new spaces");
    }
    if h.registration == "invite" && invite.is_none_or(|i| i.trim().is_empty()) {
        bail!("that relay needs an invite code (ask its operator; pass --invite)");
    }
    let enroll = DeviceKey::generate();
    conn.register(&enroll, invite.map(str::trim)).await?;

    let dd = data_dir.to_path_buf();
    let b = base.clone();
    let view = blocking(move || {
        let engine = Engine::open(&dd, &*keystore)?;
        let sealed = engine.seal_enroll(&b, &enroll.seed())?;
        engine.db.upsert_relay(&b, &sealed, now_millis(), true)?;
        engine
            .db
            .set_relay_result(&b, now_millis(), None)
            .context("save relay")?;
        publish_config(&engine)?;
        let row = engine.db.relay(&b)?.context("relay row vanished")?;
        Ok(view_from_row(&row, None))
    })
    .await?;
    tracing::info!(relay = %base, "relay added");
    Ok(view)
}

/// Share the local relay list with the space. Clients queue the event in
/// their outbox; the hub marks it for its bridge (which appends it to the
/// log and streams it to LAN devices, in whichever process runs the hub).
fn publish_config(engine: &Engine) -> Result<()> {
    match engine.role() {
        Role::Client => {
            let ev = engine.local_space_config()?;
            client_enqueue(engine, &ev)
        }
        Role::Hub => engine.db.set("space_config_dirty", "1"),
    }
}

/// Stop using a relay and tell the other devices. With `purge`, also
/// delete the space and all its ciphertext from the relay (best effort).
/// Returns false when the relay wasn't configured.
pub async fn relay_remove(
    data_dir: &Path,
    keystore: Arc<dyn KeyStore>,
    url: &str,
    purge: bool,
) -> Result<bool> {
    let base = relay::normalize_url(url)?;
    let (dd, ks) = (data_dir.to_path_buf(), keystore.clone());
    let b = base.clone();
    let found = blocking(move || {
        let engine = open_engine_blocking(&dd, &ks)?;
        if engine.db.relay(&b)?.is_none() {
            return Ok(None);
        }
        Ok(Some((
            relay::relay_space_id(&engine.keys, &engine.space.space_id),
            engine.space.device_id.clone(),
            device_key(&*ks)?,
        )))
    })
    .await?;
    let Some((rsid, device_id, key)) = found else {
        return Ok(false);
    };
    if purge {
        let conn = RelayConn::new(&base, &rsid, &device_id, key)?;
        conn.delete_space().await?;
    }
    let dd = data_dir.to_path_buf();
    blocking(move || {
        let engine = Engine::open(&dd, &*keystore)?;
        engine.db.remove_relay(&base)?;
        publish_config(&engine)
    })
    .await?;
    Ok(true)
}

/// Client leaving the space: unregister this device from every relay
/// (best effort; call before `reset`).
pub async fn leave_relays(data_dir: &Path, keystore: Arc<dyn KeyStore>) {
    let dd = data_dir.to_path_buf();
    let found = blocking(move || {
        let engine = open_engine_blocking(&dd, &keystore)?;
        let rsid = relay::relay_space_id(&engine.keys, &engine.space.space_id);
        let urls: Vec<String> = engine
            .db
            .relays()?
            .into_iter()
            .filter(|r| r.enrolled)
            .map(|r| r.url)
            .collect();
        Ok((
            rsid,
            engine.space.device_id.clone(),
            keystore.get(RELAY_KEY)?,
            urls,
        ))
    })
    .await;
    let Ok((rsid, device_id, Some(seed), urls)) = found else {
        return;
    };
    for url in urls {
        let Some(key) = DeviceKey::from_seed(&seed) else {
            return;
        };
        let Ok(conn) = RelayConn::new(&url, &rsid, &device_id, key) else {
            continue;
        };
        match tokio::time::timeout(Duration::from_secs(8), conn.leave()).await {
            Ok(Ok(())) => tracing::info!(relay = %url, "left relay"),
            Ok(Err(e)) => tracing::debug!(relay = %url, "relay leave failed: {e:#}"),
            Err(_) => tracing::debug!(relay = %url, "relay leave timed out"),
        }
    }
}

// ---- running links -------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct LiveRelay {
    state: String,
    error: Option<String>,
}

struct LinkShared {
    engine: Arc<Engine>,
    keystore: Arc<dyn KeyStore>,
    notices: broadcast::Sender<SyncNotice>,
    live: Mutex<HashMap<String, LiveRelay>>,
    wake: Notify,
    config_wake: Notify,
    stop: watch::Receiver<bool>,
    fetch_images: bool,
    /// Serializes "check seen → apply → mark seen" across hub and relays.
    apply_lock: Arc<Mutex<()>>,
    mode: Mode,
}

enum Mode {
    /// Client: true while the LAN hub session is connected.
    Client { hub_up: watch::Receiver<bool> },
    /// Hub: publish an envelope to LAN streams.
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    Hub {
        publish: Arc<dyn Fn(SeqEnvelope) + Send + Sync>,
    },
}

/// Handle to the relay loops of one hub or client process.
pub struct LinkHandle {
    shared: Arc<LinkShared>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

/// Wakes a link's push loops (cheap to clone).
#[derive(Clone)]
pub struct LinkWaker(Arc<LinkShared>);

impl LinkWaker {
    pub fn wake(&self) {
        self.0.wake.notify_waiters();
    }
}

impl LinkHandle {
    pub fn waker(&self) -> LinkWaker {
        LinkWaker(self.shared.clone())
    }

    /// The relay list or local log changed: act now.
    pub fn wake(&self) {
        self.shared.wake.notify_waiters();
        self.shared.config_wake.notify_one();
    }

    pub fn views(&self) -> Vec<RelayView> {
        let live = lock(&self.shared.live).clone();
        self.shared
            .engine
            .db
            .relays()
            .unwrap_or_default()
            .iter()
            .map(|r| view_from_row(r, live.get(&r.url)))
            .collect()
    }

    /// Fetch one full image from whichever relay has it.
    pub async fn fetch_image(&self, blob_id: &str, hash: &[u8]) -> Result<bool> {
        let rows = {
            let e = self.shared.engine.clone();
            blocking(move || e.db.relays()).await?
        };
        let mut last = anyhow!("no relay configured");
        for r in rows.into_iter().filter(|r| r.enrolled) {
            let conn = conn_for(&self.shared, &r.url).await?;
            match fetch_blob(&self.shared, &conn, blob_id, hash).await {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}

/// Start the relay loops of a client (phone, second desktop).
pub(crate) fn start_client(
    engine: Arc<Engine>,
    keystore: Arc<dyn KeyStore>,
    notices: broadcast::Sender<SyncNotice>,
    hub_up: watch::Receiver<bool>,
    fetch_images: bool,
    apply_lock: Arc<Mutex<()>>,
) -> LinkHandle {
    start(LinkShared {
        engine,
        keystore,
        notices,
        live: Mutex::new(HashMap::new()),
        wake: Notify::new(),
        config_wake: Notify::new(),
        stop: watch::channel(false).1,
        fetch_images,
        apply_lock,
        mode: Mode::Client { hub_up },
    })
}

/// Start the hub's relay bridge.
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) fn start_hub(
    engine: Arc<Engine>,
    keystore: Arc<dyn KeyStore>,
    notices: broadcast::Sender<SyncNotice>,
    publish: Arc<dyn Fn(SeqEnvelope) + Send + Sync>,
) -> LinkHandle {
    start(LinkShared {
        engine,
        keystore,
        notices,
        live: Mutex::new(HashMap::new()),
        wake: Notify::new(),
        config_wake: Notify::new(),
        stop: watch::channel(false).1,
        fetch_images: true,
        apply_lock: Arc::new(Mutex::new(())),
        mode: Mode::Hub { publish },
    })
}

fn start(mut shared: LinkShared) -> LinkHandle {
    let (stop_tx, stop_rx) = watch::channel(false);
    shared.stop = stop_rx;
    let shared = Arc::new(shared);
    let task = tokio::spawn(supervise(shared.clone()));
    LinkHandle {
        shared,
        stop: stop_tx,
        task,
    }
}

fn is_hub(sh: &LinkShared) -> bool {
    matches!(sh.mode, Mode::Hub { .. })
}

fn set_live(sh: &LinkShared, url: &str, state: &str, error: Option<String>) {
    let changed = {
        let mut live = lock(&sh.live);
        let e = live.entry(url.to_owned()).or_default();
        let changed = e.state != state || e.error != error;
        e.state = state.to_owned();
        e.error = error;
        changed
    };
    if changed {
        let _ = sh.notices.send(SyncNotice::StatusChanged);
    }
}

/// Keeps one session task per configured relay; hub housekeeping.
async fn supervise(sh: Arc<LinkShared>) {
    let mut stop = sh.stop.clone();
    let mut sessions: HashMap<String, (watch::Sender<bool>, JoinHandle<()>)> = HashMap::new();
    let mut last_prune = Instant::now()
        .checked_sub(Duration::from_secs(3600))
        .unwrap_or_else(Instant::now);
    loop {
        if *stop.borrow() {
            break;
        }
        let e = sh.engine.clone();
        let rows = blocking(move || e.db.relays()).await.unwrap_or_default();
        let urls: Vec<String> = rows.iter().map(|r| r.url.clone()).collect();
        // Stop sessions for removed relays.
        sessions.retain(|url, (tx, task)| {
            if urls.contains(url) && !task.is_finished() {
                true
            } else {
                let _ = tx.send(true);
                lock(&sh.live).remove(url);
                false
            }
        });
        for url in urls {
            sessions.entry(url.clone()).or_insert_with(|| {
                let (tx, rx) = watch::channel(false);
                let task = tokio::spawn(relay_loop(sh.clone(), url, rx));
                (tx, task)
            });
        }
        if is_hub(&sh)
            && let Err(e) = hub_housekeeping(&sh).await
        {
            tracing::debug!("relay housekeeping: {e:#}");
        }
        if last_prune.elapsed() > Duration::from_secs(3600) {
            last_prune = Instant::now();
            let e = sh.engine.clone();
            let _ = blocking(move || e.db.prune_seen(now_millis() - SEEN_TTL_MS)).await;
        }
        tokio::select! {
            _ = tokio::time::sleep(SUPERVISE_EVERY) => {}
            _ = sh.config_wake.notified() => {}
            _ = crate::engine::stopped(&mut stop) => break,
        }
    }
    for (_, (tx, task)) in sessions {
        let _ = tx.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
    }
}

/// Hub: emit a pending SpaceConfig; revoke devices on relays and rotate the
/// enroll key so a revoked device can't enroll again.
async fn hub_housekeeping(sh: &Arc<LinkShared>) -> Result<()> {
    let e = sh.engine.clone();
    let (dirty, revoked) = blocking(move || {
        let rows = e.db.relays()?;
        let mut revoked = Vec::new();
        for d in e.db.devices()? {
            if d.revoked_at.is_some() && !d.is_self {
                for r in &rows {
                    if r.enrolled && !e.db.relay_revoked(&r.url, &d.id)? {
                        revoked.push((r.url.clone(), d.id.clone()));
                    }
                }
            }
        }
        Ok((
            e.db.get("space_config_dirty")?.as_deref() == Some("1"),
            revoked,
        ))
    })
    .await?;
    let mut rotated = false;
    for (url, dev) in revoked {
        let Ok(conn) = conn_for(sh, &url).await else {
            tracing::debug!(relay = %url, "relay unreachable during housekeeping; skipping");
            continue;
        };
        if let Err(err) = conn.revoke(&dev).await {
            tracing::debug!(relay = %url, "revoke on relay failed: {err:#}");
            continue;
        }
        let new_key = DeviceKey::generate();
        if let Err(err) = conn.rotate_enroll(&new_key).await {
            tracing::debug!(relay = %url, "key rotation failed: {err:#}");
            continue;
        }
        let (e, u, d) = (sh.engine.clone(), url.clone(), dev.clone());
        blocking(move || {
            e.db.upsert_relay(&u, &e.seal_enroll(&u, &new_key.seed())?, now_millis(), true)?;
            e.db.mark_relay_revoked(&u, &d, now_millis())?;
            Ok(())
        })
        .await?;
        tracing::info!(relay = %url, device = %dev, "device revoked on relay; enroll key rotated");
        rotated = true;
    }
    if dirty || rotated {
        let e = sh.engine.clone();
        let published = blocking(move || {
            let ev = e.local_space_config()?;
            let out = hub_append(&e, &ev)?;
            e.db.set("space_config_dirty", "0")?;
            Ok(out)
        })
        .await?;
        if let (Some(env), Mode::Hub { publish }) = (published, &sh.mode) {
            publish(env);
        }
        sh.wake.notify_waiters();
    }
    Ok(())
}

async fn conn_for(sh: &LinkShared, url: &str) -> Result<RelayConn> {
    let (e, ks) = (sh.engine.clone(), sh.keystore.clone());
    let (rsid, device_id, key) = blocking(move || {
        Ok((
            relay::relay_space_id(&e.keys, &e.space.space_id),
            e.space.device_id.clone(),
            device_key(&*ks)?,
        ))
    })
    .await?;
    RelayConn::new(url, &rsid, &device_id, key)
}

fn hub_is_up(sh: &LinkShared) -> bool {
    match &sh.mode {
        Mode::Client { hub_up } => *hub_up.borrow(),
        Mode::Hub { .. } => false,
    }
}

/// One relay, forever: standby while the LAN hub is up (clients), else a
/// session with backoff.
async fn relay_loop(sh: Arc<LinkShared>, url: String, mut stop: watch::Receiver<bool>) {
    let mut global_stop = sh.stop.clone();
    let mut backoff = Duration::from_secs(1);
    if !is_hub(&sh) {
        tokio::select! {
            _ = tokio::time::sleep(LAN_GRACE) => {}
            _ = crate::engine::stopped(&mut stop) => return,
            _ = crate::engine::stopped(&mut global_stop) => return,
        }
    }
    loop {
        if *stop.borrow() || *global_stop.borrow() {
            return;
        }
        if hub_is_up(&sh) {
            set_live(&sh, &url, "standby", None);
            let Mode::Client { hub_up } = &sh.mode else {
                return;
            };
            let mut hub_up = hub_up.clone();
            tokio::select! {
                _ = hub_up.wait_for(|up| !*up) => {}
                _ = crate::engine::stopped(&mut stop) => return,
                _ = crate::engine::stopped(&mut global_stop) => return,
            }
            continue;
        }
        set_live(&sh, &url, "connecting", None);
        let started = Instant::now();
        let res = session(&sh, &url, &mut stop).await;
        let denied = res.as_ref().err().is_some_and(|e| e.is::<RelayDenied>());
        match &res {
            Ok(()) => set_live(&sh, &url, "standby", None),
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::warn!(relay = %url, "relay sync: {msg}");
                let (en, u, m) = (sh.engine.clone(), url.clone(), msg.clone());
                let _ = blocking(move || en.db.set_relay_result(&u, now_millis(), Some(&m))).await;
                set_live(
                    &sh,
                    &url,
                    if denied { "denied" } else { "offline" },
                    Some(msg),
                );
            }
        }
        if started.elapsed() > Duration::from_secs(30) {
            backoff = Duration::from_secs(1);
        }
        if denied {
            backoff = MAX_BACKOFF;
        }
        if res.is_ok() {
            continue;
        }
        let mut hub_up = match &sh.mode {
            Mode::Client { hub_up } => Some(hub_up.clone()),
            Mode::Hub { .. } => None,
        };
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = async {
                match hub_up.as_mut() {
                    Some(h) => { let _ = h.wait_for(|up| *up).await; }
                    None => std::future::pending::<()>().await,
                }
            } => {}
            _ = crate::engine::stopped(&mut stop) => return,
            _ = crate::engine::stopped(&mut global_stop) => return,
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// hello → enroll if needed → push → catch up → stream. Returns Ok(())
/// when a client should go back to standby (hub reachable again).
async fn session(sh: &Arc<LinkShared>, url: &str, stop: &mut watch::Receiver<bool>) -> Result<()> {
    let conn = conn_for(sh, url).await?;
    let h = hello(&conn.http, url).await?;
    ensure_enrolled(sh, &conn).await?;
    push_all(sh, &conn, h.limits.max_blob_bytes).await?;
    catch_up(sh, &conn).await?;
    let (e, u) = (sh.engine.clone(), url.to_owned());
    let since = blocking(move || Ok(e.db.relay(&u)?.map_or(0, |r| r.pull_seq))).await?;
    let ws = conn.stream(since).await?;
    set_live(sh, url, "connected", None);
    {
        let (e, u) = (sh.engine.clone(), url.to_owned());
        let _ = blocking(move || e.db.set_relay_result(&u, now_millis(), None)).await;
    }
    tracing::info!(relay = %url, since, "connected to relay");
    if sh.fetch_images {
        let _ = fetch_due_blobs(sh, &conn).await;
    }
    let (mut sink, mut stream) = ws.split();
    let mut global_stop = sh.stop.clone();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut slow = tokio::time::interval(Duration::from_secs(10));
    let mut last_rx = Instant::now();
    loop {
        tokio::select! {
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    last_rx = Instant::now();
                    let frame: StreamFrame = serde_json::from_str(&t)
                        .context("malformed relay stream frame")?;
                    if frame.t == "event" {
                        incoming(sh, &conn, vec![frame.ev]).await?;
                    }
                }
                Some(Ok(Message::Close(Some(f)))) if u16::from(f.code) == relay::REVOKED_CLOSE_CODE => {
                    return Err(RelayDenied("this device was revoked on the relay".into()).into());
                }
                Some(Ok(Message::Close(_))) | None => bail!("relay closed the connection"),
                Some(Ok(_)) => last_rx = Instant::now(),
                Some(Err(e)) => bail!("relay stream error: {e}"),
            },
            _ = sh.wake.notified() => {
                push_all(sh, &conn, h.limits.max_blob_bytes).await?;
            }
            _ = tick.tick() => {
                if hub_is_up(sh) {
                    let _ = sink.send(Message::Close(None)).await;
                    return Ok(());
                }
                push_all(sh, &conn, h.limits.max_blob_bytes).await?;
                if last_rx.elapsed() > STREAM_IDLE {
                    bail!("relay stopped responding");
                }
            }
            _ = slow.tick() => {
                if sh.fetch_images {
                    fetch_due_blobs(sh, &conn).await?;
                }
                if is_hub(sh) {
                    upload_deferred_blobs(sh, &conn, h.limits.max_blob_bytes).await?;
                }
            }
            _ = crate::engine::stopped(stop) => {
                let _ = sink.send(Message::Close(None)).await;
                return Ok(());
            }
            _ = crate::engine::stopped(&mut global_stop) => {
                let _ = sink.send(Message::Close(None)).await;
                return Ok(());
            }
        }
    }
}

async fn ensure_enrolled(sh: &Arc<LinkShared>, conn: &RelayConn) -> Result<()> {
    let (e, u) = (sh.engine.clone(), conn.base.clone());
    let Some((enrolled, seed)) = blocking(move || {
        let Some(r) = e.db.relay(&u)? else {
            return Ok(None);
        };
        Ok(Some((r.enrolled, e.open_enroll(&u, &r.enroll_sealed)?)))
    })
    .await?
    else {
        bail!("relay was removed");
    };
    if enrolled {
        return Ok(());
    }
    let enroll = DeviceKey::from_seed(&seed).context("relay enroll key is corrupt")?;
    conn.enroll(&enroll).await?;
    let (e, u) = (sh.engine.clone(), conn.base.clone());
    blocking(move || e.db.set_relay_enrolled(&u, true)).await?;
    tracing::info!(relay = %conn.base, "enrolled on relay");
    Ok(())
}

/// What to push, with the metadata the relay needs and the blob to upload
/// first.
struct OutItem {
    item: RelayPushItem,
    blob: Option<(String, Vec<u8>)>,
    /// Hub log seq (hub) — the push cursor moves to the last one.
    seq: i64,
}

/// Relay metadata of an envelope this device can open.
#[derive(Default)]
struct Meta {
    /// Keyed per-clip tag (compaction), never the content hash.
    tag: Option<String>,
    kind: Option<String>,
    /// (blob_id, content hash) of a referenced full image.
    blob: Option<(String, Vec<u8>)>,
}

fn describe(engine: &Engine, env: &Envelope) -> Meta {
    let Ok(ev) = env.open(&engine.keys, &engine.space.space_id) else {
        return Meta::default();
    };
    let blob = match &ev {
        Event::ClipAdded(c) => c
            .image
            .as_ref()
            .and_then(|i| ev.hash().ok().map(|h| (i.blob_id.clone(), h))),
        _ => None,
    };
    Meta {
        tag: ev.hash().ok().map(|h| engine.keys.hash_tag(&h)),
        kind: Some(ev.kind_name().to_owned()),
        blob,
    }
}

fn outgoing(sh: &LinkShared, url: &str) -> Result<Vec<OutItem>> {
    let e = &sh.engine;
    let mut out = Vec::new();
    if is_hub(sh) {
        let push_seq = e.db.relay(url)?.map_or(0, |r| r.push_seq);
        for s in e.db.envelopes_since(push_seq, PUSH_BATCH)? {
            // Don't send a relay its own events back.
            if e.db.seen_src(&s.env.event_id)?.as_deref() == Some(url) {
                out.push(OutItem {
                    item: RelayPushItem {
                        env: s.env,
                        tag: None,
                        kind: None,
                        blob: None,
                    },
                    blob: None,
                    seq: -s.seq,
                });
                continue;
            }
            let Meta { tag, kind, blob } = describe(e, &s.env);
            out.push(OutItem {
                item: RelayPushItem {
                    env: s.env,
                    tag,
                    kind,
                    blob: blob.as_ref().map(|b| b.0.clone()),
                },
                blob,
                seq: s.seq,
            });
        }
    } else {
        for row in e.db.outbox(PUSH_BATCH)? {
            let Meta { tag, kind, .. } = describe(e, &row.env);
            out.push(OutItem {
                item: RelayPushItem {
                    env: row.env,
                    tag,
                    kind,
                    blob: row.blob.as_ref().map(|b| b.0.clone()),
                },
                blob: row.blob,
                seq: 0,
            });
        }
    }
    Ok(out)
}

/// Push everything pending (outbox for clients, hub log for the hub).
async fn push_all(sh: &Arc<LinkShared>, conn: &RelayConn, max_blob: u64) -> Result<()> {
    loop {
        let (s2, u) = (sh.clone(), conn.base.clone());
        let items = blocking(move || outgoing(&s2, &u)).await?;
        if items.is_empty() {
            return Ok(());
        }
        let n = items.len();
        let last_seq = items.iter().map(|i| i.seq.abs()).max().unwrap_or(0);
        let mut to_send = Vec::new();
        for it in items {
            if it.seq < 0 {
                continue; // came from this relay
            }
            if let Some((blob_id, hash)) = &it.blob {
                upload_blob(sh, conn, blob_id, hash, max_blob).await?;
            }
            to_send.push(it.item);
        }
        let ids: Vec<String> = to_send.iter().map(|i| i.env.event_id.clone()).collect();
        let resp = if to_send.is_empty() {
            None
        } else {
            Some(conn.push(to_send).await?)
        };
        for r in resp.iter().flat_map(|r| &r.rejected) {
            tracing::warn!(event = %r.event_id, reason = %r.reason, "relay rejected an event");
        }
        let (s2, u) = (sh.clone(), conn.base.clone());
        blocking(move || {
            let e = &s2.engine;
            let src = format!("out:{u}");
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            e.db.mark_seen(&refs, &src, now_millis())?;
            if is_hub(&s2) {
                e.db.set_relay_push_seq(&u, last_seq)
            } else {
                let mut done: Vec<String> = Vec::new();
                if let Some(r) = resp {
                    done.extend(r.accepted);
                    done.extend(r.rejected.into_iter().map(|r| r.event_id));
                }
                e.db.remove_outbox(&done)
            }
        })
        .await?;
        tracing::debug!(relay = %conn.base, events = n, "pushed to relay");
        if n < PUSH_BATCH as usize {
            return Ok(());
        }
    }
}

/// Upload the sealed full image if this device has it; otherwise (hub)
/// remember to try again later.
async fn upload_blob(
    sh: &Arc<LinkShared>,
    conn: &RelayConn,
    blob_id: &str,
    hash: &[u8],
    max_blob: u64,
) -> Result<()> {
    let (e, b, h) = (sh.engine.clone(), blob_id.to_owned(), hash.to_vec());
    let sealed = blocking(move || {
        if let Some(s) = e.seal_blob(&b, &h)? {
            return Ok(Some(s));
        }
        // Hub: a sealed upload that hasn't been attached yet.
        Ok(std::fs::read(e.blob_dir().join(&b)).ok())
    })
    .await?;
    match sealed {
        Some(s) if max_blob == 0 || (s.len() as u64) <= max_blob => {
            conn.put_blob(blob_id, s).await?;
            if is_hub(sh) {
                let (e, u, b) = (sh.engine.clone(), conn.base.clone(), blob_id.to_owned());
                blocking(move || e.db.remove_relay_blob(&u, &b)).await?;
            }
        }
        Some(_) => {
            tracing::debug!(blob = %blob_id, "image too large for the relay; thumbnail only")
        }
        None if is_hub(sh) => {
            let (e, u, b, h) = (
                sh.engine.clone(),
                conn.base.clone(),
                blob_id.to_owned(),
                hash.to_vec(),
            );
            blocking(move || e.db.add_relay_blob(&u, &b, &h)).await?;
        }
        None => {}
    }
    Ok(())
}

async fn upload_deferred_blobs(
    sh: &Arc<LinkShared>,
    conn: &RelayConn,
    max_blob: u64,
) -> Result<()> {
    let (e, u) = (sh.engine.clone(), conn.base.clone());
    let todo = blocking(move || e.db.relay_blobs(&u, 8)).await?;
    for (blob_id, hash) in todo {
        let (e, h) = (sh.engine.clone(), hash.clone());
        let have = blocking(move || {
            Ok(e.pipeline
                .store()
                .get_by_hash(&h)?
                .map(|c| c.clip.image_path.is_some()))
        })
        .await?;
        match have {
            Some(true) => upload_blob(sh, conn, &blob_id, &hash, max_blob).await?,
            Some(false) => {}
            None => {
                let (e, u) = (sh.engine.clone(), conn.base.clone());
                blocking(move || e.db.remove_relay_blob(&u, &blob_id)).await?;
            }
        }
    }
    Ok(())
}

async fn catch_up(sh: &Arc<LinkShared>, conn: &RelayConn) -> Result<()> {
    loop {
        let (e, u) = (sh.engine.clone(), conn.base.clone());
        let since = blocking(move || Ok(e.db.relay(&u)?.map_or(0, |r| r.pull_seq))).await?;
        let page = conn.events(since, PAGE).await?;
        if page.events.is_empty() {
            return Ok(());
        }
        let last = page.events.last().map_or(since, |e| e.seq);
        incoming(sh, conn, page.events).await?;
        if last >= page.head_seq {
            return Ok(());
        }
    }
}

/// Apply relay envelopes in seq order (dedup by event id) and advance the
/// relay cursor. The hub also appends them to its log for LAN devices.
async fn incoming(sh: &Arc<LinkShared>, conn: &RelayConn, envs: Vec<SeqEnvelope>) -> Result<()> {
    let (s2, u) = (sh.clone(), conn.base.clone());
    let (metas, changed, blobs, published) = blocking(move || {
        let e = &s2.engine;
        let hub = is_hub(&s2);
        let _guard = lock(&s2.apply_lock);
        let mut last = e.db.relay(&u)?.map_or(0, |r| r.pull_seq);
        let (mut metas, mut changed, mut blobs, mut published) =
            (Vec::new(), 0usize, Vec::new(), Vec::new());
        for s in envs {
            if s.seq <= last {
                continue;
            }
            last = s.seq;
            let id = s.env.event_id.clone();
            if s.env.device_id == e.space.device_id || e.db.seen_src(&id)?.is_some() {
                e.db.set_relay_pull_seq(&u, last)?;
                continue;
            }
            let event = match s.env.open(&e.keys, &e.space.space_id) {
                Ok(ev) => ev,
                Err(err) => {
                    tracing::warn!(seq = s.seq, event = %id, "skipping relay event: {err}");
                    e.db.mark_seen(&[&id], &u, now_millis())?;
                    e.db.set_relay_pull_seq(&u, last)?;
                    continue;
                }
            };
            let mut log_seq = s.seq;
            if hub {
                let hash = event.hash()?;
                let (seq, new) = e.db.append_envelope(
                    &s.env,
                    &e.keys.hash_tag(&hash),
                    event.kind_name(),
                    now_millis(),
                )?;
                log_seq = seq;
                if new {
                    published.push(SeqEnvelope {
                        seq,
                        env: s.env.clone(),
                    });
                }
            }
            match e.apply_event(&event, &s.env.device_id) {
                Ok(applied) => {
                    if applied.changed {
                        changed += 1;
                    }
                    metas.push(SyncNotice::Event {
                        seq: log_seq,
                        kind: event.kind_name().to_owned(),
                        device_id: s.env.device_id.clone(),
                        event_id: id.clone(),
                        applied: applied.changed,
                    });
                    blobs.extend(applied.blob);
                }
                Err(err) => {
                    tracing::warn!(event = %id, "couldn't apply relay event: {err:#}");
                    break;
                }
            }
            e.db.mark_seen(&[&id], &u, now_millis())?;
            e.db.set_relay_pull_seq(&u, last)?;
        }
        Ok((metas, changed, blobs, published))
    })
    .await?;
    if let Mode::Hub { publish } = &sh.mode {
        for env in published {
            publish(env);
        }
    }
    for m in metas {
        let _ = sh.notices.send(m);
    }
    if changed > 0 {
        let _ = sh.notices.send(SyncNotice::ClipsChanged { count: changed });
    }
    if sh.fetch_images {
        for (blob_id, hash) in blobs {
            if let Err(e) = fetch_blob(sh, conn, &blob_id, &hash).await {
                if e.is::<RelayDenied>() {
                    return Err(e);
                }
                tracing::debug!(blob = %blob_id, "relay image fetch deferred: {e:#}");
            }
        }
    }
    Ok(())
}

async fn fetch_due_blobs(sh: &Arc<LinkShared>, conn: &RelayConn) -> Result<()> {
    let e = sh.engine.clone();
    let due = blocking(move || e.db.due_pending_blobs(now_millis(), 4)).await?;
    for (blob_id, hash, attempts) in due {
        match fetch_blob(sh, conn, &blob_id, &hash).await {
            Ok(true) => {}
            Err(e) if e.is::<RelayDenied>() => return Err(e),
            res => {
                if let Err(e) = &res {
                    tracing::debug!(blob = %blob_id, attempts, "relay image fetch failed: {e:#}");
                }
                let e = sh.engine.clone();
                blocking(move || {
                    if attempts + 1 >= BLOB_GIVE_UP {
                        e.db.remove_pending_blob(&blob_id)
                    } else {
                        let wait: Millis = (5_000i64 << attempts.min(10)).min(3_600_000);
                        e.db.retry_pending_blob(&blob_id, now_millis() + wait)
                    }
                })
                .await?;
            }
        }
    }
    Ok(())
}

/// GET one sealed image from the relay and attach it. Ok(false) when the
/// relay doesn't have it (yet) or the clip is gone.
async fn fetch_blob(sh: &LinkShared, conn: &RelayConn, blob_id: &str, hash: &[u8]) -> Result<bool> {
    let Some(body) = conn.get_blob(blob_id).await? else {
        return Ok(false);
    };
    let (e, b, h) = (sh.engine.clone(), blob_id.to_owned(), hash.to_vec());
    let attached = blocking(move || {
        let ok = e.attach_blob(&b, &h, &body)?;
        e.db.remove_pending_blob(&b)?;
        Ok(ok)
    })
    .await?;
    if attached {
        tracing::debug!(blob = %blob_id, "image downloaded from relay");
        let _ = sh.notices.send(SyncNotice::ClipsChanged { count: 1 });
    }
    Ok(attached)
}
