//! clipd-mcp: MCP stdio server over the local ClipTrail store (see lib.rs).

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use clipd_core::default_data_dir;

#[derive(Parser)]
#[command(name = "clipd-mcp", about = "ClipTrail MCP server (stdio)")]
struct Cli {
    /// Data directory holding clips.db (default: platform data dir,
    /// override with CLIPTRAIL_DATA_DIR).
    #[arg(long, env = "CLIPTRAIL_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "clipd_mcp=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    clipd_mcp::serve(&cli.data_dir.unwrap_or_else(default_data_dir))
}
