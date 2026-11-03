//! clipd-mcp: MCP stdio server over the local ClipTrail store.
//!
//! One JSON-RPC message per line on stdin; responses on stdout. All logging
//! goes to stderr — stdout is protocol-only.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use clipd_core::{Store, default_data_dir};

mod protocol;
mod tools;

const DB_FILE: &str = "clips.db";

#[derive(Parser)]
#[command(name = "clipd-mcp", about = "ClipTrail MCP server (stdio)")]
struct Cli {
    /// Data directory holding clips.db (default: platform data dir,
    /// override with CLIPTRAIL_DATA_DIR).
    #[arg(long, env = "CLIPTRAIL_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "clipd_mcp=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    let data_dir = cli.data_dir.unwrap_or_else(default_data_dir);
    let device_id = clipd_sync::local_device_id(&data_dir)?;
    let ks = clipd_sync::default_keystore(&data_dir, &device_id);
    clipd_sync::unlock_data_dir(&data_dir, ks.as_ref())?;
    let store = Store::open(&data_dir.join(DB_FILE))?;

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = protocol::handle(&store, &line) {
            serde_json::to_writer(&mut out, &resp)?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}
