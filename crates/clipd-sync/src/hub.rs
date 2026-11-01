//! The LAN hub: an axum HTTP + WebSocket server inside the desktop app.
//!
//! In v1 the hub is a user device holding the MasterKey, so besides
//! storing and relaying envelopes it decrypts and ingests them into its own
//! clip store. Endpoints (all but hello/pair need a `CT1` auth header):
//!
//! | Method | Path | |
//! |---|---|---|
//! | GET | `/v1/hello` | `{proto, space_id, hub_device_id, hub_name, head_seq, hosts}` |
//! | POST | `/v1/pair` | [`PairRequest`] → [`PairResponse`]; proof-gated, rate-limited |
//! | GET | `/v1/events?since=N&limit=200` | `{events: [SeqEnvelope], head_seq}` |
//! | POST | `/v1/events` | `{events: [Envelope]}` → `{accepted, rejected, head_seq}` |
//! | GET (WS) | `/v1/stream?since=N` | server → `{"t":"event", seq, ...Envelope}` |
//! | GET | `/v1/devices` | `[DeviceView]` |
//! | PUT/GET | `/v1/blobs/{blob_id}` | sealed full image (nonce ‖ ct) |
//! | DELETE | `/v1/devices/me` | unpair the calling device |
//!
//! When the space has relays, the hub also runs a bridge
//! ([`crate::relay_link`]) that forwards this log to them and pulls what
//! off-LAN devices posted there into it.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path as AxPath, Query, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use clipd_core::{Millis, now_millis};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, watch};
use tokio::task::JoinHandle;

use crate::auth::{AuthHeader, MAX_SKEW_MS};
use crate::crypto::aad;
use crate::db::DeviceRow;
use crate::engine::{Engine, JOURNAL_BATCH, SyncNotice, valid_blob_id};
use crate::envelope::SeqEnvelope;
use crate::keystore::KeyStore;
use crate::pairing::{PairError, PairOffers, PairRequest};
use crate::wire::{EventsPage, Hello, PushRequest, PushResponse, Rejected, StreamFrame};
use crate::{DeviceView, PROTOCOL_VERSION, SyncStatus, hub_append, init_hub};

/// Max envelopes per `POST /v1/events`.
pub const MAX_PUSH: usize = 500;
/// Max page size of `GET /v1/events`.
pub const MAX_PAGE: u32 = 500;
const EVENTS_BODY_LIMIT: usize = 64 * 1024 * 1024;
const BLOB_BODY_LIMIT: usize = clipd_core::synced::MAX_SYNCED_IMAGE_BYTES + 1024;
const PING_EVERY: Duration = Duration::from_secs(20);
const REVOKE_CHECK_EVERY: Duration = Duration::from_secs(5);
const LOCAL_POLL: Duration = Duration::from_secs(1);
/// WebSocket close code sent to a device that was just revoked.
pub const REVOKED_CLOSE_CODE: u16 = 4001;

pub struct HubConfig {
    pub data_dir: PathBuf,
    pub keystore: Arc<dyn KeyStore>,
    /// 0 = pick a free port.
    pub port: u16,
    /// Usually 0.0.0.0 (LAN); tests use 127.0.0.1.
    pub bind: IpAddr,
    /// Hosts to put in the QR (`host` or `host:port`). None = every
    /// private IPv4 of this machine.
    pub advertise: Option<Vec<String>>,
    /// Display name for this hub.
    pub name: String,
}

impl HubConfig {
    pub fn new(data_dir: PathBuf, keystore: Arc<dyn KeyStore>) -> HubConfig {
        HubConfig {
            data_dir,
            keystore,
            port: crate::DEFAULT_PORT,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            advertise: None,
            name: crate::default_device_name(),
        }
    }
}

#[derive(Clone, Debug)]
enum HubMsg {
    Event(SeqEnvelope),
    Revoked(String),
}

struct Hub {
    engine: Arc<Engine>,
    offers: Mutex<PairOffers>,
    tx: broadcast::Sender<HubMsg>,
    notices: broadcast::Sender<SyncNotice>,
    online: Mutex<HashMap<String, usize>>,
    replay: Mutex<HashMap<Vec<u8>, Millis>>,
    local: Notify,
    stop: watch::Receiver<bool>,
    port: u16,
    hosts: Vec<String>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A pairing QR to show.
#[derive(Debug, Clone, Serialize)]
pub struct OfferView {
    pub uri: String,
    pub expires_at: Millis,
    pub hosts: Vec<String>,
    /// The QR code as an SVG document.
    pub svg: String,
}

/// A running hub. Dropping it does not stop the server; call
/// [`HubHandle::shutdown`].
pub struct HubHandle {
    hub: Arc<Hub>,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    addr: SocketAddr,
    relays: crate::relay_link::LinkHandle,
}

/// Start the hub (creating the space on first run). Must be called inside
/// a multi-threaded tokio runtime.
pub async fn start(cfg: HubConfig) -> Result<HubHandle> {
    let HubConfig {
        data_dir,
        keystore,
        port,
        bind,
        advertise,
        name,
    } = cfg;
    let engine = {
        let (dd, ks) = (data_dir.clone(), keystore.clone());
        tokio::task::spawn_blocking(move || -> Result<Engine> {
            init_hub(&dd, &*ks, &name)?;
            Engine::open(&dd, &*ks)
        })
        .await
        .context("hub init task")??
    };
    let listener = tokio::net::TcpListener::bind((bind, port))
        .await
        .with_context(|| format!("couldn't listen on port {port} (in use or blocked)"))?;
    let addr = listener.local_addr()?;
    engine.db.set("port", &addr.port().to_string())?;
    let hosts = advertise
        .unwrap_or_else(lan_ipv4s)
        .into_iter()
        .map(|h| {
            if h.contains(':') {
                h
            } else {
                format!("{h}:{}", addr.port())
            }
        })
        .collect::<Vec<_>>();

    let (stop_tx, stop_rx) = watch::channel(false);
    let (tx, _) = broadcast::channel(1024);
    let (notices, _) = broadcast::channel(256);
    let hub = Arc::new(Hub {
        engine: Arc::new(engine),
        offers: Mutex::new(PairOffers::default()),
        tx,
        notices,
        online: Mutex::new(HashMap::new()),
        replay: Mutex::new(HashMap::new()),
        local: Notify::new(),
        stop: stop_rx.clone(),
        port: addr.port(),
        hosts,
    });

    let app = router(hub.clone());
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
            tracing::warn!("hub server stopped: {e}");
        }
    });

    let pump_hub = hub.clone();
    let mut stop_pump = stop_rx;
    let pump = tokio::spawn(async move {
        loop {
            pump_local(&pump_hub).await;
            tokio::select! {
                _ = pump_hub.local.notified() => {}
                _ = tokio::time::sleep(LOCAL_POLL) => {}
                _ = crate::engine::stopped(&mut stop_pump) => break,
            }
        }
    });

    // Relay bridge: forwards the log off-LAN, pulls relay events in.
    let publish_tx = hub.tx.clone();
    let relays = crate::relay_link::start_hub(
        hub.engine.clone(),
        keystore,
        hub.notices.clone(),
        Arc::new(move |env| {
            let _ = publish_tx.send(HubMsg::Event(env));
        }),
    );
    let waker = relays.waker();
    let mut log_rx = hub.tx.subscribe();
    let mut stop_fwd = hub.stop.clone();
    let forward = tokio::spawn(async move {
        loop {
            tokio::select! {
                m = log_rx.recv() => match m {
                    Ok(HubMsg::Event(_)) | Err(broadcast::error::RecvError::Lagged(_)) => waker.wake(),
                    Ok(HubMsg::Revoked(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = crate::engine::stopped(&mut stop_fwd) => break,
            }
        }
    });

    tracing::info!(port = addr.port(), "sync hub listening");
    Ok(HubHandle {
        hub,
        stop: stop_tx,
        tasks: vec![server, pump, forward],
        addr,
        relays,
    })
}

impl HubHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn space_id(&self) -> &str {
        &self.hub.engine.space.space_id
    }

    /// Make a new one-time pairing QR (valid 10 minutes).
    pub fn create_offer(&self) -> OfferView {
        let now = now_millis();
        let uri = lock(&self.hub.offers).create(
            self.hub.hosts.clone(),
            &self.hub.engine.space.hub_name,
            now,
        );
        let text = uri.to_uri();
        OfferView {
            svg: qr_svg(&text),
            uri: text,
            expires_at: now + crate::pairing::OFFER_TTL_MS,
            hosts: self.hub.hosts.clone(),
        }
    }

    /// Withdraw unused QR codes.
    pub fn cancel_offers(&self) {
        lock(&self.hub.offers).cancel_all();
    }

    /// Revoke a device: its token dies and its live stream is closed.
    pub fn revoke(&self, device_id: &str) -> Result<bool> {
        let ok = self.hub.engine.db.revoke_device(device_id, now_millis())?;
        if ok {
            // The bridge also deletes its key on relays and rotates the
            // space enroll key.
            self.relays.wake();
            let _ = self.hub.tx.send(HubMsg::Revoked(device_id.to_owned()));
            let _ = self.hub.notices.send(SyncNotice::StatusChanged);
            tracing::info!(device = %device_id, "device revoked");
        }
        Ok(ok)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SyncNotice> {
        self.hub.notices.subscribe()
    }

    /// The relay list changed (e.g. after [`crate::relay_add`]): apply it
    /// now instead of within ~2 s.
    pub fn relay_changed(&self) {
        self.relays.wake();
    }

    /// Something was copied locally: publish it now instead of on the next
    /// poll (1 s).
    pub fn notify_local_change(&self) {
        self.hub.local.notify_one();
    }

    pub fn status(&self) -> Result<SyncStatus> {
        let e = &self.hub.engine;
        let online = lock(&self.hub.online).clone();
        let devices: Vec<DeviceView> =
            e.db.devices()?
                .into_iter()
                .map(|d| {
                    let on = d.is_self || online.get(&d.id).is_some_and(|n| *n > 0);
                    DeviceView::from_row(d, on)
                })
                .collect();
        let head = e.db.head_seq()?;
        Ok(SyncStatus {
            configured: true,
            role: Some(crate::Role::Hub),
            space_id: Some(e.space.space_id.clone()),
            device_id: e.space.device_id.clone(),
            device_name: Some(e.space.device_name.clone()),
            hub_name: Some(e.space.hub_name.clone()),
            running: true,
            connected: online.values().any(|n| *n > 0),
            port: Some(self.hub.port),
            hosts: self.hub.hosts.clone(),
            devices,
            outbox: 0,
            pending_images: e.db.pending_blob_count()?,
            last_seq: head,
            head_seq: head,
            last_error: None,
            keystore: String::new(),
            relays: self.relays.views(),
        })
    }

    /// Stop the server and close every stream.
    pub async fn shutdown(self) {
        // Flush local changes into the log, then the log tail to the
        // relay: clips copied right before sleep must reach devices that
        // sync only through the relay. Bounded — a dead relay must never
        // block shutdown.
        pump_local(&self.hub).await;
        self.relays.wake();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let head = self.hub.engine.db.head_seq().unwrap_or(0);
            let behind = self
                .hub
                .engine
                .db
                .relays()
                .map(|rows| rows.iter().any(|r| r.enrolled && r.push_seq < head))
                .unwrap_or(false);
            if !behind || std::time::Instant::now() >= deadline {
                break;
            }
            self.relays.wake();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.relays.shutdown().await;
        let _ = self.stop.send(true);
        for t in self.tasks {
            let _ = tokio::time::timeout(Duration::from_secs(5), t).await;
        }
        tracing::info!("sync hub stopped");
    }
}

/// Every LAN IPv4 of this machine, private ranges first.
pub fn lan_ipv4s() -> Vec<String> {
    let mut v: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.ip() {
            IpAddr::V4(ip) if !ip.is_loopback() && (ip.is_private() || is_cgnat(ip)) => Some(ip),
            _ => None,
        })
        .collect();
    v.sort_by_key(|ip| (!ip.is_private(), ip.octets()[0] != 192, *ip));
    v.dedup();
    v.into_iter().map(|ip| ip.to_string()).collect()
}

fn is_cgnat(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (64..128).contains(&o[1])
}

/// Requests from outside the LAN (or a VPN like Tailscale) are refused.
pub fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local() || is_cgnat(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_lan(IpAddr::V4(v4));
            }
            let seg = v6.segments()[0];
            v6.is_loopback() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
        }
    }
}

/// QR code for `data` as an SVG document.
pub fn qr_svg(data: &str) -> String {
    match qrcode::QrCode::with_error_correction_level(data, qrcode::EcLevel::M) {
        Ok(code) => code
            .render::<qrcode::render::svg::Color<'_>>()
            .min_dimensions(240, 240)
            .quiet_zone(true)
            .dark_color(qrcode::render::svg::Color("#000000"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build(),
        Err(_) => String::new(),
    }
}

/// QR code for `data` drawn with Unicode half blocks (terminals).
pub fn qr_terminal(data: &str) -> String {
    match qrcode::QrCode::with_error_correction_level(data, qrcode::EcLevel::L) {
        Ok(code) => code
            .render::<qrcode::render::unicode::Dense1x2>()
            .dark_color(qrcode::render::unicode::Dense1x2::Light)
            .light_color(qrcode::render::unicode::Dense1x2::Dark)
            .quiet_zone(true)
            .build(),
        Err(_) => String::new(),
    }
}

async fn pump_local(hub: &Arc<Hub>) {
    loop {
        let engine = hub.engine.clone();
        let res = tokio::task::spawn_blocking(move || -> Result<(Vec<SeqEnvelope>, usize)> {
            let (events, cursor) = engine.collect_local()?;
            let n = events.len();
            let mut out = Vec::new();
            for ev in &events {
                if let Some(s) = hub_append(&engine, ev)? {
                    out.push(s);
                }
            }
            engine.commit_cursor(cursor)?;
            Ok((out, n))
        })
        .await;
        match res {
            Ok(Ok((envs, n))) => {
                for e in envs {
                    tracing::debug!(seq = e.seq, event = %e.env.event_id, "published local event");
                    let _ = hub.tx.send(HubMsg::Event(e));
                }
                if n < JOURNAL_BATCH as usize {
                    return;
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("publishing local changes failed: {e:#}");
                return;
            }
            Err(_) => return,
        }
    }
}

fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/v1/hello", get(hello))
        .route("/v1/pair", post(pair))
        .route(
            "/v1/events",
            get(get_events)
                .post(post_events)
                .layer(DefaultBodyLimit::max(EVENTS_BODY_LIMIT)),
        )
        .route("/v1/stream", get(stream))
        .route("/v1/devices", get(devices))
        .route("/v1/devices/me", axum::routing::delete(unpair_me))
        .route(
            "/v1/blobs/{blob_id}",
            put(put_blob)
                .get(get_blob)
                .layer(DefaultBodyLimit::max(BLOB_BODY_LIMIT)),
        )
        .layer(middleware::from_fn(lan_only))
        .with_state(hub)
}

async fn lan_only(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if is_lan(addr.ip()) {
        next.run(req).await
    } else {
        ApiError(StatusCode::FORBIDDEN, "local network only".into()).into_response()
    }
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, axum::Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        // Our errors never carry clip content, but keep details server-side.
        tracing::warn!("sync request failed: {e:#}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
    }
}

fn unauthorized(msg: &str) -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, msg.into())
}

pub(crate) fn token_aad(device_id: &str) -> Vec<u8> {
    aad(&[b"cliptrail-token-v1", device_id.as_bytes()])
}

/// Check a `CT1` header; returns the device id.
fn authenticate(
    hub: &Hub,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<String, ApiError> {
    let h = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(AuthHeader::parse)
        .ok_or_else(|| unauthorized("missing or malformed authorization"))?;
    let now = now_millis();
    if !h.fresh(now) {
        return Err(unauthorized("clock skew too large"));
    }
    let sealed = hub
        .engine
        .db
        .active_token(&h.device_id)?
        .ok_or_else(|| unauthorized("device is not paired or was revoked"))?;
    let token = hub
        .engine
        .keys
        .wrap
        .open_packed(&token_aad(&h.device_id), &sealed)
        .map_err(|_| unauthorized("device token unreadable"))?;
    let pq = uri.path_and_query().map_or(uri.path(), |p| p.as_str());
    if !h.verify(&token, method.as_str(), pq, body) {
        return Err(unauthorized("bad signature"));
    }
    {
        let mut seen = lock(&hub.replay);
        if seen.len() > 4096 {
            seen.retain(|_, ts| now - *ts <= 2 * MAX_SKEW_MS);
        }
        if seen.insert(h.mac.clone(), h.ts).is_some() {
            return Err(unauthorized("replayed request"));
        }
    }
    hub.engine.db.touch_device(&h.device_id, now)?;
    Ok(h.device_id)
}

async fn hello(State(hub): State<Arc<Hub>>) -> Result<axum::Json<Hello>, ApiError> {
    let e = &hub.engine;
    Ok(axum::Json(Hello {
        proto: PROTOCOL_VERSION,
        space_id: e.space.space_id.clone(),
        hub_device_id: e.space.device_id.clone(),
        hub_name: e.space.hub_name.clone(),
        head_seq: e.db.head_seq()?,
        hosts: hub.hosts.clone(),
    }))
}

async fn pair(State(hub): State<Arc<Hub>>, body: Bytes) -> Result<Response, ApiError> {
    let req: PairRequest = serde_json::from_slice(&body)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "malformed pairing request".into()))?;
    let e = &hub.engine;
    if req.device_id == e.space.device_id {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "that is this device".into(),
        ));
    }
    let now = now_millis();
    let res = lock(&hub.offers).accept(
        &req,
        now,
        &e.space.space_id,
        &e.space.device_id,
        &e.space.hub_name,
        e.keys.master(),
    );
    let (resp, dev) = match res {
        Ok(v) => v,
        Err(err) => {
            tracing::info!(error = %err, "pairing attempt refused");
            let code = match err {
                PairError::BadProof => StatusCode::FORBIDDEN,
                PairError::Expired => StatusCode::GONE,
                PairError::AlreadyUsed => StatusCode::CONFLICT,
                PairError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
                _ => StatusCode::BAD_REQUEST,
            };
            return Err(ApiError(code, err.to_string()));
        }
    };
    let sealed = e
        .keys
        .wrap
        .seal_packed(&token_aad(&dev.device_id), dev.token.as_bytes())
        .map_err(|_| anyhow::anyhow!("seal device token"))?;
    e.db.upsert_device(
        &DeviceRow {
            id: dev.device_id.clone(),
            name: dev.name.clone(),
            platform: dev.platform.clone(),
            paired_at: now,
            revoked_at: None,
            last_seen: Some(now),
            is_self: false,
        },
        Some(&sealed),
    )?;
    tracing::info!(device = %dev.device_id, platform = %dev.platform, "device paired");
    let _ = hub.notices.send(SyncNotice::Paired {
        device_id: dev.device_id,
        name: dev.name,
        platform: dev.platform,
        check_code: dev.check_code,
    });
    let _ = hub.notices.send(SyncNotice::StatusChanged);
    Ok(axum::Json(resp).into_response())
}

#[derive(Debug, Deserialize)]
struct SinceQuery {
    #[serde(default)]
    since: i64,
    limit: Option<u32>,
}

async fn get_events(
    State(hub): State<Arc<Hub>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Query(q): Query<SinceQuery>,
) -> Result<axum::Json<EventsPage>, ApiError> {
    authenticate(&hub, &method, &uri, &headers, b"")?;
    let limit = q.limit.unwrap_or(200).clamp(1, MAX_PAGE);
    Ok(axum::Json(EventsPage {
        events: hub.engine.db.envelopes_since(q.since, limit)?,
        head_seq: hub.engine.db.head_seq()?,
    }))
}

async fn post_events(
    State(hub): State<Arc<Hub>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::Json<PushResponse>, ApiError> {
    let device = authenticate(&hub, &method, &uri, &headers, &body)?;
    let req: PushRequest = serde_json::from_slice(&body)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "malformed events".into()))?;
    if req.events.len() > MAX_PUSH {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too many events".into(),
        ));
    }
    let engine = hub.engine.clone();
    let (resp, fresh, changed, metas) = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        let mut fresh = Vec::new();
        let mut metas = Vec::new();
        let mut changed = 0usize;
        for env in req.events {
            if env.device_id != device {
                rejected.push(Rejected {
                    event_id: env.event_id,
                    reason: "sender mismatch".into(),
                });
                continue;
            }
            let event = match env.open(&engine.keys, &engine.space.space_id) {
                Ok(ev) => ev,
                Err(err) => {
                    rejected.push(Rejected {
                        event_id: env.event_id,
                        reason: err.to_string(),
                    });
                    continue;
                }
            };
            let hash = event.hash()?;
            let (seq, new) = engine.db.append_envelope(
                &env,
                &engine.keys.hash_tag(&hash),
                event.kind_name(),
                now_millis(),
            )?;
            if new {
                match engine.apply_event(&event, &env.device_id) {
                    Ok(applied) => {
                        if applied.changed {
                            changed += 1;
                        }
                        metas.push(SyncNotice::Event {
                            seq,
                            kind: event.kind_name().to_owned(),
                            device_id: env.device_id.clone(),
                            event_id: env.event_id.clone(),
                            applied: applied.changed,
                        });
                        if let Some((blob_id, h)) = applied.blob {
                            attach_uploaded_blob(&engine, &blob_id, &h);
                        }
                    }
                    Err(err) => tracing::warn!(event = %env.event_id, "hub couldn't apply event: {err:#}"),
                }
                tracing::debug!(seq, kind = event.kind_name(), from = %env.device_id, "event received");
                fresh.push(SeqEnvelope {
                    seq,
                    env: env.clone(),
                });
            }
            accepted.push(env.event_id);
        }
        Ok((
            PushResponse {
                accepted,
                rejected,
                head_seq: engine.db.head_seq()?,
            },
            fresh,
            changed,
            metas,
        ))
    })
    .await
    .map_err(|_| anyhow::anyhow!("events task failed"))??;
    for e in fresh {
        let _ = hub.tx.send(HubMsg::Event(e));
    }
    for m in metas {
        let _ = hub.notices.send(m);
    }
    if changed > 0 {
        let _ = hub
            .notices
            .send(SyncNotice::ClipsChanged { count: changed });
    }
    Ok(axum::Json(resp))
}

/// Hub: a blob that arrived before (or with) its event → attach it.
fn attach_uploaded_blob(engine: &Engine, blob_id: &str, hash: &[u8]) {
    let path = engine.blob_dir().join(blob_id);
    let Ok(sealed) = std::fs::read(&path) else {
        return;
    };
    match engine.attach_blob(blob_id, hash, &sealed) {
        Ok(true) => {
            let _ = std::fs::remove_file(&path);
            let _ = engine.db.remove_pending_blob(blob_id);
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(blob = %blob_id, "couldn't attach uploaded image: {e:#}");
            let _ = std::fs::remove_file(&path);
            let _ = engine.db.remove_pending_blob(blob_id);
        }
    }
}

async fn put_blob(
    State(hub): State<Arc<Hub>>,
    AxPath(blob_id): AxPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    authenticate(&hub, &method, &uri, &headers, &body)?;
    if !valid_blob_id(&blob_id) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "bad blob id".into()));
    }
    let engine = hub.engine.clone();
    let changed = tokio::task::spawn_blocking(move || -> Result<Result<bool, ApiError>> {
        if !engine.verify_blob(&blob_id, &body) {
            return Ok(Err(ApiError(
                StatusCode::BAD_REQUEST,
                "blob does not decrypt".into(),
            )));
        }
        let dir = engine.blob_dir();
        std::fs::create_dir_all(&dir).context("create blob dir")?;
        std::fs::write(dir.join(&blob_id), &body).context("store blob")?;
        let hash = match engine.db.pending_blob_hash(&blob_id)? {
            Some(h) => Some(h),
            None => engine.db.blob_hash(&blob_id)?,
        };
        if let Some(h) = hash {
            let had = engine
                .pipeline
                .store()
                .get_by_hash(&h)?
                .is_some_and(|c| c.clip.image_path.is_some());
            if had {
                let _ = std::fs::remove_file(dir.join(&blob_id));
                return Ok(Ok(false));
            }
            attach_uploaded_blob(&engine, &blob_id, &h);
            return Ok(Ok(true));
        }
        Ok(Ok(false))
    })
    .await
    .map_err(|_| anyhow::anyhow!("blob task failed"))???;
    if changed {
        let _ = hub.notices.send(SyncNotice::ClipsChanged { count: 1 });
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn get_blob(
    State(hub): State<Arc<Hub>>,
    AxPath(blob_id): AxPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authenticate(&hub, &method, &uri, &headers, b"")?;
    if !valid_blob_id(&blob_id) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "bad blob id".into()));
    }
    let engine = hub.engine.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Option<Vec<u8>>> {
        if let Ok(b) = std::fs::read(engine.blob_dir().join(&blob_id)) {
            return Ok(Some(b));
        }
        match engine.db.blob_hash(&blob_id)? {
            Some(h) => engine.seal_blob(&blob_id, &h),
            None => Ok(None),
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("blob task failed"))??;
    match bytes {
        Some(b) => Ok(([(header::CONTENT_TYPE, "application/octet-stream")], b).into_response()),
        None => Err(ApiError(StatusCode::NOT_FOUND, "no such blob".into())),
    }
}

async fn devices(
    State(hub): State<Arc<Hub>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<axum::Json<Vec<DeviceView>>, ApiError> {
    authenticate(&hub, &method, &uri, &headers, b"")?;
    let online = lock(&hub.online).clone();
    Ok(axum::Json(
        hub.engine
            .db
            .devices()?
            .into_iter()
            .filter(|d| d.revoked_at.is_none())
            .map(|d| {
                let on = d.is_self || online.get(&d.id).is_some_and(|n| *n > 0);
                DeviceView::from_row(d, on)
            })
            .collect(),
    ))
}

async fn unpair_me(
    State(hub): State<Arc<Hub>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let device = authenticate(&hub, &method, &uri, &headers, b"")?;
    if hub.engine.db.revoke_device(&device, now_millis())? {
        let _ = hub.tx.send(HubMsg::Revoked(device));
        let _ = hub.notices.send(SyncNotice::StatusChanged);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn stream(
    State(hub): State<Arc<Hub>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Query(q): Query<SinceQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let device = authenticate(&hub, &method, &uri, &headers, b"")?;
    Ok(ws.on_upgrade(move |socket| ws_session(hub, device, q.since, socket)))
}

fn frame(ev: &SeqEnvelope) -> Message {
    let json = serde_json::to_string(&StreamFrame {
        t: "event".into(),
        ev: ev.clone(),
    })
    .unwrap_or_default();
    Message::Text(json.into())
}

async fn send_backlog(hub: &Hub, socket: &mut WebSocket, last: &mut i64) -> bool {
    loop {
        let page = match hub.engine.db.envelopes_since(*last, 200) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("stream backlog: {e:#}");
                return false;
            }
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

async fn ws_session(hub: Arc<Hub>, device: String, since: i64, mut socket: WebSocket) {
    let mut rx = hub.tx.subscribe();
    *lock(&hub.online).entry(device.clone()).or_insert(0) += 1;
    let _ = hub.notices.send(SyncNotice::StatusChanged);
    tracing::info!(device = %device, since, "device connected");

    let mut last = since;
    let mut revoked = false;
    let mut stop = hub.stop.clone();
    let mut ping = tokio::time::interval(PING_EVERY);
    let mut check = tokio::time::interval(REVOKE_CHECK_EVERY);
    if send_backlog(&hub, &mut socket, &mut last).await {
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Ok(HubMsg::Event(ev)) => {
                        if ev.seq > last {
                            if socket.send(frame(&ev)).await.is_err() {
                                break;
                            }
                            last = ev.seq;
                        }
                    }
                    Ok(HubMsg::Revoked(id)) if id == device => {
                        revoked = true;
                        break;
                    }
                    Ok(HubMsg::Revoked(_)) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if !send_backlog(&hub, &mut socket, &mut last).await {
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
                    let _ = hub.engine.db.touch_device(&device, now_millis());
                }
                _ = check.tick() => {
                    if hub.engine.db.is_revoked(&device).unwrap_or(false) {
                        revoked = true;
                        break;
                    }
                }
                _ = crate::engine::stopped(&mut stop) => break,
            }
        }
    }
    // Tell a revoked device why, so it stops retrying at once.
    let close = revoked.then(|| axum::extract::ws::CloseFrame {
        code: REVOKED_CLOSE_CODE,
        reason: "revoked".into(),
    });
    let _ = socket.send(Message::Close(close)).await;
    {
        let mut online = lock(&hub.online);
        if let Some(n) = online.get_mut(&device) {
            *n = n.saturating_sub(1);
        }
    }
    let _ = hub.engine.db.touch_device(&device, now_millis());
    let _ = hub.notices.send(SyncNotice::StatusChanged);
    tracing::info!(device = %device, "device disconnected");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_ranges() {
        for ip in [
            "127.0.0.1",
            "192.168.1.2",
            "10.1.2.3",
            "172.16.0.1",
            "100.100.1.1",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:192.168.0.1",
        ] {
            assert!(is_lan(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "172.32.0.1", "2001:db8::1", "::ffff:1.1.1.1"] {
            assert!(!is_lan(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn qr_renders() {
        let uri = "cliptrail://pair?v=1&h=192.168.1.2:47821&k=AAAA&s=BBBB&n=PC";
        assert!(qr_svg(uri).starts_with("<?xml"));
        assert!(qr_terminal(uri).lines().count() > 10);
    }
}
