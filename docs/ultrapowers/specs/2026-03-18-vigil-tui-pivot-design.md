# Vigil TUI Pivot — Design Spec

**Date:** 2026-03-18
**Status:** Draft
**Author:** Ennio Maldonado / Claude

## Summary

Pivot Vigil from a web-based tool (Next.js frontend + Rust daemon) to a terminal-only CLI tool with a ratatui TUI. The daemon becomes a single binary that runs background services, a minimal HTTP server (hook ingestion only), and the terminal UI. All agent orchestration is handled by the ultrapowers plugin ecosystem — pipelines are removed entirely.

## Motivation

- Ennio works primarily in the terminal
- The web dashboard adds complexity without proportional value for a single user
- Ultrapowers now handles all orchestration workflows (brainstorming → research → planning → implementation)
- Pipeline editor/runner is redundant with ultrapowers skill-driven orchestration

## Architecture

### Single Binary, Three Task Groups

```
vigil (single binary)
 ├── TUI task (ratatui event loop)
 │    ├── Render at 30fps (33ms tick)
 │    ├── Read crossterm keyboard events
 │    └── Read from app state channels
 │
 ├── HTTP task (axum::serve)
 │    ├── POST /events (hook ingestion from Claude Code)
 │    ├── GET /health
 │    └── POST /api/vigil/chat (Telegram relay)
 │
 └── Background services
      ├── SessionManager (event bus subscriber)
      ├── EscalationService (blocker timers)
      ├── MemoryDecayService (aging loop)
      └── VigilService (per-project overseer)
```

### State Sharing

TUI and services share state through the existing `AppDeps` dependency container. No IPC — the TUI reads directly from the same `Arc<Mutex<...>>` and broadcast channels that services already use.

```
AppDeps (Arc, shared across all tasks)
 ├── SessionStore      ← TUI reads for session list
 ├── OutputManager     ← TUI subscribes for terminal panes
 ├── VigilManager      ← TUI sends/receives chat messages
 ├── NotificationStore ← TUI reads for indicators
 ├── EventBus          ← TUI subscribes for real-time updates (defined in src/events.rs)
 └── Db + Config       ← Shared persistence
```

### App Architecture — Elm Architecture (TEA)

The TUI follows the Elm Architecture pattern:
- **Model** (`src/tui/state.rs`): AppState struct holding current view, selection, input buffer, session data
- **Update**: Message enum drives state transitions (KeyPress, SessionUpdated, OutputReceived, etc.)
- **View**: Pure render functions that take `&AppState` and draw to `Frame`

### TUI Event Loop

The TUI runs as the **main async tokio task** (not a dedicated OS thread). Uses `tokio::select!` to multiplex:

```rust
loop {
    tokio::select! {
        Some(event) = event_stream.next() => { /* crossterm key/mouse events */ }
        _ = tick_interval.tick() => { /* 30fps render tick */ }
        Ok(()) = state_rx.changed() => { /* session/notification state updates */ }
        _ = cancel_token.cancelled() => { break; }
    }
    terminal.draw(|frame| view(&model, frame))?;
}
```

**Why async, not a dedicated thread:** Ratatui maintainers explicitly recommend against `std::thread::spawn` and `tokio::task::spawn_blocking` for the TUI loop. Crossterm's `EventStream` is async-native. `spawn_blocking` permanently consumes a thread pool slot.

### Communication Channels

| Channel | Direction | Type | Purpose |
|---------|-----------|------|---------|
| `watch` | Services → TUI | `tokio::sync::watch` | State updates (sessions, notifications). TUI reads latest value each tick — intermediate states are skipped, which is correct for UI. |
| `mpsc` | TUI → Services | `tokio::sync::mpsc` | Commands (spawn session, send input, kill session). Ordered, buffered. |
| `CancellationToken` | Bidirectional | `tokio_util::sync` | Coordinated shutdown across TUI + axum + background tasks. |

### Logging

All logging goes to `~/.vigil/logs/vigil.log` via `tracing-appender`. ANSI disabled in log output. No `println!`, no stderr output — crossterm raw mode owns stdout/stderr exclusively. Stray writes corrupt the TUI.

### Terminal Restoration

`ratatui::init()` (since 0.28.1) automatically installs a panic hook that calls `ratatui::restore()` on crash. No manual hook needed. Use `color_eyre::install()?` BEFORE `ratatui::init()` for formatted error reports.

### Ctrl-C Handling

Crossterm raw mode captures `Ctrl-C` as a key event, not a signal. The TUI event loop handles it explicitly as equivalent to `q` (quit with confirmation). `tokio::signal::ctrl_c()` is installed as a safety net for external signals (e.g., `kill -2`).

## TUI Views

### Session List (default view, key: `1`)

- List of all sessions with status, project, duration
- `▌` thick left bar in accent color on selected row
- Status icons colored semantically: `●` green (running), `⚠` amber (blocked), `✓` muted (completed), `✗` red (failed)
- Blocked rows get subtle amber background tint
- Columns aligned with monospace tabular spacing
- Empty state: "No sessions running. Press c to chat with Vigil…"
- `Enter` opens selected session terminal, `↑↓` navigates

### Vigil Chat (key: `c`)

- Interactive chat with the Vigil overseer agent
- Vigil messages left-aligned, name in highlight color (purple)
- User messages right-aligned, name in accent color (cyan)
- Timestamps in muted text
- Input line at bottom with `›` prompt
- `Esc` returns to session list
- Vigil notifies inline when workers hit blockers

### Terminal Panes (key: `Enter` from session list)

- Opens selected session fullscreen
- `Enter` on another session from list adds it as a vertical split (side-by-side)
- Maximum 4 panes (2x2 grid). At limit, new session replaces the active pane
- Panes reflow on terminal resize — minimum 40 columns per pane, collapse to single if terminal is too narrow
- Active pane: title in accent, left border highlighted
- Inactive pane: title in muted
- `Tab` switches active pane, `Ctrl-D` closes pane
- `Esc` returns to session list
- Destructive actions (kill session) require confirmation: "Kill session #b7c1? y/n"
- Minimum terminal size: 80x24. Below this, show "Terminal too small…" message

#### PTY Output Rendering

Raw PTY output contains ANSI escape sequences, cursor movements, and color codes. The `tui-term` crate (built on `vt100`) parses PTY byte streams into a virtual terminal screen buffer, which is then rendered into ratatui cells. This is the bridge between raw PTY output and structured TUI widgets.

```toml
tui-term = "0.2"  # ANSI-aware terminal widget for ratatui
```

Each pane maintains its own `Arc<RwLock<vt100::Parser>>`. A background tokio task reads from the OutputManager broadcast subscription and feeds bytes to the parser via `parser.process(bytes)`. The render loop reads `parser.screen()` on each frame and passes it to `PseudoTerminal::new(screen)`. On pane resize, create a new parser with updated dimensions. tui-term re-exports vt100 — do not add vt100 as a separate dependency.

### Navigation

| Key | Action |
|-----|--------|
| `1` | Session list |
| `c` | Vigil chat |
| `Enter` | Open selected session terminal |
| `Tab` | Switch pane (in terminal view) |
| `Ctrl-D` | Close pane |
| `Esc` | Back to previous view |
| `q` | Quit (confirms if active sessions) |
| `?` | Help overlay |

## Visual Design

### Design Direction: Refined Command Center

High-contrast, purposeful color with restraint. Every color maps to a semantic token. Generous whitespace. Clean information hierarchy.

### Color Palette

| Token | Color | Meaning |
|-------|-------|---------|
| `bg` | `#0c0e14` | Canvas background |
| `surface` | `#161926` | Elevated panel backgrounds |
| `border` | `#252a3a` | Panel borders, dividers |
| `border-focus` | `#4fc3f7` | Focused panel border |
| `text` | `#dce0eb` | Primary body text |
| `text-muted` | `#636b83` | Timestamps, help text, secondary |
| `accent` | `#4fc3f7` | Interactive: cursor, selection, input, brand |
| `success` | `#66bb6a` | Running, completed, passing |
| `warning` | `#ffa726` | Blocked, needs input |
| `error` | `#ef5350` | Failed, errors |
| `highlight` | `#ab47bc` | Vigil's chat messages |

### Design Rules

- Never more than 3 semantic colors on screen at once (besides text + bg)
- Focus indicator: thick left bar `▌` in accent — consistent across all views
- Thin dividers (`───`, `│`) not heavy box-drawing — less visual noise
- One blank line between items — panels breathe
- `…` not `...` on loading states
- Error messages include the fix, not just the problem
- Active voice: "Press c to chat" not "Chat can be accessed"
- Empty states are first-class — always show helpful message with next action

## Scope: What Changes

### Deleted

| Component | Reason |
|-----------|--------|
| `apps/web/` | Entire Next.js frontend — replaced by TUI |
| `packages/shared/` | Zod schemas — Rust types are canonical |
| `package.json`, `package-lock.json` (root) | No more npm workspaces |
| `turbo.json` | No more Turborepo |
| `apps/daemon/` nesting | Flatten — Rust project moves to repo root |
| WebSocket endpoints | TUI reads OutputManager directly |
| Most API routes | Only `/events`, `/health`, `/api/vigil/chat` remain |
| Pipeline store + runner + execution | Ultrapowers handles orchestration |
| Pipeline DB tables | Drop `pipelines`, `pipeline_executions` |
| API modules: `middleware.rs`, `filesystem.rs`, `memory.rs`, `skills.rs`, `sub_sessions.rs`, `sessions.rs`, `projects.rs`, `notifications.rs`, `pipelines.rs`, `settings.rs`, `health.rs` | Frontend-only routes — handlers folded into `mod.rs` or deleted |
| `apps/daemon/src/e2e/` | E2E tests for deleted API routes — rewrite for TUI in Phase 4 |

### Unchanged

| Component | Reason |
|-----------|--------|
| SessionManager | Still processes hook events |
| AgentSpawner + PTY Manager | Still spawns Claude Code in worktrees |
| OutputManager | TUI subscribes to its broadcast channels |
| VigilManager + VigilService | Core of the chat interface |
| Telegram Notifier + Escalation | Away-from-desk alerts |
| Memory (SQLite + LanceDB + redb) | Persistence across restarts |
| Hook installation | Claude Code still POSTs to `/events` |
| Config resolution | `~/.vigil/` directory structure |

### Modified

| Component | Change |
|-----------|--------|
| `lib.rs` / `main.rs` | Add TUI startup, run ratatui alongside Axum |
| HTTP server | Strip to `/events`, `/health`, `/api/vigil/chat` |
| Database migrations | Drop pipeline tables |
| `deps.rs` | Remove pipeline deps, add TUI state |
| AgentSpawner | Remove skill installation logic (ultrapowers handles it) |

## Project Structure

```
vigil/
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── config.rs
│   ├── deps.rs
│   ├── events.rs
│   ├── api/
│   │   ├── mod.rs               # Router + health handler
│   │   ├── events.rs            # POST /events (hook ingestion)
│   │   └── vigil.rs             # POST /api/vigil/chat (Telegram relay)
│   ├── db/
│   │   ├── sqlite.rs
│   │   ├── models.rs
│   │   ├── lance.rs
│   │   └── kv.rs
│   ├── process/
│   │   ├── agent_spawner.rs
│   │   ├── pty_manager.rs
│   │   ├── output_manager.rs
│   │   └── output_extract.rs    # Output parsing/extraction
│   ├── services/
│   │   ├── session_manager.rs   # Hook event processing, session lifecycle
│   │   ├── session_store.rs     # Session CRUD (SQLite)
│   │   ├── project_store.rs     # Project registration
│   │   ├── notification_store.rs # Notification persistence
│   │   ├── settings_store.rs    # Telegram settings
│   │   ├── vigil_manager.rs     # Persistent Vigil PTY (sends/receives messages)
│   │   ├── vigil.rs             # Per-project overseer lifecycle, acta, memory extraction
│   │   ├── sub_session.rs       # Child session spawning
│   │   ├── escalation.rs        # Blocker timers
│   │   ├── notifier.rs          # Telegram notifications
│   │   ├── telegram_poller.rs   # Polls Telegram for incoming messages
│   │   ├── memory_store.rs      # Memory CRUD
│   │   ├── memory_search.rs     # Vector search (LanceDB)
│   │   ├── memory_decay.rs      # Background aging
│   │   ├── recovery.rs          # Session recovery on restart
│   │   ├── cleanup.rs           # Worktree cleanup
│   │   ├── lictor.rs             # Agent lifecycle guard
│   │   └── vigil_chat.rs        # Chat history persistence
│   └── tui/
│       ├── mod.rs
│       ├── state.rs
│       ├── views/
│       │   ├── session_list.rs
│       │   ├── chat.rs
│       │   └── terminal.rs
│       ├── widgets/
│       │   ├── status_badge.rs
│       │   ├── progress_bar.rs
│       │   └── help_overlay.rs
│       └── theme.rs
├── Cargo.toml
├── CLAUDE.md
├── LICENSE
└── README.md
```

### New Dependencies

```toml
ratatui = "0.30"                                      # TUI framework (includes crossterm 0.29 via re-export)
tui-term = "0.3"                                      # ANSI terminal emulator widget (vt100-backed)
tracing-appender = "0.2"                              # File-based logging (stdout is owned by TUI)
color-eyre = "0.6"                                    # Formatted error reports, integrates with ratatui
tokio-util = { version = "0.7", features = ["rt"] }   # CancellationToken for graceful shutdown
```

Do NOT add `crossterm` as a separate dependency — use `ratatui::crossterm` re-export to avoid version conflicts.

## Ultrapowers Integration

### First-Run Setup

When `vigil` detects no prior configuration, it runs a setup flow:

1. Prompt user to install ultrapowers plugin
2. If user selects "Install now":
   ```bash
   claude /plugin marketplace add ennio-datatide/ultrapowers
   claude /plugin install ultrapowers@ultrapowers
   claude /plugin install ultrapowers-dev@ultrapowers
   claude /plugin install ultrapowers-business@ultrapowers
   ```
3. If user selects "Already installed": verify `~/.claude/plugins/cache/ultrapowers/` exists
4. If user selects "Skip": warn that orchestration features will be limited

### Agent Spawning

AgentSpawner drops its skill installation logic entirely. Ultrapowers plugin is installed globally and Claude Code loads it automatically. No skill copying into worktrees.

### Vigil Orchestration

Vigil (the overseer agent) is a Claude Code agent with ultrapowers installed. It uses skills naturally:
- `ultrapowers:brainstorming` for design
- `ultrapowers:writing-plans` for planning
- `ultrapowers:subagent-driven-development` for execution
- `ultrapowers-dev:*` and `ultrapowers-business:*` for domain expertise

No special orchestration logic in Vigil's code — the skills handle routing.

## Graceful Shutdown

`Ctrl-C` or `q`:
1. If active sessions: confirm "2 sessions still running. Quit anyway? y/n"
2. `cancel_token.cancel()` — signals all tasks
3. Axum server drains via `.with_graceful_shutdown(cancel_token.cancelled())`
4. Active PTYs: SIGHUP → 5s timeout → SIGKILL
5. TUI loop breaks, `ratatui::restore()` restores terminal
6. SQLite connections close
7. Process exits

## Migration Plan

### Phase 1: Add TUI to existing daemon

1. Add `ratatui`, `crossterm`, `tui-term`, `tracing-appender` to `apps/daemon/Cargo.toml`
2. Create `src/tui/` module with stub views (session list, chat, terminal)
3. Add `theme.rs` with color palette
4. Wire TUI event loop into `main.rs` alongside existing Axum server
5. Verify: TUI renders session list, HTTP server still accepts hook events

### Phase 2: Strip HTTP surface

1. Remove WebSocket endpoints (`/ws/dashboard`, `/ws/terminal`)
2. Remove frontend-only API routes (most of `/api/*`)
3. Keep only: `POST /events`, `GET /health`, `POST /api/vigil/chat`
4. Remove pipeline store, runner, execution service
5. Add SQLite migration to drop `pipelines`, `pipeline_executions` tables
6. Verify: hook ingestion works, Telegram relay works, no regressions in services

### Phase 3: Flatten repo structure

1. `git mv apps/daemon/src apps/daemon/Cargo.toml apps/daemon/migrations apps/daemon/build.rs apps/daemon/.sqlx .` — preserves blame history for all daemon files
2. Move any remaining daemon config files (`rust-toolchain.toml`, etc.)
3. Delete `apps/web/`, `packages/shared/`, root `package.json`, `package-lock.json`, `turbo.json`
4. Update `Cargo.toml` paths
5. Update `CLAUDE.md`
6. Verify: `cargo build && cargo test` pass

### Phase 4: Polish TUI views

1. Implement full session list with live updates
2. Implement Vigil chat view with input
3. Implement terminal panes with `tui-term` PTY rendering
4. Add first-run setup flow (ultrapowers installation)
5. Add help overlay, confirmation dialogs
6. Verify: end-to-end flow — spawn session from chat, watch in terminal pane

### Database Migration

New SQLite migration added in Phase 2:

```sql
DROP TABLE IF EXISTS pipeline_executions;
DROP TABLE IF EXISTS pipelines;
```

Existing session, notification, memory, and vigil_chat tables are unchanged. No data loss for non-pipeline data.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| HTTP port already in use | Show error in TUI status bar: "Port 8000 in use. Kill other vigil instance or set VIGIL_PORT" |
| SQLite database locked/corrupt | Show error on startup: "Database error: {details}. Check ~/.vigil/vigil.db" and exit |
| PTY process crashes | SessionManager marks session as failed, TUI updates status to `✗`, notification sent |
| Terminal too small (<80x24) | Show centered message: "Terminal too small (need 80×24)…" — re-renders on resize |
| TUI panic | Panic hook restores terminal, logs stack trace to `~/.vigil/logs/vigil.log`, process exits |
| Vigil agent unresponsive (>600s) | EscalationService fires, Telegram notification sent, TUI shows warning on session |
| No internet (Telegram fails) | Notifier logs error, TUI continues normally, notifications stored locally |

## Testing Strategy

- **TUI rendering:** Ratatui supports `TestBackend` for headless rendering. Snapshot tests verify layout at standard terminal sizes (80x24, 120x40, 200x60)
- **Service tests:** Existing daemon tests (isolated temp dirs, in-memory SQLite) survive unchanged
- **Integration:** Test hook ingestion → session update → TUI state change end-to-end using `TestBackend`
- **PTY rendering:** Test `tui-term` widget with recorded ANSI output fixtures

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Pivot is irreversible (deleted frontend, dropped tables) | Work on a branch. Merge only after TUI is functional. Pipeline data is not critical — ultrapowers replaces it |
| `tui-term` crate may not handle all ANSI sequences | Fallback: raw text display with ANSI stripped. `tui-term` is actively maintained and widely used |
| Plugin CLI interface may change | Pin to known-working command format. First-run setup is a convenience, not a hard requirement |

## CLAUDE.md After Pivot

```markdown
## Build and Run

cargo build              # Build vigil
cargo run                # Start vigil (TUI + daemon)
cargo test               # Run all tests

## Project Structure

Single Rust crate — CLI tool with TUI and background daemon.

- src/tui/ — Terminal UI (ratatui + crossterm)
- src/api/ — Minimal HTTP server (hook ingestion only)
- src/services/ — Business logic (session manager, vigil, escalation, memory)
- src/process/ — PTY management (agent spawner, pty manager, output)
- src/db/ — Persistence (SQLite, LanceDB, redb)

## Key Conventions

- Rust (axum + ratatui) — Single binary, async with Tokio
- SQLite (sqlx) — Session, notification, memory persistence
- No frontend — Terminal UI only
- Ultrapowers — All agent orchestration uses ultrapowers skills
```
