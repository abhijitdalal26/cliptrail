//! `clipd-relay` binary. Every flag also reads an environment variable
//! (`CLIPD_RELAY_*`), which is how the Docker image is configured.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::Parser;
use clipd_relay::{Registration, RelayConfig};

#[derive(Parser, Debug)]
#[command(
    name = "clipd-relay",
    version,
    about = "ClipTrail relay: stores only end-to-end-encrypted envelopes for off-LAN sync"
)]
struct Args {
    /// Address to listen on (put a TLS proxy such as Caddy in front of it).
    #[arg(long, env = "CLIPD_RELAY_BIND", default_value = "127.0.0.1:8787")]
    bind: SocketAddr,
    /// Where relay.db lives.
    #[arg(long, env = "CLIPD_RELAY_DATA", default_value = "./relay-data")]
    data_dir: PathBuf,
    /// Public https:// URL devices use (informational, shown in logs).
    #[arg(long, env = "CLIPD_RELAY_PUBLIC_URL")]
    public_url: Option<String>,
    /// Storage quota per space in MiB (oldest data is evicted beyond it).
    #[arg(long, env = "CLIPD_RELAY_MAX_SPACE_MB", default_value_t = 256)]
    max_space_mb: u64,
    /// Largest sealed image accepted, in MiB.
    #[arg(long, env = "CLIPD_RELAY_MAX_BLOB_MB", default_value_t = 48)]
    max_blob_mb: u64,
    /// How many spaces this relay hosts at most.
    #[arg(long, env = "CLIPD_RELAY_MAX_SPACES", default_value_t = 100)]
    max_spaces: u64,
    /// Devices per space at most.
    #[arg(long, env = "CLIPD_RELAY_MAX_DEVICES", default_value_t = 16)]
    max_devices: u32,
    /// Drop events older than this many days (0 = never).
    #[arg(long, env = "CLIPD_RELAY_RETENTION_DAYS", default_value_t = 30)]
    retention_days: u32,
    /// Require this invite code to register a new space (recommended for
    /// any relay reachable from the internet).
    #[arg(long, env = "CLIPD_RELAY_INVITE", hide_env_values = true)]
    invite: Option<String>,
    /// Refuse new spaces entirely (existing ones keep working).
    #[arg(long, env = "CLIPD_RELAY_CLOSED")]
    closed: bool,
    /// Trust X-Forwarded-For for rate limiting (only behind your own proxy).
    #[arg(long, env = "CLIPD_RELAY_TRUST_PROXY")]
    trust_proxy: bool,
    /// Requests per device per minute.
    #[arg(long, env = "CLIPD_RELAY_DEVICE_RATE", default_value_t = 1200)]
    device_rate: u32,
    /// Unauthenticated or failed requests per IP per minute.
    #[arg(long, env = "CLIPD_RELAY_IP_RATE", default_value_t = 60)]
    ip_rate: u32,
    /// Check that a relay answers on --bind and exit (Docker HEALTHCHECK).
    #[arg(long)]
    healthcheck: bool,
}

fn healthcheck(addr: SocketAddr) -> Result<()> {
    let addr = if addr.ip().is_unspecified() {
        SocketAddr::from(([127, 0, 0, 1], addr.port()))
    } else {
        addr
    };
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
    s.set_read_timeout(Some(Duration::from_secs(3)))?;
    s.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut buf = String::new();
    s.read_to_string(&mut buf)?;
    if buf.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        bail!("relay unhealthy")
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.healthcheck {
        return healthcheck(args.bind);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CLIPD_RELAY_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut cfg = RelayConfig::new(args.bind, args.data_dir);
    cfg.public_url = args.public_url;
    cfg.max_space_bytes = args.max_space_mb.max(1) * 1024 * 1024;
    cfg.max_blob_bytes = args.max_blob_mb.max(1) * 1024 * 1024;
    cfg.max_spaces = args.max_spaces;
    cfg.max_devices = args.max_devices.max(1);
    cfg.retention_days = args.retention_days;
    cfg.trust_proxy = args.trust_proxy;
    cfg.device_rate_per_min = args.device_rate.max(10);
    cfg.ip_rate_per_min = args.ip_rate.max(5);
    cfg.registration = match (args.closed, args.invite.filter(|i| !i.trim().is_empty())) {
        (true, _) => Registration::Closed,
        (false, Some(code)) => Registration::Invite(code.trim().to_owned()),
        (false, None) => Registration::Open,
    };
    if cfg.registration == Registration::Open && !args.bind.ip().is_loopback() {
        tracing::warn!(
            "registration is open: anyone who can reach this relay can create a space; set CLIPD_RELAY_INVITE"
        );
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let h = clipd_relay::start(cfg).await?;
        shutdown_signal().await;
        tracing::info!("shutting down");
        h.shutdown().await;
        Ok(())
    })
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async {
                match term.as_mut() {
                    Some(t) => { t.recv().await; }
                    None => std::future::pending::<()>().await,
                }
            } => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
