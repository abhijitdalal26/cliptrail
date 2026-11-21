//! MCP stdio protocol: newline-delimited JSON-RPC 2.0.
//!
//! `handle` maps one incoming message to an optional response — notifications
//! and parse-level handling that produce no response return None. It is pure
//! (no I/O) so tests drive it without pipes.

use serde_json::{Value, json};

use crate::tools::{self, Ctx};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "clipd-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Told to the assistant at connect: when ClipTrail helps, and the rules.
const INSTRUCTIONS: &str = "ClipTrail is the user's clipboard history: everything they copied on this computer and on their synced phone, with the app, time and device of each copy. \
Look here whenever the user mentions something they copied, pasted or had earlier (an error message, a link, a command, an address, a snippet), or says 'that thing from yesterday': \
use search_clips (filters for kind, app, device and dates) or recent_clips, then get_clip for the full text, and copy_to_clipboard to make it ready to paste. \
get_current_clipboard reads what's copied right now; save_to_history keeps something you produced for the user to find later. \
Sensitive clips (keys, one-time codes, cards) are masked and never expose their content.";

fn prompt_list() -> Value {
    json!([
        {
            "name": "find_clip",
            "title": "Find something I copied",
            "description": "Search your ClipTrail history for something you copied and get it ready to paste.",
            "arguments": [{ "name": "what", "description": "What you're looking for, e.g. 'the docker command from yesterday'", "required": true }]
        },
        {
            "name": "todays_copies",
            "title": "What did I copy today?",
            "description": "A short summary of what you copied today, grouped by kind and app."
        }
    ])
}

fn prompt(name: &str, args: &Value) -> Option<Value> {
    let text = match name {
        "find_clip" => {
            let what = args
                .get("what")
                .and_then(Value::as_str)
                .unwrap_or("something I copied");
            format!(
                "Find this in my ClipTrail clipboard history: {what}. Search with search_clips (try other words or filters if the first search misses), show me the best match with when and where I copied it, then put it on my clipboard with copy_to_clipboard."
            )
        }
        "todays_copies" => "Using my ClipTrail history (recent_clips from today), give me a short summary of what I copied today, grouped by kind and app. Leave out sensitive clips.".to_owned(),
        _ => return None,
    };
    Some(json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }))
}

pub fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// One inbound line → one optional outbound response.
/// `id` is null for notifications (no response) and per spec for parse errors.
pub fn handle(ctx: &Ctx, line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error_response(Value::Null, PARSE_ERROR, &e.to_string())),
    };
    handle_value(ctx, &msg)
}

pub fn handle_value(ctx: &Ctx, msg: &Value) -> Option<Value> {
    // JSON-RPC batches: respond to each element; pure notifications → None.
    if let Some(items) = msg.as_array() {
        // An empty batch is an invalid request: reply with an error so the
        // client isn't left waiting for a response that never comes.
        if items.is_empty() {
            return Some(error_response(Value::Null, INVALID_REQUEST, "empty batch"));
        }
        let responses: Vec<Value> = items.iter().filter_map(|m| handle_value(ctx, m)).collect();
        return if responses.is_empty() {
            None
        } else {
            Some(Value::Array(responses))
        };
    }

    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let is_notification = msg.get("id").is_none();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");

    // Notifications get no response by definition.
    if is_notification {
        return None;
    }

    let params = msg.get("params").cloned().unwrap_or(json!({}));
    match method {
        "initialize" => Some(result(
            id,
            json!({
                "protocolVersion": params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(PROTOCOL_VERSION),
                "capabilities": {
                    "tools": { "listChanged": false },
                    "prompts": { "listChanged": false }
                },
                "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
                "instructions": INSTRUCTIONS,
            }),
        )),
        "ping" => Some(result(id, json!({}))),
        "tools/list" => Some(result(id, json!({ "tools": tools::definitions() }))),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match tools::call(ctx, name, &args) {
                Ok(out) => Some(result(id, out)),
                Err(e) => Some(result(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": e.to_string() }],
                        "isError": true
                    }),
                )),
            }
        }
        "resources/list" => Some(result(id, json!({ "resources": [] }))),
        "resources/templates/list" => Some(result(id, json!({ "resourceTemplates": [] }))),
        "prompts/list" => Some(result(id, json!({ "prompts": prompt_list() }))),
        "prompts/get" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match prompt(name, &args) {
                Some(p) => Some(result(id, p)),
                None => Some(error_response(
                    id,
                    INVALID_PARAMS,
                    &format!("unknown prompt: {name}"),
                )),
            }
        }
        "logging/setLevel" => Some(result(id, json!({}))),
        "" => Some(error_response(id, INVALID_REQUEST, "missing method")),
        _ => Some(error_response(
            id,
            METHOD_NOT_FOUND,
            &format!("unknown method: {method}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipd_core::store::NewClip;
    use clipd_core::{Kind, now_millis};
    use clipd_core::{Pipeline, PipelineConfig, Store};
    use serde_json::json;
    use ulid::Ulid;

    fn seed(store: &Store, text: &str, kind: Kind, masked: bool) -> String {
        seed_with_id(store, &Ulid::generate().to_string(), text, kind, masked)
    }

    fn seed_with_id(store: &Store, id: &str, text: &str, kind: Kind, masked: bool) -> String {
        let id = id.to_string();
        store
            .upsert(NewClip {
                id: id.clone(),
                content_hash: id.as_bytes().to_vec(),
                kind,
                subkind: None,
                text: Some(text.to_string()),
                html: None,
                files: vec![],
                image_path: None,
                thumb_path: None,
                width: None,
                height: None,
                char_count: text.chars().count() as i64,
                line_count: text.lines().count() as i64,
                source_app: Some("chrome.exe".into()),
                device_id: "dev1".into(),
                created_at: now_millis(),
                masked,
                expires_at: None,
            })
            .unwrap();
        id
    }

    fn call(store: &Store, tool: &str, args: Value) -> Value {
        call_ctx(&Ctx::store_only(store), tool, args)
    }

    fn call_ctx(ctx: &Ctx, tool: &str, args: Value) -> Value {
        let resp = handle_value(
            ctx,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":tool,"arguments":args}}),
        )
        .unwrap();
        if resp["result"]["isError"] == true {
            panic!("tool error: {}", resp["result"]["content"][0]["text"]);
        }
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap_or(Value::String(text.to_string()))
    }

    #[test]
    fn initialize_echoes_version() {
        let store = Store::open_in_memory().unwrap();
        let resp = handle(&Ctx::store_only(&store),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        )
        .unwrap();
        assert_eq!(resp["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(resp["result"]["serverInfo"]["name"], "clipd-mcp");
    }

    #[test]
    fn notifications_have_no_response() {
        let store = Store::open_in_memory().unwrap();
        assert!(
            handle(
                &Ctx::store_only(&store),
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .is_none()
        );
    }

    #[test]
    fn tools_list_has_eleven_tools() {
        let store = Store::open_in_memory().unwrap();
        let resp = handle(
            &Ctx::store_only(&store),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        )
        .unwrap();
        assert_eq!(resp["result"]["tools"].as_array().unwrap().len(), 11);
    }

    #[test]
    fn search_and_get_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let id = seed(&store, "fn main() { println!(\"hi\"); }", Kind::Code, false);
        seed(&store, "https://example.com/docs", Kind::Link, false);

        let hits = call(&store, "search_clips", json!({"query": "println"}));
        assert_eq!(hits.as_array().unwrap().len(), 1);

        let hits = call(&store, "search_clips", json!({"kind": "link"}));
        assert_eq!(hits.as_array().unwrap().len(), 1);

        // Full id resolves (prefix resolution covered below).
        let clip = call(&store, "get_clip", json!({"id": id}));
        assert_eq!(clip["text"], "fn main() { println!(\"hi\"); }");
    }

    #[test]
    fn id_prefix_resolution_and_ambiguity() {
        let store = Store::open_in_memory().unwrap();
        seed_with_id(
            &store,
            "01AAAA00000000000000000000",
            "alpha",
            Kind::Text,
            false,
        );
        seed_with_id(
            &store,
            "01BBBB00000000000000000000",
            "beta",
            Kind::Text,
            false,
        );
        let clip = call(&store, "get_clip", json!({"id": "01AA"}));
        assert_eq!(clip["text"], "alpha");
        // Same-ms ULIDs share a timestamp prefix -> ambiguity is reported, not guessed.
        seed_with_id(
            &store,
            "01CCCD00000000000000000000",
            "g1",
            Kind::Text,
            false,
        );
        seed_with_id(
            &store,
            "01CCCE00000000000000000000",
            "g2",
            Kind::Text,
            false,
        );
        let resp = handle_value(
            &Ctx::store_only(&store),
            &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":"get_clip","arguments":{"id":"01CCC"}}}),
        )
        .unwrap();
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn masked_clip_never_exposes_body() {
        let store = Store::open_in_memory().unwrap();
        let id = seed(&store, "hunter2-secret-password", Kind::Sensitive, true);
        let clip = call(&store, "get_clip", json!({"id": id}));
        assert_eq!(clip["masked"], true);
        assert!(clip.get("text").is_none());
        assert_eq!(clip["preview"], "••••••");
    }

    #[test]
    fn pin_clip_toggles() {
        let store = Store::open_in_memory().unwrap();
        let id = seed(&store, "pin me", Kind::Text, false);
        let out = call(&store, "pin_clip", json!({"id": &id[..8], "pinned": true}));
        assert_eq!(out["pinned"], true);
        let hits = call(&store, "search_clips", json!({"pinned_only": true}));
        assert_eq!(hits.as_array().unwrap().len(), 1);
    }

    #[test]
    fn stats_and_day_counts() {
        let store = Store::open_in_memory().unwrap();
        seed(&store, "one", Kind::Text, false);
        seed(&store, "two", Kind::Text, false);
        let stats = call(&store, "clip_stats", json!({}));
        assert_eq!(stats["total"], 2);
        let days = call(&store, "day_counts", json!({}));
        assert!(!days.as_array().unwrap().is_empty());
        let apps = call(&store, "list_source_apps", json!({}));
        assert_eq!(apps, json!(["chrome.exe"]));
    }

    #[test]
    fn unknown_tool_is_tool_error() {
        let store = Store::open_in_memory().unwrap();
        let resp = handle_value(
            &Ctx::store_only(&store),
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":"nope","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn unknown_method_is_rpc_error() {
        let store = Store::open_in_memory().unwrap();
        let resp = handle(
            &Ctx::store_only(&store),
            r#"{"jsonrpc":"2.0","id":9,"method":"bogus/method"}"#,
        )
        .unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn parse_error_returns_32700() {
        let store = Store::open_in_memory().unwrap();
        let resp = handle(&Ctx::store_only(&store), "not json").unwrap();
        assert_eq!(resp["error"]["code"], -32700);
    }
    #[test]
    fn instructions_tell_the_assistant_when_to_look() {
        let store = Store::open_in_memory().unwrap();
        let resp = handle(
            &Ctx::store_only(&store),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .unwrap();
        let r = &resp["result"];
        assert!(
            r["instructions"]
                .as_str()
                .unwrap()
                .contains("whenever the user mentions")
        );
        assert!(r["capabilities"]["prompts"].is_object());
    }

    #[test]
    fn prompts_list_and_get() {
        let store = Store::open_in_memory().unwrap();
        let ctx = Ctx::store_only(&store);
        let list = handle(&ctx, r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list"}"#).unwrap();
        let names: Vec<&str> = list["result"]["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["find_clip", "todays_copies"]);
        let got = handle(
            &ctx,
            r#"{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"find_clip","arguments":{"what":"the docker command"}}}"#,
        )
        .unwrap();
        let text = got["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("the docker command") && text.contains("search_clips"));
        let bad = handle(
            &ctx,
            r#"{"jsonrpc":"2.0","id":3,"method":"prompts/get","params":{"name":"nope"}}"#,
        )
        .unwrap();
        assert_eq!(bad["error"]["code"], -32602);
    }

    #[test]
    fn save_to_history_stores_a_clip_from_the_assistant() {
        let dir = std::env::temp_dir().join(format!("clipd-mcp-save-{}", Ulid::generate()));
        let pipeline = Pipeline::open(PipelineConfig::with_defaults(dir.clone()).unwrap()).unwrap();
        let ctx = Ctx {
            store: pipeline.store(),
            pipeline: Some(&pipeline),
            device_id: String::new(),
            device_names: Vec::new(),
        };
        let out = call_ctx(
            &ctx,
            "save_to_history",
            json!({ "text": "docker compose up -d --build" }),
        );
        assert_eq!(out["saved"], true);
        let found = call_ctx(&ctx, "search_clips", json!({ "query": "compose up" }));
        assert_eq!(found[0]["source_app"], "AI assistant");
        let again = call_ctx(
            &ctx,
            "save_to_history",
            json!({ "text": "docker compose up -d --build" }),
        );
        assert_eq!(again["id"], out["id"]);
        // A bare store (no pipeline) can't save, and empty text is refused.
        let store = Store::open_in_memory().unwrap();
        let resp = handle_value(
            &Ctx::store_only(&store),
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":"save_to_history","arguments":{"text":"x"}}}),
        )
        .unwrap();
        assert_eq!(resp["result"]["isError"], true);
        drop(ctx);
        drop(pipeline);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn list_devices_names_them_and_device_filter_works() {
        let store = Store::open_in_memory().unwrap();
        seed(&store, "copied on the laptop", Kind::Text, false);
        let ctx = Ctx {
            store: &store,
            pipeline: None,
            device_id: "dev1".into(),
            device_names: vec![("dev1".into(), "Laptop".into())],
        };
        let devices = call_ctx(&ctx, "list_devices", json!({}));
        assert_eq!(devices[0]["name"], "Laptop");
        assert_eq!(devices[0]["this_device"], true);
        assert_eq!(devices[0]["clips"], 1);
        let mine = call_ctx(&ctx, "recent_clips", json!({ "device_id": "dev1" }));
        assert_eq!(mine.as_array().unwrap().len(), 1);
        let none = call_ctx(&ctx, "recent_clips", json!({ "device_id": "phone" }));
        assert!(none.as_array().unwrap().is_empty());
    }
}
