# Research Brief: Vigil TUI Pivot

**Date:** 2026-03-18

## Context

Research for pivoting Vigil from a web UI to a terminal-only TUI using ratatui, running alongside an axum HTTP server in a single Rust binary. Covers framework versions, app architecture, PTY rendering, threading, and terminal safety.

## Key Findings

### 1. Ratatui — Use v0.30.0 (not 0.29)

**Finding:** Ratatui 0.30.0 is the latest stable release. It introduced workspace modularization, no_std support, and several breaking changes from 0.29.

**Spec impact — version bump required:**
- `ratatui = "0.30"` (not 0.29)
- crossterm 0.29 is the default backend (via `ratatui-crossterm`)
- Use `ratatui::crossterm` re-export — never add crossterm as a separate dependency

**Breaking changes from 0.29 → 0.30 that affect us:**
- `block::Title` removed — use `Line` instead
- `layout::Alignment` renamed to `HorizontalAlignment`
- `Style` no longer implements `Stylize` — methods defined directly on `Style`
- `TestBackend` error type changed from `io::Error` to `Infallible`

**RGB colors:** Fully supported via `Color::Rgb(r, g, b)` and `Color::from_str("#4fc3f7")`. Our entire color palette works.

**Sources:** ratatui.rs, docs.rs/ratatui, GitHub releases, BREAKING-CHANGES.md

### 2. App Architecture — Use Elm Architecture (TEA)

**Finding:** Ratatui is unopinionated but recommends three patterns. The Elm Architecture (Model/Update/View) is the most widely used and best documented.

**Recommended pattern for Vigil:**
```
Model (AppState) → Update (handle events, produce state changes) → View (render from state)
```

Main loop:
1. `terminal.draw(|frame| view(&model, frame))`
2. Poll events via crossterm `EventStream` (async)
3. Map events to messages
4. `update(&mut model, message)`
5. Check exit condition

**Spec impact:** Add TEA as the app architecture pattern. The `src/tui/state.rs` becomes the Model, views are pure render functions, and a message enum drives state transitions.

**Sources:** ratatui.rs/concepts/application-patterns/the-elm-architecture/, async-template

### 3. Threading — Tokio Task, NOT Dedicated OS Thread

**Finding:** Ratatui maintainers explicitly recommend against `std::thread::spawn` and `tokio::task::spawn_blocking` for the TUI loop. The correct pattern is an async tokio task using `tokio::select!` with crossterm's `EventStream`.

**Spec impact — must change threading model:**
- Remove: "TUI event loop runs on a dedicated OS thread"
- Replace with: TUI runs as main async task using `tokio::select!` over `EventStream`, tick intervals, render intervals, and shutdown signal
- Axum server spawned via `tokio::spawn`
- Communication: `watch` channels for state (server → TUI), `mpsc` for commands (TUI → server)
- Shutdown: `CancellationToken` from `tokio-util`

**Why:** `spawn_blocking` permanently consumes a thread pool slot and can't be aborted. `std::thread::spawn` loses tokio integration. Async `EventStream` is the designed pattern.

**Novel architecture:** No known open-source project combines ratatui + axum in a single binary. Closest reference is the ratatui async template.

**Sources:** forum.ratatui.rs, ratatui.rs/tutorials/counter-async-app/, tokio.rs/tokio/topics/shutdown

### 4. PTY Rendering — tui-term v0.3.2 Confirmed

**Finding:** tui-term v0.3.2 is purpose-built for this. Uses vt100 crate internally. Compatible with ratatui 0.30.

**How it works:**
1. Create `vt100::Parser::new(rows, cols, scrollback_len)`
2. Feed raw PTY bytes via `parser.process(bytes)` (or `Write` trait)
3. Render: `PseudoTerminal::new(parser.screen()).block(block)`
4. tui-term re-exports vt100 for version safety

**ANSI support:** Colors (indexed + likely truecolor), bold/dim/italic/underline/inverse, cursor positioning, alternate screen, scrollback, mouse protocol.

**Limitations:**
- Marked "work in progress" but stable enough for our use
- Input handling is our responsibility (already handled by PTY stdin)
- Must manually resize `vt100::Parser` when pane/terminal resizes
- `controller` module is experimental (we don't need it)

**Spec impact:** Use `tui-term = "0.3"` (not 0.2). Each terminal pane maintains its own `Arc<RwLock<vt100::Parser>>`. A background task reads from OutputManager and feeds bytes to the parser. The render loop reads `parser.screen()` on each frame.

**Alternatives considered:**
- vt100 directly — same thing but more boilerplate
- alacritty_terminal — too heavy, not designed for embedding
- vte crate — parser only, no screen state

**Sources:** docs.rs/tui-term, github.com/a-kenji/tui-term, docs.rs/vt100

### 5. Panic Recovery — Built Into ratatui::init()

**Finding:** Since ratatui 0.28.1, `ratatui::init()` automatically installs a panic hook that restores the terminal. No manual panic hook needed.

**Spec impact:** Remove the manual panic hook code from the spec. Just use `ratatui::init()` and `ratatui::restore()`.

**Enhanced error handling:** Use `color-eyre` for formatted error reports. Install with `color_eyre::install()?` BEFORE `ratatui::init()`.

**Ctrl-C:** Captured as a key event in raw mode (not SIGINT). Handle in the event loop as quit-with-confirmation. Install `tokio::signal::ctrl_c()` as safety net for external signals only.

**Sources:** ratatui.rs/recipes/apps/panic-hooks/, ratatui.rs/recipes/apps/color-eyre/

### 6. Logging — File Only, Never Stdout

**Finding:** Confirmed: any stdout/stderr writes corrupt the TUI. Must route all logging to files.

**Recommended setup:**
- `tracing` with `tracing-appender` writing to `~/.vigil/logs/vigil.log`
- Disable ANSI in log output (file doesn't need colors)
- Optional: `tui-logger` crate to display logs inside the TUI itself (could be useful for debugging)

**Sources:** ratatui.rs/recipes/apps/log-with-tracing/, tui-logger crate

## Recommended Dependency Updates

Based on research, the spec's dependency section should be:

```toml
# TUI
ratatui = "0.30"            # was 0.29 — latest stable, built-in panic hooks
tui-term = "0.3"            # was 0.2 — compatible with ratatui 0.30
tracing-appender = "0.2"    # unchanged

# Error handling
color-eyre = "0.6"          # NEW — formatted errors, integrates with ratatui

# Shutdown coordination
tokio-util = { version = "0.7", features = ["rt"] }  # NEW — CancellationToken
```

No need to add crossterm separately — use `ratatui::crossterm` re-export.

## Recommended Spec Changes

| Section | Change |
|---------|--------|
| Dependencies | ratatui 0.30, tui-term 0.3, add color-eyre and tokio-util |
| TUI Threading | Remove dedicated OS thread. Use async tokio task with `EventStream` + `tokio::select!` |
| Communication | Add `watch` (state) + `mpsc` (commands) + `CancellationToken` (shutdown) |
| Panic Recovery | Remove manual panic hook. Use `ratatui::init()` built-in hooks + `color-eyre` |
| App Architecture | Specify Elm Architecture (Model/Update/View) pattern |
| Terminal Panes | Each pane: `Arc<RwLock<vt100::Parser>>`, resize parser on pane resize |
| Graceful Shutdown | Use `CancellationToken`, axum `.with_graceful_shutdown()` |

## Sources

- [Ratatui Official Docs](https://ratatui.rs/)
- [Ratatui GitHub](https://github.com/ratatui/ratatui)
- [Ratatui Async Template](https://github.com/ratatui/async-template)
- [Ratatui Forum — tokio::spawn vs spawn_blocking](https://forum.ratatui.rs/t/understanding-tokio-spawn-and-tokio-spawn-blocking/74)
- [tui-term GitHub](https://github.com/a-kenji/tui-term)
- [tui-term docs.rs](https://docs.rs/tui-term/latest/tui_term/)
- [vt100 docs.rs](https://docs.rs/vt100)
- [Axum Graceful Shutdown Example](https://github.com/tokio-rs/axum/blob/main/examples/graceful-shutdown/src/main.rs)
- [Tokio Graceful Shutdown Guide](https://tokio.rs/tokio/topics/shutdown)
- [Ratatui Panic Hooks](https://ratatui.rs/recipes/apps/panic-hooks/)
- [Ratatui color-eyre Integration](https://ratatui.rs/recipes/apps/color-eyre/)
- [Ratatui Logging with Tracing](https://ratatui.rs/recipes/apps/log-with-tracing/)
