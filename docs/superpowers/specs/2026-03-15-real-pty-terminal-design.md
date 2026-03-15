# Real PTY Terminal Access for Praefectus

**Date:** 2026-03-15
**Status:** Proposed

## Problem

Praefectus currently spawns Claude Code with `-p` (print mode), piping stdin/stdout/stderr directly. This produces text-only output — no TUI rendering, no interactive tool approvals, no resize support. Users cannot interact with Claude Code sessions the way they would locally. Vigil and terminal access are separate, non-equivalent paths.

## Goal

Every Claude Code session should feel identical to running `claude` in a local terminal. Users can interact via the browser terminal OR via Vigil chat — both paths write to the same PTY and are fully interchangeable. Vigil manages session lifecycle, killing one-shot sessions when complete and leaving long-running ones open.

## Design

### 1. PTY Spawning Infrastructure

**Replace pipe-based spawning with real OS PTY allocation.**

The daemon currently uses `tokio::process::Command` with piped stdin/stdout/stderr. This changes to the `portable-pty` crate, which allocates real OS PTY pairs (`/dev/tty*` on macOS, `/dev/pts/*` on Linux).

#### Changes to `agent_spawner.rs`

- Use `portable_pty::native_pty_system()` to create a PTY pair (master + child).
- Spawn `claude` (no `-p` flag) as the child process inside the PTY slave.
- The master side provides a single read/write handle — no separate stdout/stderr.
- Initial prompt is sent by typing it into the PTY + pressing Enter, just like a human would.
- Terminal size (cols, rows) is set at spawn time from the frontend's reported dimensions.

#### Changes to PTY Manager (`pty_manager.rs`)

`PtyHandle` is restructured to hold real PTY handles:

```
PtyHandle {
    master_writer: Box<dyn Write + Send>,   // Write to PTY master (send keystrokes)
    child: Box<dyn Child + Send>,           // Child process handle
    alive: Arc<AtomicBool>,                 // Liveness flag
    pty: PtyPair,                           // For resize operations
}
```

Methods:
- `write(bytes)` — sends bytes directly to the PTY master (keystrokes into Claude Code)
- `resize(cols, rows)` — calls `pty.master.resize()` which delivers real `SIGWINCH` to the child
- `kill()` — sends SIGHUP to the child process group
- `is_alive()` — checks child process status via the `alive` flag

#### Changes to Output Manager (`output_manager.rs`)

- Reads from the PTY master (single byte stream, not separate stdout/stderr).
- Same broadcast channel + disk log architecture.
- Raw PTY output includes full ANSI escape sequences and TUI rendering codes.
- Buffer and disk log store raw bytes — xterm.js on the frontend interprets them.

#### New dependency

- `portable-pty` crate — cross-platform PTY allocation, well-maintained (used by Wezterm terminal emulator).

### 2. WebSocket Terminal Protocol

**Minimal protocol changes — the existing format is mostly sufficient.**

#### Client → Server messages

| Type | Payload | Change |
|------|---------|--------|
| `input` | `{ "type": "input", "data": "<raw_keystrokes>" }` | Unchanged — forwarded to PTY master |
| `resize` | `{ "type": "resize", "cols": N, "rows": N }` | Now wired to real `master.resize()` delivering `SIGWINCH` |

#### Server → Client messages

| Type | Payload | Change |
|------|---------|--------|
| `output` | `{ "type": "output", "data": "<raw_pty_bytes>" }` | Richer content — full TUI escape sequences |
| `pty_status` | `{ "type": "pty_status", "alive": bool }` | Unchanged |

#### Behavioral changes

- Resize triggers real `SIGWINCH` on the PTY child, so Claude Code's TUI reflows properly.
- Output is richer — full TUI rendering with panels, markdown, colors, cursor movement.
- Input is truly interactive — arrow keys, Ctrl+C, Tab completion, tool approval (y/n) all work natively.
- History replay still works — replays raw PTY bytes. xterm.js processes escape sequences sequentially, reconstructing the TUI state.
- WebSocket endpoint path stays the same: `/ws/terminal/{sessionId}`.

### 3. Vigil as PTY Client

**Vigil interacts with sessions by writing to the same PTY the human uses.**

#### Vigil sends input

Vigil calls an internal daemon method (in-process, not HTTP) that writes bytes to the PTY master. This is identical to what happens when the human types via the WebSocket.

| Action | What Vigil types |
|--------|-----------------|
| Answer a question | `<answer text>\n` |
| Give a new task | `<task description>\n` |
| Kill a session | `/exit\n` (or SIGHUP via PTY manager) |

When Vigil types into the PTY, it appears in the terminal exactly as if a human typed it. No hidden side channels.

#### Vigil reads session state

Two complementary channels:

1. **Structured events (primary):** Claude Code hooks fire events for tool use, session start/end, errors, blockers. Vigil uses these for decision-making — determining when a session needs input, when it's complete, when to escalate.

2. **Raw PTY output (secondary):** Vigil subscribes to the same output broadcast channel that WebSocket clients use. This gives full context when Vigil needs to understand what Claude Code is doing beyond what hooks report.

#### Vigil lifecycle management

- **One-shot tasks:** Vigil monitors for completion via hook events and sends `/exit\n` to the PTY when done.
- **Long-running sessions:** Vigil leaves them open; the user interacts freely.
- **Decision logic:** Vigil determines which mode to use based on task context and its strategy prompt.

### 4. Frontend Terminal UI

**Flexible terminal panel: starts embedded, can go full-screen.**

#### Layout states

| State | Description |
|-------|-------------|
| No session selected | Vigil chat takes full width (current behavior) |
| Panel mode | Vigil chat + terminal panel in a split view with resizable divider |
| Full-screen mode | Terminal takes over the entire viewport |

#### Transitions

- Clicking a session in SessionMonitor opens the terminal in **panel mode**.
- A maximize button switches to **full-screen mode**.
- A minimize button returns to **panel mode**.
- A close button dismisses the terminal panel entirely (back to Vigil-only).

#### Terminal panel header

- Session name/role label
- Connection status indicator (green = live, yellow = process ended)
- Minimize / Maximize / Close buttons
- "Vigil is active" indicator when Vigil is subscribed to the session

#### xterm.js changes

Minimal changes needed:
- Remove `-p` mode assumptions — sessions are always interactive while the process is alive.
- Send resize events on panel resize, maximize/minimize transitions.
- No more "read-only" state for running sessions. Read-only only applies after the process exits.
- Mobile virtual keyboard stays as-is.

#### Multiple sessions

- Only one terminal panel open at a time.
- Clicking a different session swaps the terminal (disconnect old WebSocket, connect new).
- SessionMonitor shows which session is currently viewed in the terminal.

### 5. Migration & Compatibility

#### Removed

- `-p` (print mode) spawning — all sessions use interactive PTY.
- Separate stdout/stderr reader tasks — replaced by single PTY master reader.
- Pipe-based `PtyHandle` struct — replaced with `portable-pty` based handle.
- "Read-only" terminal state for running sessions.

#### Preserved

- Output Manager disk log + broadcast architecture (same concept, different input source).
- WebSocket endpoint and message format (protocol-compatible).
- Hook event system (still fires, Vigil still consumes).
- Session database schema (no changes).
- Blocker cards in Vigil chat (rendered from hook events, but terminal is now also a reply option).
- Escalation service (driven by hooks, not PTY output).

#### New dependency

- `portable-pty` — cross-platform PTY allocation.

#### Risk areas

| Risk | Mitigation |
|------|------------|
| PTY output is a single byte stream (no stdout/stderr separation) | Not an issue — xterm.js handles raw terminal output natively |
| Claude Code TUI assumes terminal size at startup | Send correct dimensions from frontend at spawn time |
| History replay of raw PTY bytes may not perfectly reconstruct TUI | xterm.js processes escape sequences sequentially; trim to last N bytes for very long sessions |
| `portable-pty` compatibility on macOS | Well-tested — Wezterm uses it on macOS extensively |

## Non-Goals

- Mouse event forwarding (can be added later if needed).
- Multiple simultaneous terminal panels (one at a time is sufficient).
- SSH/remote access to sessions (browser-only for now).
- Changes to the session database schema.

## Testing Strategy

- **Unit tests:** PTY spawn, write, resize, kill operations against a simple shell process.
- **Integration tests:** Full WebSocket → PTY → xterm.js round-trip with a mock CLI process.
- **E2E tests:** Spawn a real `claude` session, verify TUI renders in xterm.js, send input, verify response.
- **Vigil integration tests:** Verify Vigil can write to PTY and read structured events simultaneously.
