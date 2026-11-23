//! Tool definitions and dispatch for the ClipTrail MCP server.

use anyhow::{Result, bail};
use chrono::NaiveDate;
use clipd_capture::WriteContent;
use clipd_core::{
    IngestOutcome, Kind, ListQuery, Millis, Pipeline, RawClip, RawContent, Store,
    store::MASKED_PREVIEW,
};
use serde_json::{Value, json};

/// What a tool call can use. `pipeline` (for `save_to_history`) and the device
/// info are there when serving for real; tests use a bare store.
pub struct Ctx<'a> {
    pub store: &'a Store,
    pub pipeline: Option<&'a Pipeline>,
    /// This computer's device id.
    pub device_id: String,
    /// (device id, name) of every paired device, this one included.
    pub device_names: Vec<(String, String)>,
}

#[cfg(test)]
impl<'a> Ctx<'a> {
    pub fn store_only(store: &'a Store) -> Ctx<'a> {
        Ctx {
            store,
            pipeline: None,
            device_id: String::new(),
            device_names: Vec::new(),
        }
    }
}

fn text_result(value: &Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(value).unwrap() }]
    })
}

/// Parse an optional millis-or-"YYYY-MM-DD" argument.
fn time_arg(args: &Value, key: &str) -> Result<Option<Millis>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_i64()
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("{key}: expected integer unix-ms")),
        Some(Value::String(s)) => {
            let d = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("{key}: expected unix-ms or YYYY-MM-DD"))?;
            Ok(Some(
                d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis(),
            ))
        }
        _ => bail!("{key}: expected unix-ms integer or YYYY-MM-DD string"),
    }
}

fn limit_arg(args: &Value, default: u32, max: u32) -> u32 {
    args.get("limit")
        .and_then(Value::as_u64)
        .map(|n| u32::try_from(n.clamp(1, u64::from(max))).unwrap_or(max))
        .unwrap_or(default)
}

fn list_query(args: &Value) -> Result<ListQuery> {
    let kind = match args.get("kind").and_then(Value::as_str) {
        None => None,
        Some(s) => Some(Kind::parse(s).ok_or_else(|| anyhow::anyhow!("unknown kind: {s}"))?),
    };
    Ok(ListQuery {
        kind,
        search: args
            .get("query")
            .and_then(Value::as_str)
            .map(str::to_string),
        pinned_only: args
            .get("pinned_only")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        source_app: args
            .get("source_app")
            .and_then(Value::as_str)
            .map(str::to_string),
        device_id: args
            .get("device_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        collection_id: args
            .get("collection_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        from: time_arg(args, "from")?,
        to: time_arg(args, "to")?,
        before: time_arg(args, "before")?,
        before_id: args
            .get("before_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        limit: Some(limit_arg(args, 25, 200)),
    })
}

/// Resolve an exact id or unique ULID prefix to a full clip id.
fn resolve_id(store: &Store, id_or_prefix: &str) -> Result<String> {
    let ids = store.ids_with_prefix(&id_or_prefix.to_uppercase(), 2)?;
    match ids.len() {
        0 => bail!("no clip matching id {id_or_prefix}"),
        1 => Ok(ids[0].clone()),
        _ => bail!("id prefix {id_or_prefix} is ambiguous"),
    }
}

/// Full clip for output. Masked clips never expose body — summary only.
fn clip_json(clip: &clipd_core::Clip) -> Value {
    let mut v = serde_json::to_value(&clip.summary).unwrap_or_default();
    if clip.summary.masked {
        v["preview"] = json!(MASKED_PREVIEW);
        v["note"] = json!("sensitive clip — content withheld");
    } else {
        v["text"] = json!(clip.text);
        v["html"] = json!(clip.html);
        v["image_path"] = json!(clip.image_path);
    }
    v
}

pub fn call(ctx: &Ctx, name: &str, args: &Value) -> Result<Value> {
    let store = ctx.store;
    match name {
        "search_clips" => {
            let q = list_query(args)?;
            let clips = store.list(&q)?;
            Ok(text_result(&serde_json::to_value(clips)?))
        }
        "recent_clips" => {
            let mut q = list_query(args)?;
            q.search = None;
            let clips = store.list(&q)?;
            Ok(text_result(&serde_json::to_value(clips)?))
        }
        "get_clip" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing required arg: id"))?;
            let id = resolve_id(store, id)?;
            match store.get(&id)? {
                Some(clip) => Ok(text_result(&clip_json(&clip))),
                None => bail!("no clip with id {id}"),
            }
        }
        "clip_stats" => Ok(text_result(&serde_json::to_value(store.stats()?)?)),
        "list_source_apps" => Ok(text_result(&json!(store.source_apps()?))),
        "list_collections" => Ok(text_result(&serde_json::to_value(store.collections()?)?)),
        "day_counts" => {
            let days = limit_arg(args, 30, 365);
            let now = clipd_core::now_millis();
            let from = now - i64::from(days) * 86_400_000;
            Ok(text_result(&serde_json::to_value(
                store.day_counts(from, now + 1)?,
            )?))
        }
        "pin_clip" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing required arg: id"))?;
            let pinned = args.get("pinned").and_then(Value::as_bool).unwrap_or(true);
            let id = resolve_id(store, id)?;
            store.set_pinned(&id, pinned)?;
            Ok(text_result(&json!({ "id": id, "pinned": pinned })))
        }
        "copy_to_clipboard" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing required arg: id"))?;
            let id = resolve_id(store, id)?;
            let Some(clip) = store.get(&id)? else {
                bail!("no clip with id {id}")
            };
            if clip.summary.masked {
                bail!("refusing to copy a sensitive clip");
            }
            if clip.summary.kind == Kind::Image && clip.image_path.is_none() {
                bail!("image file not synced to this device yet");
            }
            let local_files: Vec<String> = clip
                .summary
                .files
                .iter()
                .filter(|f| std::path::Path::new(f).exists())
                .cloned()
                .collect();
            if let Some(path) = &clip.image_path {
                let png = clipd_core::read_image(path)?;
                clipd_capture::write_clip(WriteContent::ImagePng(&png))?;
            } else if !clip.summary.files.is_empty() {
                if local_files.is_empty() {
                    bail!("none of this clip's files exist on this device");
                }
                clipd_capture::write_clip(WriteContent::Files(&local_files))?;
            } else {
                let text = clip.text.unwrap_or_default();
                clipd_capture::write_clip(WriteContent::Text {
                    text: &text,
                    html: clip.html.as_deref(),
                })?;
            }
            let mut out = json!({ "id": id, "copied": true });
            if cfg!(target_os = "linux") {
                // X11 selections are served by the owning process: once this
                // session ends the paste only survives if a clipboard
                // manager has taken ownership.
                out["note"] = json!(
                    "on X11 the copy survives this process only if a clipboard manager owns it"
                );
            }
            Ok(text_result(&out))
        }
        "get_current_clipboard" => {
            let raw = clipd_capture::read_current()?;
            Ok(text_result(&current_clipboard_json(raw)))
        }
        "save_to_history" => {
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("missing required arg: text"))?;
            let Some(pipeline) = ctx.pipeline else {
                bail!("saving isn't available here");
            };
            let outcome = pipeline.ingest(RawClip {
                content: RawContent::Text {
                    text: text.to_owned(),
                    html: None,
                },
                source_app: Some(SAVED_BY.to_owned()),
                captured_at: clipd_core::now_millis(),
                source_url: None,
            })?;
            Ok(text_result(&match outcome {
                IngestOutcome::Stored { id, kind } => {
                    json!({ "saved": true, "id": id, "kind": kind })
                }
                IngestOutcome::Bumped { id, kind } => json!({
                    "saved": true, "id": id, "kind": kind,
                    "note": "already in history; moved to the top"
                }),
                IngestOutcome::Skipped { reason } => json!({ "saved": false, "reason": reason }),
            }))
        }
        "list_devices" => {
            let counts = store.stats()?.by_device;
            let name_of = |id: &str| {
                ctx.device_names
                    .iter()
                    .find(|(d, _)| d == id)
                    .map(|(_, n)| n.clone())
            };
            let devices: Vec<Value> = counts
                .iter()
                .map(|(id, n)| {
                    json!({
                        "device_id": id,
                        "name": name_of(id).unwrap_or_else(|| "unknown device".into()),
                        "this_device": *id == ctx.device_id,
                        "clips": n,
                    })
                })
                .collect();
            Ok(text_result(&json!(devices)))
        }
        _ => bail!("unknown tool: {name}"),
    }
}

/// `source_app` of clips an assistant saved, so they're easy to tell apart.
const SAVED_BY: &str = "AI assistant";

/// The clipboard right now, the way search results describe clips. Text that
/// looks sensitive (keys, codes, cards) is withheld like a masked clip.
fn current_clipboard_json(raw: Option<RawClip>) -> Value {
    let Some(raw) = raw else {
        return json!({ "empty": true, "note": "the clipboard is empty, unsupported or excluded" });
    };
    let app = raw.source_app.clone();
    match raw.content {
        RawContent::Text { text, .. } => {
            let class = clipd_core::classify_text(&text);
            if class.kind == Kind::Sensitive {
                json!({
                    "kind": "sensitive", "source_app": app, "text": MASKED_PREVIEW,
                    "note": "sensitive content withheld"
                })
            } else {
                json!({ "kind": class.kind, "subkind": class.subkind, "source_app": app, "text": text })
            }
        }
        RawContent::Image { width, height, .. } => {
            json!({ "kind": "image", "source_app": app, "width": width, "height": height })
        }
        RawContent::Files(files) => json!({ "kind": "file", "source_app": app, "files": files }),
    }
}

pub fn definitions() -> Value {
    json!([
            {
                "name": "search_clips",
                "description": "Search everything the user has copied on this computer and their synced phone: full-text query plus kind, source app, device, collection, pinned and date filters. Use it whenever the user refers to something they copied, pasted or had earlier (an error, a link, a command, an address, a snippet). Returns clip summaries; sensitive clips show only a masked preview.",
                "annotations": { "readOnlyHint": true, "openWorldHint": false },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Full-text search over clip text" },
                        "kind": { "type": "string", "enum": ["link", "code", "text", "contact", "color", "file", "image", "sensitive"], "description": "Clip category" },
                        "source_app": { "type": "string", "description": "Filter by the app the clip was copied from, e.g. chrome.exe" },
                        "device_id": { "type": "string", "description": "Only clips from this device (ids from list_devices), e.g. the user's phone" },
                        "pinned_only": { "type": "boolean", "description": "Only pinned clips" },
                        "collection_id": { "type": "string", "description": "Only clips the user saved to this collection (ids from list_collections)" },
                        "from": { "description": "Start of time range: unix-ms or YYYY-MM-DD" },
                        "to": { "description": "End of time range: unix-ms or YYYY-MM-DD" },
                        "limit": { "type": "integer", "description": "Max results (default 25, max 200)" }
                    }
                }
            },
            {
                "name": "recent_clips",
                "description": "Newest clips first, without a text query — 'what did I copy recently'. Same filters as search_clips minus query.",
                "annotations": { "readOnlyHint": true, "openWorldHint": false },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["link", "code", "text", "contact", "color", "file", "image", "sensitive"] },
                        "source_app": { "type": "string" },
                        "device_id": { "type": "string", "description": "Only clips from this device (see list_devices)" },
                        "pinned_only": { "type": "boolean" },
                        "collection_id": { "type": "string", "description": "Only clips in this collection (see list_collections)" },
                        "from": { "description": "unix-ms or YYYY-MM-DD" },
                        "to": { "description": "unix-ms or YYYY-MM-DD" },
                        "limit": { "type": "integer" }
                    }
                }
            },
            {
                "name": "get_clip",
                "description": "Fetch one clip's full body by id (accepts a ULID prefix from list results). Masked/sensitive clips return metadata only — body is withheld.",
                "annotations": { "readOnlyHint": true, "openWorldHint": false },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Clip id or unique prefix" }
                    },
                    "required": ["id"]
                }
            },
            {
                "name": "clip_stats",
                "description": "Totals: clip count per kind, pinned count, top source apps.",
                "annotations": { "readOnlyHint": true, "openWorldHint": false },
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "list_source_apps",
                "description": "Apps clips were copied from (for filtering by source_app).",
                "annotations": { "readOnlyHint": true, "openWorldHint": false },
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "list_collections",
                "description": "The user's own collections (folders like 'Work' or 'Recipes' they save clips to), with ids for the collection_id filter and clip counts — for 'what's in my Work collection'.",
                "annotations": { "readOnlyHint": true, "openWorldHint": false },
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "day_counts",
                "description": "Clip counts per calendar day — 'what did I do on Tuesday' style questions and activity heatmaps.",
                "annotations": { "readOnlyHint": true, "openWorldHint": false },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "description": "Days back from now (default 30, max 365)" }
                    }
                }
            },
            {
                "name": "pin_clip",
                "description": "Pin or unpin a clip by id or prefix so it stays at the top of history.",
                "annotations": { "readOnlyHint": false, "idempotentHint": true, "destructiveHint": false, "openWorldHint": false },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "pinned": { "type": "boolean", "description": "default true" }
                    },
                    "required": ["id"]
                }
            },
            {
                "name": "copy_to_clipboard",
                "description": "Put a clip back on the OS clipboard so the user can paste it — 'get me that link/snippet ready to paste'. Refuses masked/sensitive clips.",
                "annotations": { "readOnlyHint": false, "idempotentHint": true, "destructiveHint": false, "openWorldHint": false },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Clip id or unique prefix" }
                    },
                    "required": ["id"]
                }
            }
    ,
            {
                "name": "get_current_clipboard",
                "description": "What the user has on the clipboard right now ('the error I just copied', 'this link'). Text that looks sensitive is withheld.",
                "annotations": { "readOnlyHint": true, "openWorldHint": false },
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "save_to_history",
                "description": "Save text to the user's ClipTrail history (and their synced phone) so they can find or paste it later: a command, link or snippet you produced that they'll want again. Only when it's useful or the user asks.",
                "annotations": { "readOnlyHint": false, "idempotentHint": true, "destructiveHint": false, "openWorldHint": false },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "The text to save" }
                    },
                    "required": ["text"]
                }
            },
            {
                "name": "list_devices",
                "description": "The devices clips came from (this computer, the user's phone...) with ids for the device_id filter and clip counts, for 'what did I copy on my phone'.",
                "annotations": { "readOnlyHint": true, "openWorldHint": false },
                "inputSchema": { "type": "object", "properties": {} }
            }
        ])
}
