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
    /// Lowercase pieces of the name the app gives itself over MCP
    /// (`clientInfo.name`), to match `clipd_mcp::recent_clients`.
    mcp_names: &'static [&'static str],
    /// The config file (may not exist yet).
    config: fn() -> Option<PathBuf>,
    /// Paths that exist only while the app is installed: its program (in its
    /// install folder, as a macOS bundle, or as a command on PATH). Data
    /// folders don't count: they outlive an uninstall, and connecting writes
    /// into some of them.
    markers: fn() -> Vec<PathBuf>,
}

/// A program on PATH (with Windows' executable extensions), resolved.
fn on_path(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|e| e.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| exts.iter().map(move |e| dir.join(format!("{name}{e}"))))
        .find(|p| p.is_file())
}

/// macOS: `/Applications/<name>.app` (and the per-user Applications folder).
fn app_bundle(name: &str) -> Vec<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let mut v = vec![PathBuf::from(format!("/Applications/{name}.app"))];
    v.extend(home().map(|h| h.join("Applications").join(format!("{name}.app"))));
    v
}

/// Windows: the package folders of an installed Claude Desktop (MSIX).
fn claude_package_dirs() -> Vec<PathBuf> {
    let Some(packages) = dirs::data_local_dir().map(|d| d.join("Packages")) else {
        return Vec::new();
    };
    std::fs::read_dir(packages)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .starts_with("claude_")
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// `base` joined with each relative path ("a/b" works on every OS).
fn in_dir(base: Option<PathBuf>, rels: &[&str]) -> Vec<PathBuf> {
    let Some(base) = base else { return Vec::new() };
    rels.iter()
        .map(|r| r.split('/').fold(base.clone(), |p, part| p.join(part)))
        .collect()
}

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Where desktop apps keep per-user settings: %APPDATA% on Windows,
/// ~/Library/Application Support on macOS, ~/.config on Linux.
fn app_config() -> Option<PathBuf> {
    dirs::config_dir()
}

/// The folder Claude Desktop actually reads its settings from. The Windows
/// package (MSIX) build may keep user data outside the package in
/// `%LOCALAPPDATA%\Claude-Data\<package>\`, or inside it in the package's
/// redirected `LocalCache\Roaming\Claude`; the classic installer, macOS and
/// Linux use `<config dir>/Claude`. The first one Claude has written its own
/// `config.json` to wins.
fn claude_desktop_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        let subdirs = |dir: PathBuf, prefix: &str| -> Vec<PathBuf> {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                return Vec::new();
            };
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .starts_with(prefix)
                })
                .map(|e| e.path())
                .collect()
        };
        candidates.extend(subdirs(local.join("Claude-Data"), "claude_"));
        candidates.extend(
            subdirs(local.join("Packages"), "claude_")
                .into_iter()
                .map(|p| p.join("LocalCache").join("Roaming").join("Claude")),
        );
    }
    let classic = app_config().map(|c| c.join("Claude"));
    candidates.extend(classic.clone());
    candidates
        .into_iter()
        .find(|d| d.join("config.json").exists())
        .or(classic)
}

const CLIENTS: &[Client] = &[
    Client {
        id: "claude-desktop",
        name: "Claude Desktop",
        format: Format::McpServers,
        mcp_names: &["claude-ai", "claude desktop"],
        config: || Some(claude_desktop_dir()?.join("claude_desktop_config.json")),
        markers: || {
            // The Windows package (MSIX) folder exists only while it's installed.
            let mut v = claude_package_dirs();
            v.extend(in_dir(
                dirs::data_local_dir(),
                &["AnthropicClaude/claude.exe"],
            ));
            v.extend(app_bundle("Claude"));
            v
        },
    },
    Client {
        id: "claude-code",
        name: "Claude Code",
        format: Format::McpServers,
        mcp_names: &["claude-code"],
        config: || Some(home()?.join(".claude.json")),
        markers: || on_path("claude").into_iter().collect(),
    },
    Client {
        id: "cursor",
        name: "Cursor",
        format: Format::McpServers,
        mcp_names: &["cursor"],
        config: || Some(home()?.join(".cursor").join("mcp.json")),
        markers: || {
            let mut v = in_dir(dirs::data_local_dir(), &["Programs/cursor/Cursor.exe"]);
            v.extend(app_bundle("Cursor"));
            v.extend(on_path("cursor"));
            v
        },
    },
    Client {
        id: "vscode",
        name: "VS Code (GitHub Copilot)",
        format: Format::VsCode,
        mcp_names: &["visual studio code", "vscode"],
        config: || Some(app_config()?.join("Code").join("User").join("mcp.json")),
        markers: || {
            let mut v = in_dir(
                dirs::data_local_dir(),
                &["Programs/Microsoft VS Code/Code.exe"],
            );
            v.extend(in_dir(
                std::env::var_os("ProgramFiles").map(PathBuf::from),
                &["Microsoft VS Code/Code.exe"],
            ));
            v.extend(app_bundle("Visual Studio Code"));
            v.extend(on_path("code"));
            v
        },
    },
    Client {
        id: "windsurf",
        name: "Windsurf",
        format: Format::McpServers,
        mcp_names: &["windsurf", "codeium"],
        config: || {
            Some(
                home()?
                    .join(".codeium")
                    .join("windsurf")
                    .join("mcp_config.json"),
            )
        },
        markers: || {
            let mut v = in_dir(dirs::data_local_dir(), &["Programs/Windsurf/Windsurf.exe"]);
            v.extend(app_bundle("Windsurf"));
            v.extend(on_path("windsurf"));
            v
        },
    },
    Client {
        id: "codex",
        name: "Codex CLI",
        format: Format::CodexToml,
        mcp_names: &["codex"],
        config: || Some(home()?.join(".codex").join("config.toml")),
        markers: || on_path("codex").into_iter().collect(),
    },
    Client {
        id: "gemini",
        name: "Gemini CLI",
        format: Format::McpServers,
        mcp_names: &["gemini"],
        config: || Some(home()?.join(".gemini").join("settings.json")),
        markers: || on_path("gemini").into_iter().collect(),
    },
    Client {
        id: "lmstudio",
        name: "LM Studio",
        format: Format::McpServers,
        mcp_names: &["lm studio", "lmstudio"],
        config: || Some(home()?.join(".lmstudio").join("mcp.json")),
        markers: || {
            let mut v = in_dir(
                dirs::data_local_dir(),
                &["Programs/LM Studio/LM Studio.exe"],
            );
            v.extend(app_bundle("LM Studio"));
            v.extend(on_path("lms"));
            v
        },
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
    /// Last time this app actually started ClipTrail's server (ms), as
    /// recorded by `cliptrail --mcp`: proof it's working, not just set up.
    pub last_used: Option<i64>,
    pub config_path: Option<String>,
}

/// How assistants start the server: this executable, `--mcp`.
fn server_command() -> Result<(String, Vec<String>)> {
    let exe = std::env::current_exe().context("finding ClipTrail's own path")?;
    Ok((exe.to_string_lossy().into_owned(), vec!["--mcp".to_owned()]))
}

pub fn clients() -> Vec<ClientView> {
    let used = clipd_mcp::recent_clients(&clipd_core::default_data_dir());
    CLIENTS
        .iter()
        .map(|c| {
            let config = (c.config)();
            let installed = (c.markers)().iter().any(|m| m.exists());
            let connected = config
                .as_ref()
                .is_some_and(|p| has_entry(p, c.format).unwrap_or(false));
            let last_used = used
                .iter()
                .filter(|(name, _)| {
                    let n = name.to_lowercase();
                    c.mcp_names.iter().any(|m| n.contains(m))
                })
                .map(|(_, at)| *at)
                .max();
            ClientView {
                id: c.id,
                name: c.name,
                installed,
                connected,
                last_used,
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
    if !(c.markers)().iter().any(|m| m.exists()) {
        bail!("{} isn't installed on this computer", c.name);
    }
    // Only point an app at a server that answers.
    self_test().context("ClipTrail's AI server didn't answer, so nothing was changed")?;
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

/// The Settings switch: connect every assistant found on this computer.
/// Keeps going past one that fails; the error names the ones that did.
pub fn connect_all() -> Result<()> {
    let failed: Vec<String> = clients()
        .into_iter()
        .filter(|c| c.installed && !c.connected)
        .filter_map(|c| connect(c.id).err().map(|e| format!("{}: {e:#}", c.name)))
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        bail!("{}", failed.join("; "))
    }
}

/// The Settings switch turned off: disconnect every connected assistant.
pub fn disconnect_all() -> Result<()> {
    let failed: Vec<String> = clients()
        .into_iter()
        .filter(|c| c.connected)
        .filter_map(|c| disconnect(c.id).err().map(|e| format!("{}: {e:#}", c.name)))
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        bail!("{}", failed.join("; "))
    }
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

/// Start `cliptrail --mcp` the way an assistant does and check it lists its
/// tools. Returns how many.
pub fn self_test() -> Result<usize> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let (command, args) = server_command()?;
    let mut cmd = Command::new(&command);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().context("starting cliptrail --mcp")?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": { "name": clipd_mcp::SELF_TEST_CLIENT, "version": env!("CARGO_PKG_VERSION") }
        }
    });
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    writeln!(stdin, "{init}\n{list}")?;
    stdin.flush()?;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let result = loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(line) => {
                let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                if v["id"] == 2 {
                    break v["result"]["tools"]
                        .as_array()
                        .map(Vec::len)
                        .filter(|n| *n > 0)
                        .context("it answered without any tools");
                }
                if let Some(msg) = v.pointer("/error/message").and_then(Value::as_str) {
                    break Err(anyhow::anyhow!("{msg}"));
                }
            }
            Err(_) => break Err(anyhow::anyhow!("no answer within 10 seconds")),
        }
    };
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    result
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
    fn json_connect_keeps_nested_settings_exactly() {
        let before = "{
  \"preferences\": {
    \"zebra\": 1,
    \"apple\": {\"b\": 2, \"a\": 1}
  }
}";
        let a = args();
        let out = edit_json(before, Format::McpServers, Some(("ct", &a))).unwrap();
        // Claude Desktop's own settings keep their order, nested keys included.
        assert!(
            out.find("zebra").unwrap() < out.find("apple").unwrap(),
            "{out}"
        );
        assert!(
            out.find("\"b\"").unwrap() < out.find("\"a\"").unwrap(),
            "{out}"
        );
        let back = edit_json(&out, Format::McpServers, None).unwrap();
        let v: Value = serde_json::from_str(&back).unwrap();
        assert_eq!(v["preferences"]["apple"]["b"], 2);
    }

    #[test]
    fn programs_are_found_on_path_and_missing_ones_are_not() {
        let present = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(on_path(present).is_some());
        assert!(on_path("cliptrail-surely-not-a-program").is_none());
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
