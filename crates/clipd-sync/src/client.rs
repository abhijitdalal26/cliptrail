//! The sync client: pairs with a hub, then keeps this data dir in sync.
//!
//! Plain Rust + tokio (reqwest without TLS, tokio-tungstenite): usable from
//! a desktop process and from Android. Call [`start`] from inside a
//! multi-threaded tokio runtime (e.g. `rt.block_on(async { start(cfg) })`
//! or with `rt.enter()` held).
//!
//! Loop: hello (pick a working host) → push outbox → catch up with
//! `GET /v1/events?since=` → stream `/v1/stream` → apply live events, push
//! local changes as they happen. Offline: local changes keep going to the
//! persisted outbox; reconnect with backoff 1 s … 60 s.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
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

use crate::crypto::SecretKey;
use crate::db::{DeviceRow, Role, SpaceState};
use crate::engine::{Engine, JOURNAL_BATCH, SyncNotice, open_db, open_pipeline};
use crate::envelope::{Envelope, SeqEnvelope};
use crate::keystore::{DEVICE_TOKEN, KeyStore, MASTER_KEY};
use crate::pairing::{ClientPairing, PairResponse, PairUri};
use crate::{DeviceView, SyncStatus, auth, client_enqueue, local_device_id};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const STREAM_IDLE: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const OUTBOX_BATCH: u32 = 100;
const BLOB_GIVE_UP: i64 = 12;

/// The hub no longer accepts this device (revoked or unpaired).
#[derive(Debug, thiserror::Error)]
#[error("this device was unpaired by the hub; pair again")]
pub struct Unpaired;

pub struct ClientConfig {
    pub data_dir: PathBuf,
    pub keystore: Arc<dyn KeyStore>,
    /// Download full images right after their clip arrives. When false,
    /// only thumbnails sync until [`ClientHandle::fetch_image`] is called.
    pub fetch_images: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairOutcome {
    pub space_id: String,
    pub hub_name: String,
    pub hub_device_id: String,
    /// Same 4 digits the hub shows.
    pub check_code: String,
    pub host: String,
    /// Local clips queued to share with the space.
    pub backfilled: usize,
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .no_proxy()
        .build()
        .context("build HTTP client")
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| anyhow!("sync worker panicked"))?
}

async fn error_message(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.bytes().await.unwrap_or_default();
    serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_owned))
        .unwrap_or_else(|| format!("HTTP {status}"))
}

/// Pair this data dir with the hub in a `cliptrail://pair?...` link.
pub async fn pair(
    data_dir: &Path,
    keystore: Arc<dyn KeyStore>,
    uri: &str,
    name: &str,
    platform: &str,
) -> Result<PairOutcome> {
    let uri = PairUri::parse(uri)?;
    let dd = data_dir.to_path_buf();
    let (existing, device_id) = blocking(move || {
        let db = open_db(&dd)?;
        Ok((db.space()?, local_device_id(&dd)?))
    })
    .await?;
    if let Some(s) = existing {
        match s.role {
            Role::Hub => bail!("this data dir is a sync hub; reset sync before pairing it"),
            Role::Client => bail!(
                "this data dir is already paired with \"{}\"; reset sync first",
                s.hub_name
            ),
        }
    }
    let name = if name.trim().is_empty() {
        crate::default_device_name()
    } else {
        name.trim().to_owned()
    };
    let pairing = ClientPairing::new(uri.clone(), &device_id, &name, platform);
    let body = serde_json::to_vec(pairing.request())?;
    let http = http()?;
    let mut found: Option<(String, PairResponse)> = None;
    let mut last_err = anyhow!("the pairing link has no hosts");
    for host in &uri.hosts {
        let res = http
            .post(format!("http://{host}/v1/pair"))
            .header("content-type", "application/json")
            .body(body.clone())
            .send()
            .await;
        match res {
            Ok(r) if r.status().is_success() => {
                let bytes = r.bytes().await.context("read pairing reply")?;
                let resp: PairResponse =
                    serde_json::from_slice(&bytes).context("malformed pairing reply")?;
                found = Some((host.clone(), resp));
                break;
            }
            Ok(r) => bail!("the hub refused pairing: {}", error_message(r).await),
            Err(e) => {
                tracing::debug!(%host, "pair attempt failed: {e}");
                last_err = anyhow!(
                    "couldn't reach the hub at {} (same Wi-Fi? firewall set to allow ClipTrail on Private networks?)",
                    uri.hosts.join(", ")
                );
            }
        }
    }
    let (host, resp) = found.ok_or(last_err)?;
    let result = pairing.finish(&resp)?;

    let dd = data_dir.to_path_buf();
    let mut hosts = vec![host.clone()];
    hosts.extend(uri.hosts.iter().filter(|h| **h != host).cloned());
    let space = SpaceState {
        role: Role::Client,
        space_id: result.space_id.clone(),
        device_id,
        device_name: name,
        hub_device_id: result.hub_device_id.clone(),
        hub_name: result.hub_name.clone(),
        hub_hosts: hosts,
    };
    let (master, token) = (result.master_key.clone(), result.token.clone());
    let backfilled = blocking(move || {
        keystore.set(MASTER_KEY, master.as_bytes())?;
        keystore.set(DEVICE_TOKEN, token.as_bytes())?;
        let db = open_db(&dd)?;
        let pipeline = open_pipeline(&dd)?;
        db.set_i64("journal_cursor", pipeline.store().journal_head()?)?;
        db.set_i64("last_seq", 0)?;
        db.save_space(&space)?;
        drop((db, pipeline));
        let engine = Engine::open(&dd, &*keystore)?;
        let events = engine.backfill(crate::engine::BACKFILL_LIMIT)?;
        for ev in &events {
            client_enqueue(&engine, ev)?;
        }
        Ok(events.len())
    })
    .await?;
    tracing::info!(space = %result.space_id, backfilled, "paired with hub");
    Ok(PairOutcome {
        space_id: result.space_id,
        hub_name: result.hub_name,
        hub_device_id: result.hub_device_id,
        check_code: result.check_code,
        host,
        backfilled,
    })
}

/// Leave the space from a client: tell the hub (`DELETE /v1/devices/me`,
/// best effort: the hub may be off) and forget the local sync state and
/// keys. Clips stay. Stop the client loop first. Returns true when the hub
/// confirmed; false when it couldn't be reached (it can still revoke this
/// device from its own device list).
pub async fn unpair(data_dir: &Path, keystore: Arc<dyn KeyStore>) -> Result<bool> {
    let dd = data_dir.to_path_buf();
    let ks = keystore.clone();
    let found = blocking(move || {
        let db = open_db(&dd)?;
        let Some(space) = db.space()? else {
            return Ok(None);
        };
        if space.role != Role::Client {
            bail!("this device is the sync hub; turn sync off instead");
        }
        let token = ks.get(DEVICE_TOKEN)?;
        Ok(Some((space, token)))
    })
    .await?;
    let Some((space, token)) = found else {
        return Ok(false);
    };
    let mut confirmed = false;
    if let Some(token) = token {
        let http = http()?;
        let path = "/v1/devices/me";
        for host in &space.hub_hosts {
            let sig = auth::sign(&space.device_id, &token, "DELETE", path, now_millis(), b"");
            let res = http
                .delete(format!("http://{host}{path}"))
                .header("authorization", sig)
                .timeout(Duration::from_secs(4))
                .send()
                .await;
            match res {
                Ok(r) if r.status().is_success() => {
                    confirmed = true;
                    break;
                }
                // Reached the hub, which no longer knows us: same outcome.
                Ok(r) if r.status() == reqwest::StatusCode::UNAUTHORIZED => {
                    confirmed = true;
                    break;
                }
                Ok(r) => tracing::debug!(%host, status = %r.status(), "unpair refused"),
                Err(e) => tracing::debug!(%host, "unpair attempt failed: {e}"),
            }
        }
    }
    let dd = data_dir.to_path_buf();
    blocking(move || crate::reset(&dd, &*keystore)).await?;
    tracing::info!(confirmed, "left the sync space");
    Ok(confirmed)
}

#[derive(Default)]
struct LiveState {
    connected: bool,
    host: Option<String>,
    last_error: Option<String>,
    unpaired: bool,
    reached_stream: bool,
}

struct Shared {
    engine: Arc<Engine>,
    token: SecretKey,
    http: reqwest::Client,
    notices: broadcast::Sender<SyncNotice>,
    local: Notify,
    live: Mutex<LiveState>,
    fetch_images: bool,
    stop: watch::Receiver<bool>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A running client loop.
pub struct ClientHandle {
    shared: Arc<Shared>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

/// Start syncing a paired data dir in the background.
pub fn start(cfg: ClientConfig) -> Result<ClientHandle> {
    let engine = Engine::open(&cfg.data_dir, &*cfg.keystore)?;
    if engine.role() != Role::Client {
        bail!("this data dir is not paired as a client");
    }
    let token = cfg
        .keystore
        .get(DEVICE_TOKEN)?
        .context("device token missing from the key store; pair again")?;
    let token = SecretKey::from_slice(&token).context("device token is corrupt")?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let (notices, _) = broadcast::channel(256);
    let shared = Arc::new(Shared {
        engine: Arc::new(engine),
        token,
        http: http()?,
        notices,
        local: Notify::new(),
        live: Mutex::new(LiveState::default()),
        fetch_images: cfg.fetch_images,
        stop: stop_rx,
    });
    let task = tokio::spawn(run(shared.clone()));
    Ok(ClientHandle {
        shared,
        stop: stop_tx,
        task,
    })
}

impl ClientHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<SyncNotice> {
        self.shared.notices.subscribe()
    }

    /// Something was copied locally: push it now (else within ~1 s).
    pub fn notify_local_change(&self) {
        self.shared.local.notify_one();
    }

    pub fn status(&self) -> Result<SyncStatus> {
        let e = &self.shared.engine;
        let live = lock(&self.shared.live);
        Ok(SyncStatus {
            configured: true,
            role: Some(Role::Client),
            space_id: Some(e.space.space_id.clone()),
            device_id: e.space.device_id.clone(),
            device_name: Some(e.space.device_name.clone()),
            hub_name: Some(e.space.hub_name.clone()),
            running: true,
            connected: live.connected,
            port: None,
            hosts: live
                .host
                .clone()
                .into_iter()
                .chain(e.db.space()?.map(|s| s.hub_hosts).unwrap_or_default())
                .fold(Vec::new(), |mut v, h| {
                    if !v.contains(&h) {
                        v.push(h);
                    }
                    v
                }),
            devices: e
                .db
                .devices()?
                .into_iter()
                .map(|d| {
                    let online = d.id == e.space.hub_device_id && live.connected;
                    DeviceView::from_row(d, online)
                })
                .collect(),
            outbox: e.db.outbox_len()?,
            pending_images: e.db.pending_blob_count()?,
            last_seq: e.db.get_i64("last_seq")?,
            head_seq: 0,
            last_error: live.last_error.clone(),
            keystore: String::new(),
        })
    }

    pub fn is_unpaired(&self) -> bool {
        lock(&self.shared.live).unpaired
    }

    /// Download the full image of a synced image clip now. Ok(false) when
    /// it isn't available (yet).
    pub async fn fetch_image(&self, clip_id: &str) -> Result<bool> {
        let engine = self.shared.engine.clone();
        let id = clip_id.to_owned();
        let Some((blob_id, hash)) = blocking(move || {
            Ok(engine
                .pipeline
                .store()
                .content_hash(&id)?
                .map(|h| (engine.keys.blob_id(&h), h)))
        })
        .await?
        else {
            return Ok(false);
        };
        let host = lock(&self.shared.live)
            .host
            .clone()
            .context("not connected to the hub")?;
        fetch_blob(&self.shared, &host, &blob_id, &hash).await
    }

    /// Stop the loop (closes the stream; the outbox stays on disk).
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}

fn notice(sh: &Shared, n: SyncNotice) {
    let _ = sh.notices.send(n);
}

fn set_connected(sh: &Shared, connected: bool, host: Option<String>) {
    let mut live = lock(&sh.live);
    let changed = live.connected != connected;
    live.connected = connected;
    if host.is_some() {
        live.host = host;
    }
    if connected {
        live.last_error = None;
        live.unpaired = false;
        live.reached_stream = true;
    }
    drop(live);
    if changed {
        notice(sh, SyncNotice::StatusChanged);
    }
}

async fn run(sh: Arc<Shared>) {
    let mut stop = sh.stop.clone();
    let mut backoff = Duration::from_secs(1);
    loop {
        if *stop.borrow() {
            break;
        }
        lock(&sh.live).reached_stream = false;
        let res = session(&sh).await;
        set_connected(&sh, false, None);
        match res {
            Ok(()) => {}
            Err(e) => {
                let unpaired = e.is::<Unpaired>();
                let msg = format!("{e:#}");
                {
                    let mut live = lock(&sh.live);
                    if live.last_error.as_deref() != Some(&msg) {
                        tracing::warn!("sync: {msg}");
                    }
                    live.last_error = Some(msg);
                    live.unpaired = unpaired;
                }
                notice(&sh, SyncNotice::StatusChanged);
                if unpaired {
                    backoff = MAX_BACKOFF;
                }
            }
        }
        if *stop.borrow() {
            break;
        }
        // A session that got as far as the stream resets the backoff.
        let (reached, unpaired) = {
            let live = lock(&sh.live);
            (live.reached_stream, live.unpaired)
        };
        if reached && !unpaired {
            backoff = Duration::from_secs(1);
        }
        // Offline: keep turning local changes into outbox rows.
        let deadline = tokio::time::Instant::now() + backoff;
        loop {
            let _ = pump_local(&sh).await;
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                _ = sh.local.notified() => {}
                _ = crate::engine::stopped(&mut stop) => return,
            }
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Send a signed request.
async fn send(
    sh: &Shared,
    host: &str,
    method: Method,
    path_and_query: &str,
    body: Vec<u8>,
) -> Result<reqwest::Response> {
    let sig = auth::sign(
        sh.engine.device_id(),
        sh.token.as_bytes(),
        method.as_str(),
        path_and_query,
        now_millis(),
        &body,
    );
    let mut req = sh
        .http
        .request(method, format!("http://{host}{path_and_query}"))
        .header("authorization", sig);
    if !body.is_empty() {
        req = req.body(body);
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("hub at {host} unreachable"))?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        let msg = error_message(resp).await;
        if msg.contains("clock") {
            bail!("hub refused the request: {msg} (check the clocks of both devices)");
        }
        return Err(Unpaired.into());
    }
    Ok(resp)
}

async fn expect_ok(resp: reqwest::Response, what: &str) -> Result<Vec<u8>> {
    if !resp.status().is_success() {
        bail!("{what} failed: {}", error_message(resp).await);
    }
    Ok(resp.bytes().await.context(what.to_owned())?.to_vec())
}

async fn pick_host(sh: &Shared) -> Result<String> {
    let engine = sh.engine.clone();
    let space = blocking(move || engine.db.space()?.context("sync not set up")).await?;
    for host in &space.hub_hosts {
        let Ok(resp) = sh
            .http
            .get(format!("http://{host}/v1/hello"))
            .timeout(Duration::from_secs(3))
            .send()
            .await
        else {
            continue;
        };
        let Ok(body) = expect_ok(resp, "hello").await else {
            continue;
        };
        let Ok(hello) = serde_json::from_slice::<crate::wire::Hello>(&body) else {
            continue;
        };
        if hello.space_id != space.space_id {
            bail!("the device at {host} is a different ClipTrail hub");
        }
        // Remember: working host first, then any new addresses of the hub.
        let mut hosts = vec![host.clone()];
        for h in hello.hosts.iter().chain(space.hub_hosts.iter()) {
            if !hosts.contains(h) {
                hosts.push(h.clone());
            }
        }
        hosts.truncate(8);
        if hosts != space.hub_hosts {
            let engine = sh.engine.clone();
            let mut s = space.clone();
            s.hub_hosts = hosts;
            blocking(move || engine.db.save_space(&s)).await?;
        }
        return Ok(host.clone());
    }
    bail!(
        "hub \"{}\" not reachable at {}",
        space.hub_name,
        space.hub_hosts.join(", ")
    )
}

async fn session(sh: &Arc<Shared>) -> Result<()> {
    let host = pick_host(sh).await?;
    pump_local(sh).await?;
    flush(sh, &host).await?;
    catch_up(sh, &host).await?;
    if let Err(e) = refresh_devices(sh, &host).await {
        if e.is::<Unpaired>() {
            return Err(e);
        }
        tracing::debug!("device list: {e:#}");
    }

    let engine = sh.engine.clone();
    let since = blocking(move || engine.db.get_i64("last_seq")).await?;
    let pq = format!("/v1/stream?since={since}");
    let sig = auth::sign(
        sh.engine.device_id(),
        sh.token.as_bytes(),
        "GET",
        &pq,
        now_millis(),
        b"",
    );
    let mut req = format!("ws://{host}{pq}").into_client_request()?;
    req.headers_mut()
        .insert("authorization", HeaderValue::from_str(&sig)?);
    let (ws, _) = match tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(req),
    )
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(resp))) if resp.status() == 401 => {
            return Err(Unpaired.into());
        }
        Ok(Err(e)) => bail!("stream connect failed: {e}"),
        Err(_) => bail!("stream connect timed out"),
    };
    set_connected(sh, true, Some(host.clone()));
    tracing::info!(%host, since, "connected to hub");
    if sh.fetch_images {
        let _ = fetch_due_blobs(sh, &host).await;
    }

    let (mut sink, mut stream) = ws.split();
    let mut stop = sh.stop.clone();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut blobs = tokio::time::interval(Duration::from_secs(5));
    let mut devs = tokio::time::interval(Duration::from_secs(60));
    let mut last_rx = Instant::now();
    loop {
        tokio::select! {
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    last_rx = Instant::now();
                    let frame: crate::wire::StreamFrame = serde_json::from_str(&t)
                        .context("malformed stream frame")?;
                    if frame.t == "event" {
                        apply(sh, vec![frame.ev], &host).await?;
                    }
                }
                Some(Ok(Message::Close(Some(f)))) if u16::from(f.code) == 4001 => {
                    return Err(Unpaired.into());
                }
                Some(Ok(Message::Close(_))) | None => bail!("hub closed the connection"),
                Some(Ok(_)) => last_rx = Instant::now(),
                Some(Err(e)) => bail!("stream error: {e}"),
            },
            _ = sh.local.notified() => {
                pump_local(sh).await?;
                flush(sh, &host).await?;
            }
            _ = tick.tick() => {
                pump_local(sh).await?;
                flush(sh, &host).await?;
                if last_rx.elapsed() > STREAM_IDLE {
                    bail!("hub stopped responding");
                }
            }
            _ = blobs.tick() => {
                if sh.fetch_images {
                    fetch_due_blobs(sh, &host).await?;
                }
            }
            _ = devs.tick() => {
                refresh_devices(sh, &host).await?;
            }
            _ = crate::engine::stopped(&mut stop) => {
                let _ = sink.send(Message::Close(None)).await;
                return Ok(());
            }
        }
    }
}

/// Journal → sealed outbox rows.
async fn pump_local(sh: &Shared) -> Result<usize> {
    let mut total = 0;
    loop {
        let engine = sh.engine.clone();
        let n = blocking(move || {
            let (events, cursor) = engine.collect_local()?;
            for ev in &events {
                client_enqueue(&engine, ev)?;
            }
            engine.commit_cursor(cursor)?;
            Ok(events.len())
        })
        .await?;
        total += n;
        if n < JOURNAL_BATCH as usize {
            return Ok(total);
        }
    }
}

/// Push the outbox (uploading image blobs first).
async fn flush(sh: &Shared, host: &str) -> Result<()> {
    loop {
        let engine = sh.engine.clone();
        let rows = blocking(move || engine.db.outbox(OUTBOX_BATCH)).await?;
        if rows.is_empty() {
            return Ok(());
        }
        for row in &rows {
            if let Some((blob_id, hash)) = &row.blob {
                let (engine, b, h) = (sh.engine.clone(), blob_id.clone(), hash.clone());
                let sealed = blocking(move || engine.seal_blob(&b, &h)).await?;
                if let Some(sealed) = sealed {
                    let n = sealed.len();
                    let resp = send(
                        sh,
                        host,
                        Method::PUT,
                        &format!("/v1/blobs/{blob_id}"),
                        sealed,
                    )
                    .await?;
                    expect_ok(resp, "image upload").await?;
                    tracing::debug!(blob = %blob_id, bytes = n, "image uploaded");
                }
            }
        }
        let envs: Vec<Envelope> = rows.iter().map(|r| r.env.clone()).collect();
        let n = envs.len();
        let body = serde_json::to_vec(&crate::wire::PushRequest { events: envs })?;
        let resp = send(sh, host, Method::POST, "/v1/events", body).await?;
        let body = expect_ok(resp, "push").await?;
        let pr: crate::wire::PushResponse =
            serde_json::from_slice(&body).context("malformed push reply")?;
        for r in &pr.rejected {
            tracing::warn!(event = %r.event_id, reason = %r.reason, "hub rejected an event");
        }
        let mut done = pr.accepted;
        done.extend(pr.rejected.into_iter().map(|r| r.event_id));
        let engine = sh.engine.clone();
        blocking(move || engine.db.remove_outbox(&done)).await?;
        tracing::debug!(events = n, "pushed local events");
        if n < OUTBOX_BATCH as usize {
            return Ok(());
        }
    }
}

async fn catch_up(sh: &Arc<Shared>, host: &str) -> Result<()> {
    loop {
        let engine = sh.engine.clone();
        let since = blocking(move || engine.db.get_i64("last_seq")).await?;
        let resp = send(
            sh,
            host,
            Method::GET,
            &format!("/v1/events?since={since}&limit=200"),
            Vec::new(),
        )
        .await?;
        let body = expect_ok(resp, "catch-up").await?;
        let page: crate::wire::EventsPage =
            serde_json::from_slice(&body).context("malformed events page")?;
        if page.events.is_empty() {
            return Ok(());
        }
        let last = page.events.last().map_or(since, |e| e.seq);
        apply(sh, page.events, host).await?;
        if last >= page.head_seq {
            return Ok(());
        }
    }
}

/// Apply hub envelopes in seq order and advance the cursor.
async fn apply(sh: &Arc<Shared>, envs: Vec<SeqEnvelope>, host: &str) -> Result<()> {
    let engine = sh.engine.clone();
    let (metas, changed, blobs) = blocking(move || {
        let mut last = engine.db.get_i64("last_seq")?;
        let mut metas = Vec::new();
        let mut changed = 0usize;
        let mut blobs = Vec::new();
        for e in envs {
            if e.seq <= last {
                continue;
            }
            match engine.apply_envelope(&e.env) {
                Ok((kind, applied)) => {
                    if kind != "own" {
                        if applied.changed {
                            changed += 1;
                        }
                        metas.push(SyncNotice::Event {
                            seq: e.seq,
                            kind: kind.to_owned(),
                            device_id: e.env.device_id.clone(),
                            event_id: e.env.event_id.clone(),
                            applied: applied.changed,
                        });
                    }
                    blobs.extend(applied.blob);
                }
                // A bad event must not wedge the stream: skip it.
                Err(err) => {
                    tracing::warn!(seq = e.seq, event = %e.env.event_id, "skipping event: {err:#}")
                }
            }
            last = e.seq;
            engine.db.set_i64("last_seq", last)?;
        }
        Ok((metas, changed, blobs))
    })
    .await?;
    for m in metas {
        notice(sh, m);
    }
    if changed > 0 {
        notice(sh, SyncNotice::ClipsChanged { count: changed });
    }
    if sh.fetch_images && !blobs.is_empty() {
        let (sh2, host) = (sh.clone(), host.to_owned());
        tokio::spawn(async move {
            for (blob_id, hash) in blobs {
                if let Err(e) = fetch_blob(&sh2, &host, &blob_id, &hash).await {
                    tracing::debug!(blob = %blob_id, "image fetch deferred: {e:#}");
                }
            }
        });
    }
    Ok(())
}

async fn refresh_devices(sh: &Shared, host: &str) -> Result<()> {
    let resp = send(sh, host, Method::GET, "/v1/devices", Vec::new()).await?;
    let body = expect_ok(resp, "device list").await?;
    let list: Vec<DeviceView> = serde_json::from_slice(&body).context("malformed devices")?;
    let me = sh.engine.device_id().to_owned();
    let rows: Vec<DeviceRow> = list
        .into_iter()
        .map(|d| DeviceRow {
            is_self: d.id == me,
            id: d.id,
            name: d.name,
            platform: d.platform,
            paired_at: d.paired_at,
            revoked_at: d.revoked_at,
            last_seen: d.last_seen,
        })
        .collect();
    let engine = sh.engine.clone();
    let changed = blocking(move || {
        let before = engine.db.devices()?;
        let differs = before.len() != rows.len()
            || before
                .iter()
                .zip(&rows)
                .any(|(a, b)| a.id != b.id || a.name != b.name);
        engine.db.replace_devices(&rows)?;
        Ok(differs)
    })
    .await?;
    if changed {
        notice(sh, SyncNotice::StatusChanged);
    }
    Ok(())
}

async fn fetch_due_blobs(sh: &Shared, host: &str) -> Result<()> {
    let engine = sh.engine.clone();
    let due = blocking(move || engine.db.due_pending_blobs(now_millis(), 4)).await?;
    for (blob_id, hash, attempts) in due {
        match fetch_blob(sh, host, &blob_id, &hash).await {
            Ok(_) => {}
            Err(e) if e.is::<Unpaired>() => return Err(e),
            Err(e) => {
                tracing::debug!(blob = %blob_id, attempts, "image fetch failed: {e:#}");
                let engine = sh.engine.clone();
                blocking(move || {
                    if attempts + 1 >= BLOB_GIVE_UP {
                        engine.db.remove_pending_blob(&blob_id)
                    } else {
                        let wait: Millis = (5_000i64 << attempts.min(10)).min(3_600_000);
                        engine.db.retry_pending_blob(&blob_id, now_millis() + wait)
                    }
                })
                .await?;
            }
        }
    }
    Ok(())
}

/// GET one sealed image and attach it. Ok(false) when the clip is gone.
async fn fetch_blob(sh: &Shared, host: &str, blob_id: &str, hash: &[u8]) -> Result<bool> {
    let resp = send(
        sh,
        host,
        Method::GET,
        &format!("/v1/blobs/{blob_id}"),
        Vec::new(),
    )
    .await?;
    let body = expect_ok(resp, "image download").await?;
    let n = body.len();
    let (engine, b, h) = (sh.engine.clone(), blob_id.to_owned(), hash.to_vec());
    let attached = blocking(move || {
        let ok = engine.attach_blob(&b, &h, &body)?;
        engine.db.remove_pending_blob(&b)?;
        Ok(ok)
    })
    .await?;
    if attached {
        tracing::debug!(blob = %blob_id, bytes = n, "image downloaded");
        notice(sh, SyncNotice::ClipsChanged { count: 1 });
    }
    Ok(attached)
}
