//! ClipTrail MCP server over the local store (stdio).
//!
//! One JSON-RPC message per line on stdin; responses on stdout. All logging
//! goes to stderr — stdout is protocol-only. Used by the `clipd-mcp` binary
//! and by the desktop app's `cliptrail --mcp` mode (what the one-click
//! "Connect" in Settings → AI assistants points assistants at).

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::Result;
use clipd_core::Store;

mod protocol;
mod tools;

const DB_FILE: &str = "clips.db";

/// Unlock `data_dir`, then answer MCP requests from stdin until it closes.
pub fn serve(data_dir: &Path) -> Result<()> {
    let device_id = clipd_sync::local_device_id(data_dir)?;
    let ks = clipd_sync::default_keystore(data_dir, &device_id);
    clipd_sync::unlock_data_dir(data_dir, ks.as_ref())?;
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
