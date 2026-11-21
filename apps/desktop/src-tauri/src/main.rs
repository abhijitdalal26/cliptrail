// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `cliptrail --mcp`: serve MCP on stdin/stdout for an AI assistant
    // (Settings → AI assistants) instead of starting the app.
    if std::env::args().skip(1).any(|a| a == "--mcp") {
        cliptrail_desktop_lib::run_mcp();
    }
    cliptrail_desktop_lib::run()
}
