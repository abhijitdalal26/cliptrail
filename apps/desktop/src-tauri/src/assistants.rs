//! Settings → AI assistants: one-click setup of ClipTrail's MCP server in the
//! AI apps on this computer.
//!
//! Each assistant keeps its MCP servers in its own config file. "Connect"
//! adds a `cliptrail` entry that starts this executable with `--mcp` (see
//! `run_mcp`); "Disconnect" removes it. Nothing else in the file is touched
//! (top-level key order is kept), the previous file is saved next to it as
//! `<name>.cliptrail-backup`, and a file that can't be parsed (JSON with
//! comments, say) is left alone with an error pointing at "Copy config".
//!
//! The server is local: the assistant starts it on this computer and talks
//! to it over stdin/stdout. ClipTrail itself sends nothing anywhere.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{Value, json};

/// Name of our entry in every assistant's server list.
pub const SERVER_NAME: &str = "cliptrail";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    /// `{ "mcpServers": { name: { command, args } } }` (most apps).
    McpServers,
    /// VS Code: `{ "servers": { name: { type: "stdio", command, args } } }`.
    VsCode,
    /// Codex CLI: `[mcp_servers.<name>] command = …, args = […]` in TOML.
    CodexToml,
}

struct Client {
    id: &'static str,
    name: &'static str,
    format: Format,
    /// The config file (may not exist yet).
    config: fn() -> Option<PathBuf>,
    /// Something that exists once the app is installed / has been run.
    marker: fn() -> Option<PathBuf>,
}

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Where desktop apps keep per-user settings: %APPDATA% on Windows,
/// ~/Library/Application Support on macOS, ~/.config on Linux.
fn app_config() -> Option<PathBuf> {
    dirs::config_dir()
}

const CLIENTS: &[Client] = &[
    Client {
        id: "claude-desktop",
        name: "Claude Desktop",
        format: Format::McpServers,
        config: || {
            Some(
                app_config()?
                    .join("Claude")
                    .join("claude_desktop_config.json"),
            )
        },
        marker: || Some(app_config()?.join("Claude")),
    },
    Client {
        id: "claude-code",
        name: "Claude Code",
        format: Format::McpServers,
        config: || Some(home()?.join(".claude.json")),
        marker: || Some(home()?.join(".claude")),
    },
    Client {
        id: "cursor",
        name: "Cursor",
        format: Format::McpServers,
        config: || Some(home()?.join(".cursor").join("mcp.json")),
        marker: || Some(home()?.join(".cursor")),
    },
    Client {
        id: "vscode",
        name: "VS Code (GitHub Copilot)",
        format: Format::VsCode,
        config: || Some(app_config()?.join("Code").join("User").join("mcp.json")),
        marker: || Some(app_config()?.join("Code").join("User")),
    },
    Client {
        id: "windsurf",
        name: "Windsurf",
        format: Format::McpServers,
        config: || {
            Some(
                home()?
                    .join(".codeium")
                    .join("windsurf")
                    .join("mcp_config.json"),
            )
        },
        marker: || Some(home()?.join(".codeium").join("windsurf")),
    },
    Client {
        id: "codex",
        name: "Codex CLI",
        format: Format::CodexToml,
        config: || Some(home()?.join(".codex").join("config.toml")),
        marker: || Some(home()?.join(".codex")),
    },
    Client {
        id: "gemini",
        name: "Gemini CLI",
        format: Format::McpServers,
        config: || Some(home()?.join(".gemini").join("settings.json")),
        marker: || Some(home()?.join(".gemini")),
    },
    Client {
        id: "lmstudio",
        name: "LM Studio",
        format: Format::McpServers,
        config: || Some(home()?.join(".lmstudio").join("mcp.json")),
        marker: || Some(home()?.join(".lmstudio")),
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct ClientView {
    pub id: &'static str,
    pub name: &'static str,
    /// The app looks installed on this computer.
    pub installed: bool,
    /// Its config has a `cliptrail` server entry.
    pub connected: bool,
    pub config_path: Option<String>,
}

/// How assistants start the server: this executable, `--mcp`.
fn server_command() -> Result<(String, Vec<String>)> {
    let exe = std::env::current_exe().context("finding ClipTrail's own path")?;
    Ok((exe.to_string_lossy().into_owned(), vec!["--mcp".to_owned()]))
}

pub fn clients() -> Vec<ClientView> {
    CLIENTS
        .iter()
        .map(|c| {
            let config = (c.config)();
            let installed = (c.marker)().is_some_and(|m| m.exists())
                || config.as_ref().is_some_and(|p| p.exists());
            let connected = config
                .as_ref()
                .is_some_and(|p| has_entry(p, c.format).unwrap_or(false));
            ClientView {
                id: c.id,
                name: c.name,
                installed,
                connected,
                config_path: config.map(|p| p.to_string_lossy().into_owned()),
            }
        })
        .collect()
}

fn client(id: &str) -> Result<&'static Client> {
    CLIENTS
        .iter()
        .find(|c| c.id == id)
        .with_context(|| format!("unknown assistant {id}"))
}

pub fn connect(id: &str) -> Result<()> {
    let c = client(id)?;
    let path = (c.config)().context("couldn't find this app's settings folder")?;
    let (command, args) = server_command()?;
    if c.id == "claude-code" {
        // Claude Code rewrites ~/.claude.json while it runs; its own CLI is
        // the safe way in. Without the CLI, edit the file.
        let _ = claude_cli(&["mcp", "remove", "--scope", "user", SERVER_NAME]);
        let mut cli = vec![
            "mcp",
            "add",
            "--scope",
            "user",
            SERVER_NAME,
            "--",
            command.as_str(),
        ];
        cli.extend(args.iter().map(String::as_str));
        if claude_cli(&cli)? {
            return Ok(());
        }
    }
    edit(&path, c.format, Some((&command, &args)))
}

pub fn disconnect(id: &str) -> Result<()> {
    let c = client(id)?;
    let path = (c.config)().context("couldn't find this app's settings folder")?;
    if c.id == "claude-code" && claude_cli(&["mcp", "remove", "--scope", "user", SERVER_NAME])? {
        return Ok(());
    }
    if !path.exists() {
        return Ok(());
    }
    edit(&path, c.format, None)
}

/// Run the `claude` CLI. Ok(false) when it isn't installed; Ok(true) when it
/// ran and succeeded.
fn claude_cli(args: &[&str]) -> Result<bool> {
    let mut cmd = if cfg!(windows) {
        // `claude` is often an npm .cmd shim, which only cmd resolves.
        let mut c = std::process::Command::new("cmd");
        c.arg("/C").arg("claude").args(args);
        c
    } else {
        let mut c = std::process::Command::new("claude");
        c.args(args);
        c
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = match cmd.stdin(std::process::Stdio::null()).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).context("running the claude CLI"),
    };
    if out.status.success() {
        return Ok(true);
    }
    let err = String::from_utf8_lossy(&out.stderr);
    // cmd reports a missing program this way; anything else is a real failure.
    if err.contains("is not recognized") || out.status.code() == Some(127) {
        return Ok(false);
    }
    // "remove" of an entry that isn't there fails; callers ignore that one.
    if args.get(1) == Some(&"remove") {
        return Ok(true);
    }
    bail!("claude mcp failed: {}", err.trim())
}

/// The JSON block for any other MCP app ("Copy config").
pub fn snippet() -> Result<String> {
    let (command, args) = server_command()?;
    let v = json!({ "mcpServers": { SERVER_NAME: { "command": command, "args": args } } });
    Ok(serde_json::to_string_pretty(&v)?)
}

fn has_entry(path: &Path, format: Format) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(path)?;
    Ok(match format {
        Format::CodexToml => {
            let doc: toml_edit::Document = text.parse()?;
            doc.get("mcp_servers")
                .and_then(|t| t.get(SERVER_NAME))
                .is_some()
        }
        _ => {
            let root = parse_json_object(&text)?;
            root.get(servers_key(format))
                .and_then(|s| s.get(SERVER_NAME))
                .is_some()
        }
    })
}

fn servers_key(format: Format) -> &'static str {
    match format {
        Format::VsCode => "servers",
        _ => "mcpServers",
    }
}

fn parse_json_object(text: &str) -> Result<IndexMap<String, Value>> {
    if text.trim().is_empty() {
        return Ok(IndexMap::new());
    }
    serde_json::from_str(text).context(
        "this app's settings file isn't plain JSON (it may contain comments); \
         use Copy config and add it by hand",
    )
}

/// Add (`Some`) or remove (`None`) our server entry in one config file.
fn edit(path: &Path, format: Format, server: Option<(&str, &[String])>) -> Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let text = existing.as_deref().unwrap_or("");
    let updated = match format {
        Format::CodexToml => edit_toml(text, server)?,
        _ => edit_json(text, format, server)?,
    };
    if existing.as_deref() == Some(updated.as_str()) {
        return Ok(());
    }
    if let Some(old) = &existing {
        let backup = path.with_file_name(format!(
            "{}.cliptrail-backup",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("config")
        ));
        std::fs::write(&backup, old).with_context(|| format!("backing up {}", path.display()))?;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Write next to it, then rename: a crash never leaves half a file.
    let tmp = path.with_extension("cliptrail-tmp");
    std::fs::write(&tmp, &updated)?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn edit_json(text: &str, format: Format, server: Option<(&str, &[String])>) -> Result<String> {
    let mut root = parse_json_object(text)?;
    let key = servers_key(format);
    match server {
        Some((command, args)) => {
            let entry = match format {
                Format::VsCode => json!({ "type": "stdio", "command": command, "args": args }),
                _ => json!({ "command": command, "args": args }),
            };
            let servers = root.entry(key.to_owned()).or_insert_with(|| json!({}));
            let Some(map) = servers.as_object_mut() else {
                bail!("\"{key}\" in this app's settings isn't a list of servers");
            };
            map.insert(SERVER_NAME.to_owned(), entry);
        }
        None => {
            if let Some(map) = root.get_mut(key).and_then(Value::as_object_mut) {
                map.remove(SERVER_NAME);
            }
        }
    }
    Ok(serde_json::to_string_pretty(&root)? + "\n")
}

fn edit_toml(text: &str, server: Option<(&str, &[String])>) -> Result<String> {
    let mut doc: toml_edit::Document = text
        .parse()
        .context("this app's config.toml can't be read")?;
    match server {
        Some((command, args)) => {
            if doc.get("mcp_servers").is_none() {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                doc["mcp_servers"] = toml_edit::Item::Table(t);
            }
            let mut arr = toml_edit::Array::new();
            for a in args {
                arr.push(a.as_str());
            }
            let mut entry = toml_edit::Table::new();
            entry["command"] = toml_edit::value(command);
            entry["args"] = toml_edit::value(arr);
            doc["mcp_servers"][SERVER_NAME] = toml_edit::Item::Table(entry);
        }
        None => {
            if let Some(t) = doc
                .get_mut("mcp_servers")
                .and_then(|i| i.as_table_like_mut())
            {
                t.remove(SERVER_NAME);
            }
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARGS: &[String] = &[];

    fn args() -> Vec<String> {
        vec!["--mcp".to_owned()]
    }

    #[test]
    fn json_connect_keeps_other_servers_and_top_level_order() {
        let before = r#"{"zeta": 1, "mcpServers": {"other": {"command": "x"}}, "alpha": true}"#;
        let a = args();
        let out = edit_json(before, Format::McpServers, Some(("C:\\ct.exe", &a))).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["cliptrail"]["command"], "C:\\ct.exe");
        assert_eq!(v["mcpServers"]["cliptrail"]["args"][0], "--mcp");
        assert!(out.find("zeta").unwrap() < out.find("alpha").unwrap());

        let removed = edit_json(&out, Format::McpServers, None).unwrap();
        let v: Value = serde_json::from_str(&removed).unwrap();
        assert!(v["mcpServers"].get("cliptrail").is_none());
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
    }

    #[test]
    fn json_from_nothing_and_vscode_shape() {
        let a = args();
        let out = edit_json("", Format::VsCode, Some(("/usr/bin/cliptrail", &a))).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["servers"]["cliptrail"]["type"], "stdio");
        assert!(edit_json("// comment\n{}", Format::McpServers, Some(("x", ARGS))).is_err());
    }

    #[test]
    fn toml_connect_and_disconnect_keep_the_rest() {
        let before = "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        let a = args();
        let out = edit_toml(before, Some(("C:\\ct.exe", &a))).unwrap();
        assert!(out.starts_with("model = \"o3\""), "{out}");
        let doc: toml_edit::Document = out.parse().unwrap();
        assert_eq!(
            doc["mcp_servers"]["cliptrail"]["command"].as_str(),
            Some("C:\\ct.exe")
        );
        assert_eq!(doc["mcp_servers"]["other"]["command"].as_str(), Some("x"));

        let removed = edit_toml(&out, None).unwrap();
        let doc: toml_edit::Document = removed.parse().unwrap();
        assert!(doc["mcp_servers"].get("cliptrail").is_none());
        assert_eq!(doc["mcp_servers"]["other"]["command"].as_str(), Some("x"));
    }

    #[test]
    fn edit_writes_a_backup_and_status_follows() {
        let dir = std::env::temp_dir().join(format!("ct-assist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mcp.json");
        std::fs::write(&file, "{\"mcpServers\": {}}").unwrap();
        let a = args();
        edit(&file, Format::McpServers, Some(("ct", &a))).unwrap();
        assert!(has_entry(&file, Format::McpServers).unwrap());
        assert!(dir.join("mcp.json.cliptrail-backup").exists());
        edit(&file, Format::McpServers, None).unwrap();
        assert!(!has_entry(&file, Format::McpServers).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
