# CLAUDE.md -- Instructions for Claude Code sessions

## Build and Run

```bash
cargo build              # Build the project
cargo run -- tui         # Launch the TUI (daemon + interactive terminal UI)
cargo run -- daemon      # Run headless daemon only (HTTP server on port 8000)
cargo test               # Run all tests
```

## Project Structure

Single Rust crate (binary + library):

- `src/lib.rs` -- Library crate root
- `src/main.rs` -- CLI entry point (clap)
- `src/tui/` -- Terminal UI (ratatui, tui-term, TEA architecture)
- `src/api/` -- Minimal HTTP API (health, events, vigil chat)
- `src/services/` -- Business logic (session-manager, vigil-manager, notifier, cleanup, recovery)
- `src/db/` -- SQLite schema and client (sqlx)
- `src/process/` -- PTY management, output capture
- `src/hooks/` -- Claude Code hook installation
- `src/events/` -- Event bus
- `src/config.rs` -- Config resolution (`Config::resolve()`)
- `src/deps.rs` -- Dependency injection container (`AppDeps`)
- `migrations/` -- SQLite migrations (run automatically on startup)

## Key Conventions

- **Rust (axum + ratatui)** -- TUI frontend, HTTP backend for hook ingestion
- **TEA architecture** -- The TUI follows The Elm Architecture (Model/Update/View)
- **SQLite (sqlx)** -- Database with compile-time checked queries
- **No Docker** -- Runs directly on macOS
- **Clippy pedantic** -- `clippy::pedantic` is warn-level

## API Endpoints (minimal — hook ingestion only)

- `GET /health` -- Health check
- `POST /events` -- Hook event ingestion
- `POST /api/vigil/chat` -- Vigil chat

## Testing Patterns

- Tests use isolated temp directories with in-memory SQLite
- Use `Config::for_testing(base)` for test configs
- **Bug fixes MUST include a regression test**
- **SOLID principles, clean code, TDD**

## Code Quality

- **MANDATORY PRE-COMMIT CHECKS** -- Before EVERY git commit, you MUST run: `cargo clippy` then `cargo test`. NO EXCEPTIONS.
- **Clippy pedantic** -- Always run `cargo clippy` before committing.
- **cargo fmt** -- Format code with `cargo fmt` before committing.
