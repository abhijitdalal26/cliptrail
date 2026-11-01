//! `clipd sync ...`: run a hub, pair, run a client, manage devices.
//! Prints ids, kinds and device names only, never clip content.

use std::collections::HashMap;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use clipd_core::now_millis;
use clipd_sync::{
    ClientConfig, HubConfig, KeyStore, SyncNotice, SyncStatus, client, default_keystore, hub,
};
use tokio::io::AsyncBufReadExt;

use crate::{clock, when};

#[derive(Subcommand)]
pub enum SyncCommand {
    /// Run this data dir as the sync hub and show a pairing QR code
    /// (press Enter for a new code, Ctrl+C to stop).
    Host {
        /// TCP port to listen on.
        #[arg(long, default_value_t = clipd_sync::DEFAULT_PORT)]
        port: u16,
        /// Address to put in the QR instead of this PC's LAN IPs
        /// (repeatable), e.g. 127.0.0.1 for a test on one machine.
        #[arg(long, value_name = "HOST")]
        advertise: Vec<String>,
        /// Listen on 127.0.0.1 only.
        #[arg(long)]
        loopback: bool,
        /// Name shown on paired devices.
        #[arg(long)]
        name: Option<String>,
        /// Don't print a pairing QR at start.
        #[arg(long)]
        no_pair: bool,
    },
    /// Pair this data dir as a client of a hub.
    Pair {
        /// The cliptrail://pair?... link from the hub.
        uri: String,
        /// Name shown on the hub.
        #[arg(long)]
        name: Option<String>,
    },
    /// Run the client loop (Ctrl+C to stop). Prints event metadata only.
    Run {
        /// Only sync thumbnails; skip full image downloads.
        #[arg(long)]
        no_images: bool,
    },
    /// Paired devices.
    Devices {
        #[arg(long)]
        json: bool,
    },
    /// Revoke a device (hub only): its token stops working at once.
    Revoke { device_id: String },
    /// Sync state of this data dir.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Forget sync for this data dir (keys, cursors, device list). Clips stay.
    Reset,
    /// Off-LAN sync through a self-hosted relay (clipd-relay).
    Relay {
        #[command(subcommand)]
        cmd: RelayCommand,
    },
}

#[derive(Subcommand)]
pub enum RelayCommand {
    /// Register this space on a relay and share it with your other devices
    /// (they pick it up the next time they sync with the hub).
    Add {
        /// e.g. https://relay.example.com (http:// only for localhost/LAN).
        url: String,
        /// The relay operator's invite code, if it asks for one.
        #[arg(long, env = "CLIPTRAIL_RELAY_INVITE", hide_env_values = true)]
        invite: Option<String>,
    },
    /// Stop syncing through a relay (on every device).
    Remove {
        url: String,
        /// Also delete this space's data from the relay.
        #[arg(long)]
        purge: bool,
    },
    /// Relays of this space and their state.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Check that a URL is a working ClipTrail relay (changes nothing).
    Check { url: String },
}

fn keystore(data_dir: &Path) -> Result<Arc<dyn KeyStore>> {
    let id = clipd_sync::local_device_id(data_dir)?;
    Ok(default_keystore(data_dir, &id))
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("start async runtime")
}

pub fn run(data_dir: PathBuf, cmd: SyncCommand) -> Result<()> {
    let ks = keystore(&data_dir)?;
    match cmd {
        SyncCommand::Host {
            port,
            advertise,
            loopback,
            name,
            no_pair,
        } => runtime()?.block_on(host(data_dir, ks, port, advertise, loopback, name, no_pair)),
        SyncCommand::Pair { uri, name } => runtime()?.block_on(async {
            let name = name.unwrap_or_else(clipd_sync::default_device_name);
            let out = client::pair(&data_dir, ks, &uri, &name, clipd_sync::platform()).await?;
            println!("paired with \"{}\" at {}", out.hub_name, out.host);
            println!("check code: {} (should match the hub)", out.check_code);
            println!(
                "queued {} existing clip{} to share; run `clipd sync run` to start syncing",
                out.backfilled,
                if out.backfilled == 1 { "" } else { "s" }
            );
            Ok(())
        }),
        SyncCommand::Run { no_images } => runtime()?.block_on(run_client(data_dir, ks, !no_images)),
        SyncCommand::Devices { json } => {
            let st = clipd_sync::status(&data_dir, &*ks)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&st.devices)?);
            } else {
                print_devices(&st);
            }
            Ok(())
        }
        SyncCommand::Revoke { device_id } => {
            if clipd_sync::revoke(&data_dir, &device_id)? {
                println!("revoked {device_id}");
                Ok(())
            } else {
                bail!("no active paired device with id {device_id}")
            }
        }
        SyncCommand::Status { json } => {
            let st = clipd_sync::status(&data_dir, &*ks)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&st)?);
            } else {
                print_status(&st);
            }
            Ok(())
        }
        SyncCommand::Reset => {
            clipd_sync::reset(&data_dir, &*ks)?;
            println!("sync reset; clips were kept");
            Ok(())
        }
        SyncCommand::Relay { cmd } => relay(data_dir, ks, cmd),
    }
}

fn relay(data_dir: PathBuf, ks: Arc<dyn KeyStore>, cmd: RelayCommand) -> Result<()> {
    match cmd {
        RelayCommand::Add { url, invite } => runtime()?.block_on(async {
            let v = clipd_sync::relay_add(&data_dir, ks, &url, invite.as_deref()).await?;
            println!("relay added: {}", v.url);
            println!(
                "your other devices learn it the next time they sync with the hub; \
                 a running `clipd sync host`/`run` picks it up within a few seconds"
            );
            Ok(())
        }),
        RelayCommand::Remove { url, purge } => runtime()?.block_on(async {
            if clipd_sync::relay_remove(&data_dir, ks, &url, purge).await? {
                println!(
                    "relay removed{}",
                    if purge { " (and its data deleted)" } else { "" }
                );
                Ok(())
            } else {
                bail!("{url} is not one of this space's relays")
            }
        }),
        RelayCommand::Status { json } => {
            let st = clipd_sync::status(&data_dir, &*ks)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&st.relays)?);
            } else {
                print_relays(&st);
            }
            Ok(())
        }
        RelayCommand::Check { url } => runtime()?.block_on(async {
            let h = clipd_sync::relay_check(&url).await?;
            println!(
                "ClipTrail relay v{} (protocol {}), registration {}",
                h.version, h.proto, h.registration
            );
            let l = &h.limits;
            println!(
                "limits: {} MiB per space, images up to {} MiB, {} devices, keeps {} days",
                l.max_space_bytes >> 20,
                l.max_blob_bytes >> 20,
                l.max_devices,
                if l.retention_days == 0 {
                    "forever".to_owned()
                } else {
                    l.retention_days.to_string()
                }
            );
            Ok(())
        }),
    }
}

fn print_relays(st: &SyncStatus) {
    if st.relays.is_empty() {
        println!("no relays (sync works on the local network only)");
        return;
    }
    println!("relays:");
    for r in &st.relays {
        let seen = r
            .last_ok
            .map(|t| format!("last ok {}", when(t)))
            .unwrap_or_else(|| "never reached".into());
        println!(
            "  {:<40} {:<10} {}{}",
            r.url,
            r.state,
            if r.enrolled { "" } else { "not enrolled yet, " },
            seen
        );
        if let Some(e) = &r.last_error {
            println!("      last problem: {e}");
        }
    }
}

fn print_status(st: &SyncStatus) {
    if !st.configured {
        println!("sync: not set up (device {})", st.device_id);
        println!("  run `clipd sync host` here, or `clipd sync pair <link>` to join a hub");
        return;
    }
    let role = match st.role {
        Some(clipd_sync::Role::Hub) => "hub",
        _ => "client",
    };
    println!(
        "sync:      {role} of space {}",
        st.space_id.as_deref().unwrap_or("?")
    );
    println!(
        "device:    {} ({})",
        st.device_name.as_deref().unwrap_or("?"),
        st.device_id
    );
    if role == "client" {
        println!(
            "hub:       {} at {}",
            st.hub_name.as_deref().unwrap_or("?"),
            st.hosts.join(", ")
        );
        println!("cursor:    seq {}", st.last_seq);
        println!("outbox:    {} event(s) waiting", st.outbox);
    } else {
        println!("log head:  seq {}", st.head_seq);
        if let Some(p) = st.port {
            println!("port:      {p}");
        }
    }
    println!("images:    {} pending", st.pending_images);
    println!("keys in:   {}", st.keystore);
    if let Some(e) = &st.last_error {
        println!("problem:   {e}");
    }
    print_devices(st);
    print_relays(st);
}

fn print_devices(st: &SyncStatus) {
    if st.devices.is_empty() {
        println!("no devices");
        return;
    }
    println!("devices:");
    for d in &st.devices {
        let state = if d.is_self {
            "this device".to_owned()
        } else if let Some(r) = d.revoked_at {
            format!("revoked {}", when(r))
        } else {
            d.last_seen
                .map(|t| format!("seen {}", when(t)))
                .unwrap_or_else(|| "never seen".into())
        };
        println!("  {}  {:<20} {:<8} {}", d.id, d.name, d.platform, state);
    }
}

/// Print relay state changes (connected / standby / offline / denied).
fn report_relays(st: &SyncStatus, seen: &mut HashMap<String, String>) {
    for r in &st.relays {
        if seen.get(&r.url) != Some(&r.state) {
            let why = match (&r.state[..], &r.last_error) {
                ("offline" | "denied", Some(e)) => format!("  ({e})"),
                _ => String::new(),
            };
            println!(
                "{}  relay    {} {}{why}",
                clock(now_millis()),
                r.url,
                r.state
            );
            seen.insert(r.url.clone(), r.state.clone());
        }
    }
}

fn print_offer(offer: &hub::OfferView) {
    println!();
    println!("{}", hub::qr_terminal(&offer.uri));
    println!("pairing link (valid 10 min, single use):");
    println!("  {}", offer.uri);
    println!("  addresses: {}", offer.hosts.join(", "));
    println!("press Enter for a new code, Ctrl+C to stop");
    let _ = std::io::stdout().flush();
}

fn names(data_dir: &Path) -> HashMap<String, String> {
    clipd_sync::device_names(data_dir)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn print_notice(data_dir: &Path, n: &SyncNotice) {
    let now = clock(now_millis());
    match n {
        SyncNotice::Paired {
            device_id,
            name,
            platform,
            check_code,
        } => println!("{now}  paired   {name} ({platform}) {device_id}  check code {check_code}"),
        SyncNotice::Event {
            seq,
            kind,
            device_id,
            event_id,
            applied,
        } => {
            let from = names(data_dir)
                .get(device_id)
                .cloned()
                .unwrap_or_else(|| device_id.clone());
            println!(
                "{now}  seq {seq:<5} {kind:<11} from {from:<16} {event_id}{}",
                if *applied { "" } else { "  (no change)" }
            );
        }
        SyncNotice::ClipsChanged { count } => println!("{now}  applied  {count} change(s)"),
        SyncNotice::StatusChanged => {}
    }
    let _ = std::io::stdout().flush();
}

async fn host(
    data_dir: PathBuf,
    ks: Arc<dyn KeyStore>,
    port: u16,
    advertise: Vec<String>,
    loopback: bool,
    name: Option<String>,
    no_pair: bool,
) -> Result<()> {
    let mut cfg = HubConfig::new(data_dir.clone(), ks);
    cfg.port = port;
    if loopback {
        cfg.bind = IpAddr::V4(Ipv4Addr::LOCALHOST);
    }
    if !advertise.is_empty() {
        cfg.advertise = Some(advertise);
    }
    if let Some(n) = name {
        cfg.name = n;
    }
    let h = hub::start(cfg).await?;
    let st = h.status()?;
    println!(
        "hub \"{}\" listening on port {} (space {})",
        st.hub_name.as_deref().unwrap_or("?"),
        h.port(),
        h.space_id()
    );
    if !loopback {
        println!(
            "Windows may ask to allow ClipTrail through the firewall: allow it on Private networks."
        );
    }
    if !no_pair {
        print_offer(&h.create_offer());
    }
    let mut notices = h.subscribe();
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut stdin_open = true;
    let mut last_online = st.connected;
    let mut relay_states: HashMap<String, String> = HashMap::new();
    loop {
        tokio::select! {
            n = notices.recv() => match n {
                Ok(SyncNotice::StatusChanged) => {
                    if let Ok(s) = h.status() {
                        let online: Vec<&str> = s.devices.iter()
                            .filter(|d| d.online && !d.is_self)
                            .map(|d| d.name.as_str())
                            .collect();
                        if s.connected != last_online || !online.is_empty() {
                            println!("{}  online   {}", clock(now_millis()),
                                if online.is_empty() { "(none)".to_owned() } else { online.join(", ") });
                        }
                        last_online = s.connected;
                        report_relays(&s, &mut relay_states);
                    }
                }
                Ok(n) => print_notice(&data_dir, &n),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            },
            line = stdin.next_line(), if stdin_open => match line {
                Ok(Some(_)) => print_offer(&h.create_offer()),
                _ => stdin_open = false,
            },
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    h.shutdown().await;
    eprintln!("hub stopped");
    Ok(())
}

async fn run_client(data_dir: PathBuf, ks: Arc<dyn KeyStore>, images: bool) -> Result<()> {
    let c = client::start(ClientConfig {
        data_dir: data_dir.clone(),
        keystore: ks,
        fetch_images: images,
    })?;
    let mut notices = c.subscribe();
    eprintln!("syncing {} (Ctrl+C to stop)", data_dir.display());
    let mut was_connected = false;
    let mut last_err: Option<String> = None;
    let mut relay_states: HashMap<String, String> = HashMap::new();
    loop {
        tokio::select! {
            n = notices.recv() => match n {
                Ok(SyncNotice::StatusChanged) => {
                    if let Ok(s) = c.status() {
                        if s.connected != was_connected {
                            if s.connected {
                                println!("{}  connected to {} ({})", clock(now_millis()),
                                    s.hub_name.as_deref().unwrap_or("hub"),
                                    s.hosts.first().map(String::as_str).unwrap_or("?"));
                            } else {
                                println!("{}  disconnected", clock(now_millis()));
                            }
                            was_connected = s.connected;
                        }
                        if s.last_error != last_err {
                            if let Some(e) = &s.last_error {
                                println!("{}  offline  {e}", clock(now_millis()));
                            }
                            last_err = s.last_error.clone();
                        }
                        report_relays(&s, &mut relay_states);
                    }
                }
                Ok(n) => print_notice(&data_dir, &n),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            },
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    c.shutdown().await;
    eprintln!("stopped");
    Ok(())
}
