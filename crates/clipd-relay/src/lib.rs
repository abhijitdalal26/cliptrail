//! `clipd-relay`: a self-hostable mailbox for ClipTrail off-LAN sync.
//!
//! It stores sealed envelopes and sealed image blobs per space and fans
//! them out to the space's devices. It holds no key that opens anything:
//! devices authenticate with per-device Ed25519 keys (it stores only the
//! public halves) and every payload is XChaCha20-Poly1305 ciphertext made
//! on the devices. See `docs/adr/0001-off-lan-sync.md`.
//!
//! | Method | Path | Auth | |
//! |---|---|---|---|
//! | GET | `/v1/hello` | – | [`RelayHello`] (limits, registration mode) |
//! | GET | `/healthz` | – | `ok` |
//! | POST | `/v1/spaces` | CTR1 (key in body) | register a space + first device |
//! | POST | `/v1/enroll` | CTR1 (key in body) + enroll proof | add a device |
//! | PUT | `/v1/enroll-key` | CTR1 | rotate the space enroll key |
//! | GET | `/v1/events?since=N&limit=M` | CTR1 | `{events, head_seq}` |
//! | POST | `/v1/events` | CTR1 | [`RelayPushRequest`] → `{accepted, rejected, head_seq}` |
//! | GET (WS) | `/v1/stream?since=N` | CTR1 | `{"t":"event", seq, ...Envelope}` frames |
//! | PUT/GET | `/v1/blobs/{blob_id}` | CTR1 | sealed image bytes |
//! | DELETE | `/v1/devices/me`, `/v1/devices/{id}` | CTR1 | revoke |
//! | GET/DELETE | `/v1/space` | CTR1 | usage + devices / delete everything |
//!
//! Never logs bodies, signatures or keys.

pub mod store;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path as AxPath, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use clipd_sync::auth::MAX_SKEW_MS;
use clipd_sync::envelope::SeqEnvelope;
use clipd_sync::relay::{
    self, EVENT_KINDS, EnrollRequest, RELAY_PROTO, REVOKED_CLOSE_CODE, RegisterRequest, RelayAuth,
    RelayHello, RelayLimits, RelayPushRequest, RotateEnrollRequest, SpaceInfo,
};
use clipd_sync::wire::{EventsPage, PushResponse, Rejected, StreamFrame};
use serde::Deserialize;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::store::{Enrolled, Millis, Store};

pub const MAX_PUSH: usize = 500;
pub const MAX_PAGE: u32 = 500;
const EVENTS_BODY_LIMIT: usize = 64 * 1024 * 1024;
const PING_EVERY: Duration = Duration::from_secs(20);
const REVOKE_CHECK_EVERY: Duration = Duration::from_secs(5);

pub use clipd_core::now_millis;

/// Resolves once `rx` reads true. Returns `()` so it can sit in a
/// `select!` of a `Send` future.
async fn stopped(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|s| *s).await;
}

/// Who may create spaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registration {
    Open,
    /// Needs this invite code (shared out of band by the operator).
    Invite(String),
    Closed,
}

#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    /// Shown in logs; informational.
    pub public_url: Option<String>,
    pub max_space_bytes: u64,
    pub max_blob_bytes: u64,
    pub max_spaces: u64,
    pub max_devices: u32,
    /// Envelopes older than this are dropped (0 = keep forever).
    pub retention_days: u32,
    pub registration: Registration,
    /// Take the client IP from `X-Forwarded-For` (behind Caddy/Cloudflare).
    pub trust_proxy: bool,
    /// Authenticated requests per device per minute.
    pub device_rate_per_min: u32,
    /// Unauthenticated / failed requests per IP per minute.
    pub ip_rate_per_min: u32,
    pub prune_every: Duration,
}

impl RelayConfig {
    pub fn new(bind: SocketAddr, data_dir: PathBuf) -> RelayConfig {
        RelayConfig {
            bind,
            data_dir,
            public_url: None,
            max_space_bytes: 256 * 1024 * 1024,
            max_blob_bytes: clipd_core::synced::MAX_SYNCED_IMAGE_BYTES as u64 + 1024,
            max_spaces: 100,
            max_devices: 16,
            retention_days: 30,
            registration: Registration::Open,
            trust_proxy: false,
            device_rate_per_min: 1200,
            ip_rate_per_min: 60,
            prune_every: Duration::from_secs(3600),
        }
    }

    fn limits(&self) -> RelayLimits {
        RelayLimits {
            max_event_bytes: clipd_sync::envelope::MAX_EVENT_BYTES as u64,
            max_blob_bytes: self.max_blob_bytes,
            max_space_bytes: self.max_space_bytes,
            max_devices: self.max_devices,
            retention_days: self.retention_days,
        }
    }
}

#[derive(Clone, Debug)]
enum RelayMsg {
    Event(SeqEnvelope),
    Revoked(String),
    SpaceDeleted,
}

struct Relay {
    store: Arc<Store>,
    cfg: RelayConfig,
    streams: Mutex<HashMap<String, broadcast::Sender<RelayMsg>>>,
    replay: Mutex<HashMap<Vec<u8>, Millis>>,
    limits: Mutex<HashMap<String, (Millis, u32)>>,
    stop: watch::Receiver<bool>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct RelayHandle {
    relay: Arc<Relay>,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    addr: SocketAddr,
}

impl RelayHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>` style base URL (tests, local runs).
    pub fn local_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Run retention now (tests / admin).
    pub fn prune_now(&self, now: Millis) -> Result<(usize, usize)> {
        prune(&self.relay, now)
    }

    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        for t in self.tasks {
            let _ = tokio::time::timeout(Duration::from_secs(5), t).await;
        }
    }
}

/// Open the store and start serving. Call inside a tokio runtime.
pub async fn start(cfg: RelayConfig) -> Result<RelayHandle> {
    let store = {
        let dir = cfg.data_dir.clone();
        tokio::task::spawn_blocking(move || Store::open(&dir))
            .await
            .context("open store")??
    };
    let listener = tokio::net::TcpListener::bind(cfg.bind)
        .await
        .with_context(|| format!("couldn't listen on {}", cfg.bind))?;
    let addr = listener.local_addr()?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let relay = Arc::new(Relay {
        store: Arc::new(store),
        cfg,
        streams: Mutex::new(HashMap::new()),
        replay: Mutex::new(HashMap::new()),
        limits: Mutex::new(HashMap::new()),
        stop: stop_rx.clone(),
    });
    let app = router(relay.clone());
    let mut stop_srv = stop_rx.clone();
    let server = tokio::spawn(async move {
        let serve = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = stop_srv.wait_for(|s| *s).await;
        });
        if let Err(e) = serve.await {
            tracing::warn!("relay server stopped: {e}");
        }
    });
    let pr = relay.clone();
    let mut stop_prune = stop_rx;
    let pruner = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(pr.cfg.prune_every) => {
                    let r = pr.clone();
                    match tokio::task::spawn_blocking(move || prune(&r, now_millis())).await {
                        Ok(Ok((e, b))) if e + b > 0 => tracing::info!(envelopes = e, blobs = b, "retention pruned"),
                        Ok(Err(e)) => tracing::warn!("retention failed: {e:#}"),
                        _ => {}
                    }
                }
                _ = stopped(&mut stop_prune) => break,
            }
        }
    });
    tracing::info!(
        %addr,
        public_url = relay.cfg.public_url.as_deref().unwrap_or("-"),
        "clipd-relay listening"
    );
    Ok(RelayHandle {
        relay,
        stop: stop_tx,
        tasks: vec![server, pruner],
        addr,
    })
}

fn prune(relay: &Relay, now: Millis) -> Result<(usize, usize)> {
    {
        let mut seen = lock(&relay.replay);
        seen.retain(|_, ts| now - *ts <= 2 * MAX_SKEW_MS);
        lock(&relay.limits).retain(|_, (start, _)| now - *start < 120_000);
        drop(seen);
    }
    let cutoff = if relay.cfg.retention_days == 0 {
        i64::MIN
    } else {
        now - i64::from(relay.cfg.retention_days) * 24 * 3600 * 1000
    };
    relay.store.prune(cutoff, now)
}

fn router(relay: Arc<Relay>) -> Router {
    let blob_limit = relay.cfg.max_blob_bytes as usize + 1024;
    Router::new()
        .route("/v1/hello", get(hello))
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/v1/spaces",
            post(register).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/v1/enroll",
            post(enroll).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/v1/enroll-key",
            put(rotate_enroll).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/v1/events",
            get(get_events)
                .post(post_events)
                .layer(DefaultBodyLimit::max(EVENTS_BODY_LIMIT)),
        )
        .route("/v1/stream", get(stream))
        .route(
            "/v1/blobs/{blob_id}",
            put(put_blob)
                .get(get_blob)
                .layer(DefaultBodyLimit::max(blob_limit)),
        )
        .route("/v1/devices/me", delete(leave))
        .route("/v1/devices/{device_id}", delete(revoke_device))
        .route("/v1/space", get(space_info).delete(delete_space))
        .with_state(relay)
}

// ---- errors, auth, limits ----------------------------------------------------------

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, axum::Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::warn!("relay request failed: {e:#}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
    }
}

fn err(code: StatusCode, msg: &str) -> ApiError {
    ApiError(code, msg.into())
}

fn client_ip(relay: &Relay, peer: SocketAddr, headers: &HeaderMap) -> String {
    if relay.cfg.trust_proxy
        && let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 64)
    {
        return ip.to_owned();
    }
    peer.ip().to_string()
}

/// Fixed one-minute windows. Err when over the limit.
fn rate(relay: &Relay, key: String, per_min: u32) -> Result<(), ApiError> {
    let now = now_millis();
    let mut l = lock(&relay.limits);
    if l.len() > 50_000 {
        l.retain(|_, (start, _)| now - *start < 60_000);
    }
    let e = l.entry(key).or_insert((now, 0));
    if now - e.0 >= 60_000 {
        *e = (now, 0);
    }
    e.1 += 1;
    if e.1 > per_min {
        return Err(err(
            StatusCode::TOO_MANY_REQUESTS,
            "too many requests; slow down",
        ));
    }
    Ok(())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| anyhow::anyhow!("worker panicked"))?
        .map_err(ApiError::from)
}

fn path_and_query(uri: &Uri) -> &str {
    uri.path_and_query().map_or(uri.path(), |p| p.as_str())
}

fn parse_auth(headers: &HeaderMap) -> Result<RelayAuth, ApiError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(RelayAuth::parse)
        .ok_or_else(|| {
            err(
                StatusCode::UNAUTHORIZED,
                "missing or malformed authorization",
            )
        })
}

/// Freshness + replay window shared by every signed request.
fn check_fresh_once(relay: &Relay, a: &RelayAuth) -> Result<(), ApiError> {
    let now = now_millis();
    if !a.fresh(now) {
        return Err(err(StatusCode::UNAUTHORIZED, "clock skew too large"));
    }
    let mut seen = lock(&relay.replay);
    if seen.len() > 100_000 {
        seen.retain(|_, ts| now - *ts <= 2 * MAX_SKEW_MS);
    }
    if seen.insert(a.sig.clone(), a.ts).is_some() {
        return Err(err(StatusCode::UNAUTHORIZED, "replayed request"));
    }
    Ok(())
}

struct Authed {
    space: String,
    device: String,
}

/// Verify a CTR1 request from an enrolled, active device.
async fn authenticate(
    relay: &Arc<Relay>,
    peer: SocketAddr,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Authed, ApiError> {
    let ip = client_ip(relay, peer, headers);
    let res = authenticate_inner(relay, method, uri, headers, body).await;
    if res.is_err() {
        rate(relay, format!("fail:{ip}"), relay.cfg.ip_rate_per_min)?;
    }
    let a = res?;
    rate(
        relay,
        format!("dev:{}:{}", a.space, a.device),
        relay.cfg.device_rate_per_min,
    )?;
    Ok(a)
}

async fn authenticate_inner(
    relay: &Arc<Relay>,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Authed, ApiError> {
    let a = parse_auth(headers)?;
    let (st, sp, dv) = (relay.store.clone(), a.space.clone(), a.device.clone());
    let dev = blocking(move || st.device(&sp, &dv)).await?;
    let Some(dev) = dev else {
        return Err(err(StatusCode::UNAUTHORIZED, "unknown device or space"));
    };
    if dev.revoked_at.is_some() {
        return Err(err(StatusCode::FORBIDDEN, "device revoked"));
    }
    let public = relay::parse_public(&dev.public)
        .ok_or_else(|| anyhow::anyhow!("stored device key is malformed"))?;
    if !a.verify(&public, method.as_str(), path_and_query(uri), body) {
        return Err(err(StatusCode::UNAUTHORIZED, "bad signature"));
    }
    check_fresh_once(relay, &a)?;
    let now = now_millis();
    if dev.last_seen.is_none_or(|t| now - t > 60_000) {
        let (st, sp, dv) = (relay.store.clone(), a.space.clone(), a.device.clone());
        blocking(move || st.touch(&sp, &dv, now)).await?;
    }
    Ok(Authed {
        space: a.space,
        device: a.device,
    })
}

fn broadcast_to(relay: &Relay, space: &str, msg: RelayMsg) {
    if let Some(tx) = lock(&relay.streams).get(space) {
        let _ = tx.send(msg);
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let (ha, hb) = (clipd_sync::crypto::sha256(a), clipd_sync::crypto::sha256(b));
    ha.iter()
        .zip(hb.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

// ---- handlers ----------------------------------------------------------------

async fn hello(State(relay): State<Arc<Relay>>) -> axum::Json<RelayHello> {
    axum::Json(RelayHello {
        proto: RELAY_PROTO,
        kind: "clipd-relay".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        registration: match relay.cfg.registration {
            Registration::Open => "open",
            Registration::Invite(_) => "invite",
            Registration::Closed => "closed",
        }
        .into(),
        limits: relay.cfg.limits(),
    })
}

async fn register(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let ip = client_ip(&relay, peer, &headers);
    rate(
        &relay,
        format!("reg:{ip}"),
        relay.cfg.ip_rate_per_min.min(10),
    )?;
    match &relay.cfg.registration {
        Registration::Closed => {
            return Err(err(
                StatusCode::FORBIDDEN,
                "this relay does not accept new spaces",
            ));
        }
        Registration::Invite(code) => {
            let given: Option<RegisterRequest> = serde_json::from_slice(&body).ok();
            let ok = given
                .and_then(|r| r.invite)
                .is_some_and(|i| ct_eq(i.as_bytes(), code.as_bytes()));
            if !ok {
                return Err(err(StatusCode::FORBIDDEN, "wrong or missing invite code"));
            }
        }
        Registration::Open => {}
    }
    let req: RegisterRequest = serde_json::from_slice(&body)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "malformed registration"))?;
    if !relay::valid_id(&req.space_id) || !relay::valid_id(&req.device_id) {
        return Err(err(StatusCode::BAD_REQUEST, "bad id"));
    }
    let device_key = relay::parse_public(&req.device_pub)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad device key"))?;
    relay::parse_public(&req.enroll_pub)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad enroll key"))?;
    let a = parse_auth(&headers)?;
    if a.space != req.space_id
        || a.device != req.device_id
        || !a.verify(&device_key, "POST", "/v1/spaces", &body)
    {
        return Err(err(StatusCode::UNAUTHORIZED, "bad signature"));
    }
    check_fresh_once(&relay, &a)?;
    let st = relay.store.clone();
    let max_spaces = relay.cfg.max_spaces as i64;
    let created = blocking(move || {
        if st.space(&req.space_id)?.is_some() {
            return Ok(Err(StatusCode::CONFLICT));
        }
        if st.space_count()? >= max_spaces {
            return Ok(Err(StatusCode::INSUFFICIENT_STORAGE));
        }
        let ok = st.register(
            &req.space_id,
            &req.device_id,
            &req.device_pub,
            &req.enroll_pub,
            now_millis(),
        )?;
        Ok(if ok {
            Ok(())
        } else {
            Err(StatusCode::CONFLICT)
        })
    })
    .await?;
    match created {
        Ok(()) => {
            tracing::info!(space = %a.space, "space registered");
            Ok(StatusCode::CREATED)
        }
        Err(StatusCode::CONFLICT) => Err(err(StatusCode::CONFLICT, "space already registered")),
        Err(code) => Err(err(code, "this relay is full")),
    }
}

async fn enroll(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let ip = client_ip(&relay, peer, &headers);
    rate(&relay, format!("enroll:{ip}"), relay.cfg.ip_rate_per_min)?;
    let req: EnrollRequest = serde_json::from_slice(&body)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "malformed enrollment"))?;
    if !relay::valid_id(&req.space_id) || !relay::valid_id(&req.device_id) {
        return Err(err(StatusCode::BAD_REQUEST, "bad id"));
    }
    let device_key = relay::parse_public(&req.device_pub)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad device key"))?;
    let a = parse_auth(&headers)?;
    if a.space != req.space_id
        || a.device != req.device_id
        || !a.verify(&device_key, "POST", "/v1/enroll", &body)
    {
        return Err(err(StatusCode::UNAUTHORIZED, "bad signature"));
    }
    if (now_millis() - req.ts).abs() > MAX_SKEW_MS {
        return Err(err(StatusCode::UNAUTHORIZED, "clock skew too large"));
    }
    check_fresh_once(&relay, &a)?;
    let (st, sid) = (relay.store.clone(), req.space_id.clone());
    let space = blocking(move || st.space(&sid))
        .await?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such space on this relay"))?;
    let enroll_key = relay::parse_public(&space.enroll_pub)
        .ok_or_else(|| anyhow::anyhow!("stored enroll key is malformed"))?;
    let proof = clipd_sync::crypto::unb64(&req.proof).unwrap_or_default();
    let msg = relay::enroll_message(&req.space_id, &req.device_id, &req.device_pub, req.ts);
    if !relay::verify(&enroll_key, &msg, &proof) {
        return Err(err(
            StatusCode::FORBIDDEN,
            "enroll proof rejected (stale or wrong key)",
        ));
    }
    let st = relay.store.clone();
    let max = relay.cfg.max_devices;
    let r = blocking(move || {
        st.enroll(
            &req.space_id,
            &req.device_id,
            &req.device_pub,
            max,
            now_millis(),
        )
    })
    .await?;
    match r {
        Enrolled::Ok => {
            tracing::info!(space = %a.space, device = %a.device, "device enrolled");
            Ok(StatusCode::NO_CONTENT)
        }
        Enrolled::Revoked => Err(err(StatusCode::FORBIDDEN, "device revoked")),
        Enrolled::Full => Err(err(
            StatusCode::INSUFFICIENT_STORAGE,
            "too many devices in this space",
        )),
        Enrolled::NoSpace => Err(err(StatusCode::NOT_FOUND, "no such space on this relay")),
    }
}

async fn rotate_enroll(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, &body).await?;
    let req: RotateEnrollRequest = serde_json::from_slice(&body)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "malformed request"))?;
    relay::parse_public(&req.enroll_pub)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad enroll key"))?;
    let st = relay.store.clone();
    blocking(move || st.set_enroll_pub(&a.space, &req.enroll_pub)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct SinceQuery {
    #[serde(default)]
    since: i64,
    limit: Option<u32>,
}

async fn get_events(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Query(q): Query<SinceQuery>,
) -> Result<axum::Json<EventsPage>, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, b"").await?;
    let limit = q.limit.unwrap_or(200).clamp(1, MAX_PAGE);
    let st = relay.store.clone();
    let page = blocking(move || {
        Ok(EventsPage {
            events: st.events_since(&a.space, q.since, limit)?,
            head_seq: st.head_seq(&a.space)?,
        })
    })
    .await?;
    Ok(axum::Json(page))
}

async fn post_events(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::Json<PushResponse>, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, &body).await?;
    let req: RelayPushRequest = serde_json::from_slice(&body)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "malformed events"))?;
    if req.events.len() > MAX_PUSH {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "too many events"));
    }
    let mut rejected = Vec::new();
    let mut items = Vec::new();
    for it in req.events {
        let bad = if let Err(e) = it.env.validate_shape() {
            Some(e.to_string())
        } else if it.tag.as_deref().is_some_and(|t| !relay::valid_id(t)) {
            Some("bad tag".to_owned())
        } else if it
            .kind
            .as_deref()
            .is_some_and(|k| !EVENT_KINDS.contains(&k))
        {
            Some("unknown kind".to_owned())
        } else if it
            .blob
            .as_deref()
            .is_some_and(|b| !clipd_sync::engine::valid_blob_id(b))
        {
            Some("bad blob id".to_owned())
        } else {
            // No `env.device_id == a.device` check on purpose: the hub
            // forwards its whole log, i.e. envelopes of LAN-only devices,
            // signed with its own key. The sender id inside an envelope is
            // authenticated by the space key (AEAD), which every member
            // holds anyway, so the relay can't add assurance here; revoked
            // devices are already refused at authentication.
            None
        };
        match bad {
            Some(reason) => rejected.push(Rejected {
                event_id: it.env.event_id,
                reason,
            }),
            None => items.push(it),
        }
    }
    let st = relay.store.clone();
    let space = a.space.clone();
    let max = relay.cfg.max_space_bytes as i64;
    let out = blocking(move || st.push(&space, items, now_millis(), max)).await?;
    for e in &out.fresh {
        broadcast_to(&relay, &a.space, RelayMsg::Event(e.clone()));
    }
    tracing::debug!(space = %a.space, device = %a.device, new = out.fresh.len(), "events pushed");
    rejected.extend(out.rejected);
    Ok(axum::Json(PushResponse {
        accepted: out.accepted,
        rejected,
        head_seq: out.head_seq,
    }))
}

async fn put_blob(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    AxPath(blob_id): AxPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, &body).await?;
    if !clipd_sync::engine::valid_blob_id(&blob_id) {
        return Err(err(StatusCode::BAD_REQUEST, "bad blob id"));
    }
    if body.len() as u64 > relay.cfg.max_blob_bytes || body.len() as u64 > relay.cfg.max_space_bytes
    {
        return Err(err(
            StatusCode::PAYLOAD_TOO_LARGE,
            "blob too large for this relay",
        ));
    }
    if body.len() < 40 {
        return Err(err(StatusCode::BAD_REQUEST, "not a sealed blob"));
    }
    let st = relay.store.clone();
    let max = relay.cfg.max_space_bytes as i64;
    blocking(move || st.put_blob(&a.space, &blob_id, &body, now_millis(), max)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_blob(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    AxPath(blob_id): AxPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, b"").await?;
    if !clipd_sync::engine::valid_blob_id(&blob_id) {
        return Err(err(StatusCode::BAD_REQUEST, "bad blob id"));
    }
    let st = relay.store.clone();
    match blocking(move || st.get_blob(&a.space, &blob_id)).await? {
        Some(b) => Ok(([(header::CONTENT_TYPE, "application/octet-stream")], b).into_response()),
        None => Err(err(StatusCode::NOT_FOUND, "no such blob")),
    }
}

async fn leave(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, b"").await?;
    let (st, sp, dv) = (relay.store.clone(), a.space.clone(), a.device.clone());
    blocking(move || st.revoke(&sp, &dv, now_millis())).await?;
    broadcast_to(&relay, &a.space, RelayMsg::Revoked(a.device));
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke_device(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    AxPath(device_id): AxPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, b"").await?;
    if !relay::valid_id(&device_id) {
        return Err(err(StatusCode::BAD_REQUEST, "bad device id"));
    }
    let (st, sp, dv) = (relay.store.clone(), a.space.clone(), device_id.clone());
    let ok = blocking(move || st.revoke(&sp, &dv, now_millis())).await?;
    if !ok {
        return Err(err(StatusCode::NOT_FOUND, "no such active device"));
    }
    tracing::info!(space = %a.space, device = %device_id, by = %a.device, "device revoked");
    broadcast_to(&relay, &a.space, RelayMsg::Revoked(device_id));
    Ok(StatusCode::NO_CONTENT)
}

async fn space_info(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<axum::Json<SpaceInfo>, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, b"").await?;
    let st = relay.store.clone();
    let (max, days) = (relay.cfg.max_space_bytes as i64, relay.cfg.retention_days);
    let info = blocking(move || {
        let u = st.usage(&a.space)?;
        Ok(SpaceInfo {
            head_seq: st.head_seq(&a.space)?,
            envelopes: u.envelopes,
            blobs: u.blobs,
            bytes: u.bytes,
            max_bytes: max,
            retention_days: days,
            devices: st.devices(&a.space)?,
            space_id: a.space,
        })
    })
    .await?;
    Ok(axum::Json(info))
}

async fn delete_space(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, b"").await?;
    let (st, sp) = (relay.store.clone(), a.space.clone());
    blocking(move || st.delete_space(&sp)).await?;
    broadcast_to(&relay, &a.space, RelayMsg::SpaceDeleted);
    lock(&relay.streams).remove(&a.space);
    tracing::info!(space = %a.space, by = %a.device, "space deleted");
    Ok(StatusCode::NO_CONTENT)
}

async fn stream(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Query(q): Query<SinceQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let a = authenticate(&relay, peer, &method, &uri, &headers, b"").await?;
    Ok(ws.on_upgrade(move |socket| ws_session(relay, a, q.since, socket)))
}

fn frame(ev: &SeqEnvelope) -> Message {
    let json = serde_json::to_string(&StreamFrame {
        t: "event".into(),
        ev: ev.clone(),
    })
    .unwrap_or_default();
    Message::Text(json.into())
}

async fn send_backlog(relay: &Relay, space: &str, socket: &mut WebSocket, last: &mut i64) -> bool {
    loop {
        let (st, sp, since) = (relay.store.clone(), space.to_owned(), *last);
        let page = match tokio::task::spawn_blocking(move || st.events_since(&sp, since, 200)).await
        {
            Ok(Ok(p)) => p,
            _ => return false,
        };
        if page.is_empty() {
            return true;
        }
        for ev in &page {
            if socket.send(frame(ev)).await.is_err() {
                return false;
            }
            *last = ev.seq;
        }
    }
}

async fn ws_session(relay: Arc<Relay>, a: Authed, since: i64, mut socket: WebSocket) {
    let mut rx = lock(&relay.streams)
        .entry(a.space.clone())
        .or_insert_with(|| broadcast::channel(512).0)
        .subscribe();
    tracing::debug!(space = %a.space, device = %a.device, since, "stream opened");
    let mut last = since;
    let mut revoked = false;
    let mut stop = relay.stop.clone();
    let mut ping = tokio::time::interval(PING_EVERY);
    let mut check = tokio::time::interval(REVOKE_CHECK_EVERY);
    if send_backlog(&relay, &a.space, &mut socket, &mut last).await {
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Ok(RelayMsg::Event(ev)) => {
                        if ev.seq > last {
                            if socket.send(frame(&ev)).await.is_err() {
                                break;
                            }
                            last = ev.seq;
                        }
                    }
                    Ok(RelayMsg::Revoked(id)) if id == a.device => {
                        revoked = true;
                        break;
                    }
                    Ok(RelayMsg::Revoked(_)) => {}
                    Ok(RelayMsg::SpaceDeleted) => {
                        revoked = true;
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if !send_backlog(&relay, &a.space, &mut socket, &mut last).await {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                incoming = socket.recv() => match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                },
                _ = ping.tick() => {
                    if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                        break;
                    }
                }
                _ = check.tick() => {
                    let (st, sp, dv) = (relay.store.clone(), a.space.clone(), a.device.clone());
                    let gone = tokio::task::spawn_blocking(move || st.device(&sp, &dv))
                        .await
                        .ok()
                        .and_then(|r| r.ok())
                        .is_none_or(|d| d.is_none_or(|d| d.revoked_at.is_some()));
                    if gone {
                        revoked = true;
                        break;
                    }
                }
                _ = stopped(&mut stop) => break,
            }
        }
    }
    let close = revoked.then(|| CloseFrame {
        code: REVOKED_CLOSE_CODE,
        reason: "revoked".into(),
    });
    let _ = socket.send(Message::Close(close)).await;
    tracing::debug!(space = %a.space, device = %a.device, "stream closed");
}
