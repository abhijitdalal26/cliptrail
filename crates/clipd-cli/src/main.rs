//! `clipd`: ClipTrail from the terminal.
//!
//! Only `show` prints a clip body in full. Everything else prints ids,
//! kinds and a one-line preview; `watch` never prints clip content at all.

mod sync;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chrono::{Local, TimeZone};
use clap::{Parser, Subcommand};
use clipd_core::{
    Clip, ClipSummary, IngestOutcome, Kind, ListQuery, Pipeline, PipelineConfig, RawClip,
    RawContent, default_data_dir, now_millis,
};

/// How often `watch` removes expired (sensitive) clips.
const PRUNE_EVERY: Duration = Duration::from_secs(5 * 60);
/// Preview width in list output, in chars.
const PREVIEW_WIDTH: usize = 80;

#[derive(Parser)]
#[command(name = "clipd", version, about = "ClipTrail clipboard history")]
struct Cli {
    /// Data directory (default: $CLIPTRAIL_DATA_DIR or the OS data dir).
    #[arg(long, global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Watch the clipboard and store every copy (Ctrl+C to stop).
    Watch,
    /// List recent clips, newest first.
    List {
        /// Only this kind: link, code, text, contact, color, file, image, sensitive.
        #[arg(long, value_parser = parse_kind)]
        kind: Option<Kind>,
        /// Only pinned clips.
        #[arg(long)]
        pinned: bool,
        /// Max rows.
        #[arg(short = 'n', long = "limit", default_value_t = 25)]
        limit: u32,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Substring search over text, file paths and source app.
    Search {
        query: String,
        #[arg(long, value_parser = parse_kind)]
        kind: Option<Kind>,
        #[arg(short = 'n', long = "limit", default_value_t = 25)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Show one clip in full (id or unique id prefix).
    Show {
        id: String,
        #[arg(long)]
        json: bool,
        /// Also print the body of a masked (sensitive) clip.
        #[arg(long)]
        reveal: bool,
    },
    /// Totals by kind and source app.
    Stats,
    /// Pin a clip so it's never pruned.
    Pin { id: String },
    /// Unpin a clip.
    Unpin { id: String },
    /// Delete a clip (and its image files).
    Delete { id: String },
    /// Remove expired clips now.
    Prune,
    /// Store a text clip without touching the clipboard ("-" reads stdin).
    IngestText {
        text: String,
        /// Pretend the copy came from this app (e.g. to test exclusions).
        #[arg(long, value_name = "EXE")]
        app: Option<String>,
    },
    /// Store an image clip from a PNG file without touching the clipboard.
    IngestImage {
        path: PathBuf,
        #[arg(long, value_name = "EXE")]
        app: Option<String>,
    },
    /// Sync with your other devices (hub, pairing, devices).
    Sync {
        #[command(subcommand)]
        command: sync::SyncCommand,
    },
}

fn parse_kind(s: &str) -> Result<Kind, String> {
    Kind::parse(&s.to_ascii_lowercase()).ok_or_else(|| {
        let all: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
        format!("unknown kind '{s}' (expected one of: {})", all.join(", "))
    })
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Err(e) = run(Cli::parse()) {
        eprintln!("clipd: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let data_dir = cli.data_dir.unwrap_or_else(default_data_dir);
    let pipeline = Pipeline::open(PipelineConfig::with_defaults(data_dir)?)?;
    let store = pipeline.store();

    match cli.command {
        Command::Watch => watch(pipeline)?,
        Command::List {
            kind,
            pinned,
            limit,
            json,
        } => {
            let rows = store.list(&ListQuery {
                kind,
                pinned_only: pinned,
                limit: Some(limit),
                ..Default::default()
            })?;
            print_rows(&rows, json)?;
        }
        Command::Search {
            query,
            kind,
            limit,
            json,
        } => {
            let rows = store.list(&ListQuery {
                kind,
                search: Some(query),
                limit: Some(limit),
                ..Default::default()
            })?;
            print_rows(&rows, json)?;
        }
        Command::Show { id, json, reveal } => {
            let id = resolve_id(&pipeline, &id)?;
            let mut clip = store.get(&id)?.context("clip vanished")?;
            let hidden = clip.summary.masked && !reveal;
            if hidden {
                clip.text = None;
                clip.html = None;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&clip)?);
            } else {
                print_clip(&clip, hidden);
            }
        }
        Command::Stats => print_stats(&pipeline)?,
        Command::Pin { id } => {
            let id = resolve_id(&pipeline, &id)?;
            store.set_pinned(&id, true)?;
            println!("pinned {id}");
        }
        Command::Unpin { id } => {
            let id = resolve_id(&pipeline, &id)?;
            store.set_pinned(&id, false)?;
            println!("unpinned {id}");
        }
        Command::Delete { id } => {
            let id = resolve_id(&pipeline, &id)?;
            pipeline.delete(&id)?;
            println!("deleted {id}");
        }
        Command::Prune => {
            let n = pipeline.prune()?;
            println!("pruned {n} expired clip{}", if n == 1 { "" } else { "s" });
        }
        Command::IngestText { text, app } => {
            let text = if text == "-" {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .context("read stdin")?;
                buf
            } else {
                text
            };
            let outcome = pipeline.ingest(RawClip {
                content: RawContent::Text { text, html: None },
                source_app: app,
                captured_at: now_millis(),
            })?;
            print_outcome(&pipeline, &outcome);
        }
        Command::IngestImage { path, app } => {
            let img = image::open(&path)
                .with_context(|| format!("read image {}", path.display()))?
                .to_rgba8();
            let (width, height) = img.dimensions();
            let outcome = pipeline.ingest(RawClip {
                content: RawContent::Image {
                    width,
                    height,
                    rgba: img.into_raw(),
                },
                source_app: app,
                captured_at: now_millis(),
            })?;
            print_outcome(&pipeline, &outcome);
        }
        Command::Sync { command } => {
            let data_dir = pipeline.config().data_dir.clone();
            drop(pipeline);
            sync::run(data_dir, command)?;
        }
    }
    Ok(())
}

/// Exact id, or a prefix that matches exactly one clip.
fn resolve_id(pipeline: &Pipeline, id: &str) -> Result<String> {
    let id = id.trim();
    if id.is_empty() {
        bail!("empty id");
    }
    if pipeline.store().get(id)?.is_some() {
        return Ok(id.to_owned());
    }
    let mut matches = pipeline.store().ids_with_prefix(id, 2)?;
    match matches.len() {
        0 => bail!("no clip with id {id}"),
        1 => Ok(matches.remove(0)),
        _ => bail!("id prefix {id} is ambiguous; use more characters"),
    }
}

enum Event {
    Clip(Box<RawClip>),
    Stop,
}

fn watch(pipeline: Pipeline) -> Result<()> {
    let (tx, rx) = mpsc::channel::<Event>();
    let stop_tx = tx.clone();
    ctrlc::set_handler(move || {
        let _ = stop_tx.send(Event::Stop);
    })
    .context("install Ctrl+C handler")?;
    let watcher = clipd_capture::spawn_watcher(move |raw| {
        let _ = tx.send(Event::Clip(Box::new(raw)));
    })
    .context("start clipboard watcher")?;

    eprintln!(
        "watching clipboard → {} (Ctrl+C to stop)",
        pipeline.config().data_dir.display()
    );
    let mut next_prune = Instant::now();
    loop {
        if Instant::now() >= next_prune {
            match pipeline.prune() {
                Ok(0) => {}
                Ok(n) => println!("{}  pruned   {n} expired", clock(now_millis())),
                Err(e) => eprintln!("prune failed: {e:#}"),
            }
            next_prune = Instant::now() + PRUNE_EVERY;
        }
        match rx.recv_timeout(next_prune.saturating_duration_since(Instant::now())) {
            Ok(Event::Clip(raw)) => match pipeline.ingest(*raw) {
                Ok(outcome) => print_outcome(&pipeline, &outcome),
                // Pipeline errors never contain clip content.
                Err(e) => eprintln!("{}  error    {e:#}", clock(now_millis())),
            },
            Ok(Event::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    watcher.stop();
    eprintln!("stopped");
    Ok(())
}

/// `HH:MM:SS  outcome  kind/subkind  id` — never the content.
fn print_outcome(pipeline: &Pipeline, outcome: &IngestOutcome) {
    let now = clock(now_millis());
    match outcome {
        IngestOutcome::Stored { id, kind } | IngestOutcome::Bumped { id, kind } => {
            let verb = if matches!(outcome, IngestOutcome::Stored { .. }) {
                "stored"
            } else {
                "bumped"
            };
            let subkind = pipeline
                .store()
                .get(id)
                .ok()
                .flatten()
                .and_then(|c| c.summary.subkind);
            println!(
                "{now}  {verb:<7}  {:<18}  {id}",
                kind_label(*kind, subkind.as_deref())
            );
        }
        IngestOutcome::Skipped { reason } => println!("{now}  skipped  ({reason})"),
    }
    let _ = std::io::stdout().flush();
}

fn print_rows(rows: &[ClipSummary], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("no clips");
        return Ok(());
    }
    for r in rows {
        let flags = format!(
            "{}{}",
            if r.pinned { "P" } else { " " },
            if r.copy_count > 1 { "+" } else { " " }
        );
        println!(
            "{}  {}  {:<18} {flags}  {}",
            r.id,
            when(r.last_seen_at),
            kind_label(r.kind, r.subkind.as_deref()),
            preview_line(r)
        );
    }
    Ok(())
}

fn print_clip(clip: &Clip, hidden: bool) {
    let s = &clip.summary;
    let field = |name: &str, value: &str| println!("{:<10} {value}", format!("{name}:"));
    field("id", &s.id);
    field("kind", &kind_label(s.kind, s.subkind.as_deref()));
    field("created", &when(s.created_at));
    field(
        "last seen",
        &format!("{} (copied {}×)", when(s.last_seen_at), s.copy_count),
    );
    if let Some(app) = &s.source_app {
        field("source", app);
    }
    field("device", &s.device_id);
    if s.pinned {
        field("pinned", "yes");
    }
    if let Some(exp) = s.expires_at {
        field("expires", &when(exp));
    }
    if let (Some(w), Some(h)) = (s.width, s.height) {
        field("size", &format!("{w}×{h}"));
    } else {
        field(
            "size",
            &format!(
                "{} chars, {} line{}",
                s.char_count,
                s.line_count,
                if s.line_count == 1 { "" } else { "s" }
            ),
        );
    }
    if let Some(p) = &clip.image_path {
        field("image", p);
    }
    if let Some(p) = &s.thumb_path {
        field("thumb", p);
    }
    if clip.html.is_some() {
        field("html", "yes");
    }
    if hidden {
        println!(
            "\n{}  (masked; pass --reveal to print)",
            clipd_core::store::MASKED_PREVIEW
        );
    } else if let Some(text) = &clip.text {
        println!("\n{text}");
    }
}

fn print_stats(pipeline: &Pipeline) -> Result<()> {
    let store = pipeline.store();
    let s = store.stats()?;
    let dir = &pipeline.config().data_dir;
    println!("data dir:  {}", dir.display());
    // Recent writes sit in the WAL until checkpointed; count both.
    let db_bytes: u64 = ["clips.db", "clips.db-wal"]
        .iter()
        .filter_map(|f| std::fs::metadata(dir.join(f)).ok())
        .map(|m| m.len())
        .sum();
    println!("database:  {}", human_bytes(db_bytes));
    println!("clips:     {} ({} pinned)", s.total, s.pinned);
    if !s.by_kind.is_empty() {
        println!("\nby kind:");
        for (kind, n) in &s.by_kind {
            println!("  {:<10} {n:>6}", kind.as_str());
        }
    }
    if !s.top_apps.is_empty() {
        println!("\ntop apps:");
        for (app, n) in &s.top_apps {
            println!("  {app:<24} {n:>6}");
        }
    }
    let now = now_millis();
    let days = store.day_counts(now - 7 * 86_400_000, now + 1)?;
    if !days.is_empty() {
        println!("\nlast 7 days:");
        for d in &days {
            println!("  {}  {:>5}", d.day, d.count);
        }
    }
    Ok(())
}

fn kind_label(kind: Kind, subkind: Option<&str>) -> String {
    match subkind {
        Some(sub) => format!("{}/{sub}", kind.as_str()),
        None => kind.as_str().to_owned(),
    }
}

/// One terminal line: newlines as ⏎, other control chars dropped.
fn preview_line(r: &ClipSummary) -> String {
    if r.kind == Kind::Image {
        return match (r.width, r.height) {
            (Some(w), Some(h)) => format!("[image {w}×{h}]"),
            _ => "[image]".to_owned(),
        };
    }
    let mut out = String::new();
    let mut chars = 0;
    let mut src = r.preview.trim().chars().peekable();
    while let Some(c) = src.next() {
        if chars == PREVIEW_WIDTH {
            out.push('…');
            return out;
        }
        match c {
            '\r' if src.peek() == Some(&'\n') => continue,
            '\n' | '\r' => out.push('⏎'),
            '\t' => out.push(' '),
            c if c.is_control() => continue,
            c => out.push(c),
        }
        chars += 1;
    }
    if r.truncated {
        out.push('…');
    }
    out
}

fn when(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| ms.to_string())
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

fn clock(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| ms.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(preview: &str, truncated: bool) -> ClipSummary {
        ClipSummary {
            id: "01X".into(),
            kind: Kind::Text,
            subkind: None,
            preview: preview.into(),
            truncated,
            char_count: preview.chars().count() as i64,
            line_count: 1,
            files: vec![],
            thumb_path: None,
            width: None,
            height: None,
            source_app: None,
            device_id: "d".into(),
            created_at: 0,
            last_seen_at: 0,
            copy_count: 1,
            pinned: false,
            masked: false,
            expires_at: None,
        }
    }

    #[test]
    fn preview_is_one_line() {
        assert_eq!(preview_line(&row("a\r\nb\nc\td\u{7}", false)), "a⏎b⏎c d");
        assert_eq!(preview_line(&row("short", true)), "short…");
        let long = "é".repeat(200);
        let p = preview_line(&row(&long, false));
        assert_eq!(p.chars().count(), PREVIEW_WIDTH + 1);
        assert!(p.ends_with('…'));
        let mut img = row("", false);
        img.kind = Kind::Image;
        img.width = Some(10);
        img.height = Some(5);
        assert_eq!(preview_line(&img), "[image 10×5]");
    }

    #[test]
    fn sizes_and_kinds() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(kind_label(Kind::Code, Some("rust")), "code/rust");
        assert_eq!(parse_kind("CODE").unwrap(), Kind::Code);
        assert!(parse_kind("nope").is_err());
    }

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
