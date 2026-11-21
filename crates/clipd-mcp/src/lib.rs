//! ClipTrail MCP server over the local store (stdio).
//!
//! One JSON-RPC message per line on stdin; responses on stdout. All logging
//! goes to stderr — stdout is protocol-only. Used by the `clipd-mcp` binary
//! and by the desktop app's `cliptrail --mcp` mode (what the one-click
//! "Connect" in Settings → AI assistants points assistants at).

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::Result;
use clipd_core::Store;

mod protocol;
mod tools;

const DB_FILE: &str = "clips.db";
/// Which AI apps started this server, and when (their MCP `clientInfo.name`
/// → ms since epoch). Shown in Settings as proof an assistant really uses it.
/// Holds app names only, never anything from the history.
const CLIENTS_FILE: &str = "mcp-clients.json";
/// The app's own "does it answer?" check on Connect; not an assistant.
pub const SELF_TEST_CLIENT: &str = "cliptrail-self-test";

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
        note_client(data_dir, &line);
        if let Some(resp) = protocol::handle(&store, &line) {
            serde_json::to_writer(&mut out, &resp)?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Record the assistant named in an `initialize` request (best effort).
fn note_client(data_dir: &Path, line: &str) {
    let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    if msg.get("method").and_then(|m| m.as_str()) != Some("initialize") {
        return;
    }
    let Some(name) = msg
        .pointer("/params/clientInfo/name")
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty() && *n != SELF_TEST_CLIENT)
    else {
        return;
    };
    let mut seen = recent_clients(data_dir);
    seen.insert(name.chars().take(80).collect(), clipd_core::now_millis());
    if let Ok(json) = serde_json::to_vec_pretty(&seen) {
        let _ = std::fs::write(data_dir.join(CLIENTS_FILE), json);
    }
}

/// The AI apps that started this server: MCP client name → last time (ms).
pub fn recent_clients(data_dir: &Path) -> BTreeMap<String, i64> {
    std::fs::read(data_dir.join(CLIENTS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_records_the_client_but_not_the_self_test() {
        let dir = std::env::temp_dir().join(format!("mcp-clients-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let init = |name: &str| {
            format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"clientInfo":{{"name":"{name}","version":"1"}}}}}}"#
            )
        };
        note_client(&dir, &init("claude-ai"));
        note_client(&dir, &init(SELF_TEST_CLIENT));
        note_client(&dir, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let seen = recent_clients(&dir);
        assert_eq!(seen.keys().collect::<Vec<_>>(), ["claude-ai"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
