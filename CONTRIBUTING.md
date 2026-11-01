# Contributing to ClipTrail

Thanks for your interest! ClipTrail is in early days — the fastest ways to help right now:

- **Read the docs** in [`docs/`](docs/) and open issues with feedback, use-cases, or disagreements.
- **Pick a `good first issue`** — we label newcomer-friendly work.
- **Platform expertise wanted:** Wayland clipboard capture, Android IME/lifecycle quirks, macOS pasteboard internals, Windows focus/foreground behavior.

## Ground rules

- The clipboard sees passwords. Any code path that could leak clip content (telemetry, logging, sync, error reporting) is reviewed with maximum skepticism. When in doubt, don't capture it.
- No telemetry, no analytics, no phone-home. Ever.
- Dependencies must be MIT/Apache/BSD-compatible — CI runs `cargo deny` to enforce this. No GPL deps.

## Dev setup

_Coming with the first code drop._ Planned: Rust toolchain (`rust-toolchain.toml` pins it), `cargo test` at the workspace root, and `docs/architecture.md` as the tour of the codebase.

## Pull requests

- Keep PRs focused; describe the "why" in the body.
- Run `cargo fmt`, `cargo clippy`, and `cargo test` before pushing — CI enforces all three.
- See `docs/build-plan.md` for the monorepo layout before adding crates or apps.
