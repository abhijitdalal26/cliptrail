//! Tool definitions and dispatch for the ClipTrail MCP server.

use anyhow::{Result, bail};
use chrono::NaiveDate;
use clipd_core::{Kind, ListQuery, Millis, Store, store::MASKED_PREVIEW};
use serde_json::{Value, json};

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
        .map(|n| (n as u32).clamp(1, max))
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

pub fn call(store: &Store, name: &str, args: &Value) -> Result<Value> {
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
        "day_counts" => {
            let days = limit_arg(args, 30, 365);
            let now = clipd_core::now_millis();
            let from = now - i64::from(days) * 86_400_000;
            Ok(text_result(&serde_json::to_value(
                store.day_counts(from, now)?,
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
        _ => bail!("unknown tool: {name}"),
    }
}

pub fn definitions() -> Value {
    json!([
        {
            "name": "search_clips",
            "description": "Search clipboard history: full-text query plus kind, source app, pinned and date filters. Returns clip summaries (masked previews for sensitive clips).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Full-text search over clip text" },
                    "kind": { "type": "string", "enum": ["link", "code", "text", "contact", "color", "file", "image", "sensitive"], "description": "Clip category" },
                    "source_app": { "type": "string", "description": "Filter by the app the clip was copied from, e.g. chrome.exe" },
                    "pinned_only": { "type": "boolean", "description": "Only pinned clips" },
                    "from": { "description": "Start of time range: unix-ms or YYYY-MM-DD" },
                    "to": { "description": "End of time range: unix-ms or YYYY-MM-DD" },
                    "limit": { "type": "integer", "description": "Max results (default 25, max 200)" }
                }
            }
        },
        {
            "name": "recent_clips",
            "description": "Newest clips first, without a text query — 'what did I copy recently'. Same filters as search_clips minus query.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["link", "code", "text", "contact", "color", "file", "image", "sensitive"] },
                    "source_app": { "type": "string" },
                    "pinned_only": { "type": "boolean" },
                    "from": { "description": "unix-ms or YYYY-MM-DD" },
                    "to": { "description": "unix-ms or YYYY-MM-DD" },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "get_clip",
            "description": "Fetch one clip's full body by id (accepts a ULID prefix from list results). Masked/sensitive clips return metadata only — body is withheld.",
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
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "list_source_apps",
            "description": "Apps clips were copied from (for filtering by source_app).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "day_counts",
            "description": "Clip counts per calendar day — 'what did I do on Tuesday' style questions and activity heatmaps.",
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
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "pinned": { "type": "boolean", "description": "default true" }
                },
                "required": ["id"]
            }
        }
    ])
}
