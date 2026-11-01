//! MCP stdio protocol: newline-delimited JSON-RPC 2.0.
//!
//! `handle` maps one incoming message to an optional response — notifications
//! and parse-level handling that produce no response return None. It is pure
//! (no I/O) so tests drive it without pipes.

use clipd_core::Store;
use serde_json::{Value, json};

use crate::tools;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "clipd-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;

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
pub fn handle(store: &Store, line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error_response(Value::Null, PARSE_ERROR, &e.to_string())),
    };
    handle_value(store, &msg)
}

pub fn handle_value(store: &Store, msg: &Value) -> Option<Value> {
    // JSON-RPC batches: respond to each element; pure notifications → None.
    if let Some(items) = msg.as_array() {
        // An empty batch is an invalid request: reply with an error so the
        // client isn't left waiting for a response that never comes.
        if items.is_empty() {
            return Some(error_response(Value::Null, INVALID_REQUEST, "empty batch"));
        }
        let responses: Vec<Value> = items
            .iter()
            .filter_map(|m| handle_value(store, m))
            .collect();
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
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
                "instructions": concat!(
                    "ClipTrail clipboard history on this machine. ",
                    "Use search_clips with kind/source_app/date filters; ",
                    "get_clip returns full text unless the clip is masked ",
                    "(sensitive) — masked clips never expose content."
                ),
            }),
        )),
        "ping" => Some(result(id, json!({}))),
        "tools/list" => Some(result(id, json!({ "tools": tools::definitions() }))),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match tools::call(store, name, &args) {
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
        "prompts/list" => Some(result(id, json!({ "prompts": [] }))),
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
        let resp = handle_value(
            store,
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
        let resp = handle(
            &store,
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
                &store,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .is_none()
        );
    }

    #[test]
    fn tools_list_has_eight_tools() {
        let store = Store::open_in_memory().unwrap();
        let resp = handle(
            &store,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        )
        .unwrap();
        assert_eq!(resp["result"]["tools"].as_array().unwrap().len(), 8);
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
            &store,
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
            &store,
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
            &store,
            r#"{"jsonrpc":"2.0","id":9,"method":"bogus/method"}"#,
        )
        .unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn parse_error_returns_32700() {
        let store = Store::open_in_memory().unwrap();
        let resp = handle(&store, "not json").unwrap();
        assert_eq!(resp["error"]["code"], -32700);
    }
}
