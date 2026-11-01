# clipd-mcp

An [MCP](https://modelcontextprotocol.io) server that lets AI agents (Claude Code, Claude Desktop, Cursor, Devin, …) search your ClipTrail clipboard history. "Find that connection string I copied in March" becomes a tool call instead of a manual dig.

- **Stdio transport** — the agent spawns `clipd-mcp` and talks JSON-RPC over stdin/stdout.
- **Read-mostly** — `search_clips`, `recent_clips`, `get_clip`, `clip_stats`, `list_source_apps`, `day_counts`, plus `pin_clip`. No delete/edit/copy-to-clipboard.
- **Privacy boundary stays on-device** — clips are served only to the local agent. Masked (sensitive) clips never expose their body: `get_clip` returns metadata + `••••••`.

## Install

```sh
cargo install --path crates/clipd-mcp   # or cargo build --release -p clipd-mcp
```

The server reads `clips.db` from the platform data dir (`%APPDATA%\ClipTrail`, `~/Library/Application Support/ClipTrail`, `~/.local/share/ClipTrail`). Override with `--data-dir` or `CLIPTRAIL_DATA_DIR`.

## Configure your agent

### Claude Code

```sh
claude mcp add cliptrail -- clipd-mcp
```

### Claude Desktop / Cursor (`mcpServers` config)

```json
{
  "mcpServers": {
    "cliptrail": {
      "command": "clipd-mcp",
      "args": []
    }
  }
}
```

### ChatGPT / remote agents

ChatGPT connectors call a **remote** MCP endpoint over HTTPS+OAuth — they cannot run a local stdio binary. ClipTrail's data is end-to-end encrypted and lives on your devices, so a remote endpoint would need a hosted MCP service that a paired device serves content to. Not planned for v1 — and arguably undesirable, since it would pipe plaintext clips into a cloud chat. The local story is the right boundary.

## Example questions your agent can now answer

- "find the Postgres connection string I copied in March" → `search_clips` + date filter
- "what code did I copy from VS Code yesterday" → `search_clips(kind=code, source_app=code, from=...)`
- "pin the clip I copied an hour ago" → `recent_clips` → `pin_clip`
- "how much did I copy this week" → `clip_stats` / `day_counts`

## Testing

```sh
cargo test -p clipd-mcp
# Seed a data dir, then run the real-MCP-client check (requires `pip install mcp`):
cargo run -p clipd-mcp --example seed -- /tmp/mcpdata
python3 crates/clipd-mcp/tests/mcp_client.py ./target/debug/clipd-mcp /tmp/mcpdata
```
