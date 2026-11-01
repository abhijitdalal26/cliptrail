//! Seed a data dir with sample clips for manual MCP client testing:
//! `cargo run -p clipd-mcp --example seed -- /tmp/mcpdata`

use clipd_core::store::NewClip;
use clipd_core::{Kind, Store, now_millis};
use ulid::Ulid;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: seed <data_dir>");
    let store = Store::open(&std::path::Path::new(&dir).join("clips.db"))?;
    let samples = [
        (
            "https://github.com/abhijitdalal26/cliptrail",
            Kind::Link,
            "firefox",
        ),
        (
            "fn main() {\n    let store = Store::open(path)?;\n}",
            Kind::Code,
            "code",
        ),
        (
            "meeting notes: ship relay milestone friday",
            Kind::Text,
            "slack",
        ),
        (
            "password: correct-horse-fake-example",
            Kind::Sensitive,
            "terminal",
        ),
    ];
    for (text, kind, app) in samples {
        let masked = kind == Kind::Sensitive;
        store.upsert(NewClip {
            id: Ulid::generate().to_string(),
            content_hash: Ulid::generate().to_string().into_bytes(),
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
            source_app: Some(app.into()),
            device_id: "dev-seed".into(),
            created_at: now_millis(),
            masked,
            expires_at: None,
        })?;
    }
    println!("seeded {} clips into {dir}", samples.len());
    Ok(())
}
