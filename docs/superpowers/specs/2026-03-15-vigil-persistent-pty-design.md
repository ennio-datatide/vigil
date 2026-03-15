# Vigil as Persistent Interactive PTY Session

**Date:** 2026-03-15
**Status:** Proposed

## Problem

Vigil currently invokes `claude -p` (print mode) for every user message via `invoke_vigil()` in `claude_cli.rs`. Each call spawns a new process, runs the prompt, exits, and returns the response. This means:
- No persistent context — 6 messages of history are replayed every time
- Still uses pipe-based I/O despite the PTY refactor
- Every message creates a visible session with a terminal, even for simple orchestration
- The `vigil_cli_mutex` serializes all calls, creating a bottleneck

## Goal

Vigil becomes a single long-lived Claude Code instance running inside a real PTY. It starts when the daemon starts, stays alive, and user messages are written directly into its PTY. It maintains its own context naturally and calls MCP tools as needed. On death, it auto-restarts with recent history injected as context.

## Design

### 1. Vigil PTY Session

At daemon startup, after all services are initialized, the daemon spawns Vigil as an interactive Claude Code session inside a real PTY using the existing `portable-pty` infrastructure.

**Spawn configuration:**
- Uses `spawn_claude_pty()` (existing function in `agent_spawner.rs`) or a similar PTY allocator
- Args: `--mcp-config <path>`, `--append-system-prompt-file <strategy.md>`, `--verbose`, `--dangerously-skip-permissions`
- No `-p` flag — interactive mode
- `TERM=xterm-256color`, `env_remove("CLAUDE_CODE")`, `env_remove("CLAUDECODE")`, `env_remove("CLAUDE_CODE_ENTRYPOINT")`
- Working directory: daemon's working directory (Vigil is project-agnostic)

**Registration:**
- PTY handle registered in `PtyManager` under well-known ID `"vigil"`
- Output streams to `OutputManager` so the terminal UI can connect via `/ws/terminal/vigil`
- Vigil session registered in the DB as a special session (can be identified by a fixed ID or a `type` field)

### 2. Input Path (User Message → Vigil)

When a user sends a message via `POST /api/vigil/chat`:

1. `process_chat()` persists the user message to SQLite (unchanged)
2. Writes the message to the Vigil PTY: `pty_manager.write("vigil", format!("{message}\n").into_bytes())`
3. Waits for the response (see section 3)
4. Persists the Vigil response to SQLite (unchanged)
5. Returns the response to the HTTP handler

The PTY `stdin_tx` channel serializes writes naturally — no mutex needed.

### 3. Output Path (Vigil Response → User)

Vigil's PTY output is raw TUI bytes (ANSI, cursor movement). For the chat UI, we need clean text responses. Two mechanisms:

**Primary: Hook events.**
Claude Code fires hook events including `assistant_response` with structured payloads. The daemon already receives these via the hook ingestion endpoint (`POST /events`). When the hook fires for the Vigil session, the daemon extracts the response text.

**Flow:**
1. `process_chat()` writes user message to PTY
2. Subscribes to the event bus, filtering for hook events on the Vigil session
3. Waits for an `assistant_response` hook event (or equivalent completion signal)
4. Extracts the response text from the hook payload
5. Returns it to the caller
6. Timeout after 300 seconds — if no response, return an error message

**Implementation:**
- A `tokio::sync::oneshot` or `broadcast` channel per request
- `process_chat()` creates a receiver, registers it with the `VigilManager`, writes the message, then awaits the receiver
- When the hook event arrives, the `VigilManager` sends the response through the channel

### 4. VigilManager Service

New service that owns the Vigil PTY lifecycle. Located in `apps/daemon/src/services/vigil_manager.rs`.

**Responsibilities:**
- Spawn Vigil PTY at startup
- Monitor liveness and auto-restart on death
- Provide `send_message(text) -> Result<String>` method that writes to PTY and waits for response via hook events
- Manage response channels (map from request → oneshot sender)

**Struct:**
```
VigilManager {
    pty_manager: Arc<PtyManager>,
    output_manager: Arc<OutputManager>,
    event_bus: Arc<EventBus>,
    config: Arc<Config>,
    session_id: String,  // "vigil" or a UUID
    pending_responses: Mutex<VecDeque<oneshot::Sender<String>>>,
}
```

**Key method:**
```
async fn send_message(&self, message: &str) -> Result<String> {
    let (tx, rx) = oneshot::channel();
    self.pending_responses.lock().await.push_back(tx);
    self.pty_manager.write(&self.session_id, format!("{message}\n").into_bytes()).await?;
    tokio::time::timeout(Duration::from_secs(300), rx).await??
}
```

**Hook event listener:**
The VigilManager subscribes to the event bus. When an `assistant_response` hook event arrives for the Vigil session, it pops the oldest pending sender and sends the response text.

### 5. Lifecycle Management

**Startup:**
1. Write MCP config file (`~/.praefectus/vigil/mcp-config.json`)
2. Write strategy prompt file (`~/.praefectus/vigil/strategy.md`)
3. Allocate PTY, spawn `claude` with config
4. Register PTY handle as `"vigil"` in PtyManager
5. Register Vigil session in DB
6. Start exit monitor task

**Restart on death:**
1. Exit monitor detects `alive=false` (reader thread EOF)
2. Wait 2 seconds
3. Load last 10 chat messages from SQLite
4. Format as context prompt:
   ```
   You are resuming after a restart. Recent conversation:

   User: ...
   You: ...
   User: ...
   ```
5. Respawn Vigil PTY
6. Type context prompt into PTY as first input
7. Emit system message in chat: "Vigil restarted"

**Shutdown:**
- Daemon shutdown writes `/exit\n` to Vigil PTY
- Kill after 2-second grace period (drop master → SIGHUP → SIGKILL)

### 6. Terminal Access

- Vigil's session appears in SessionMonitor like any other session
- User can click it to watch Vigil think in real-time via the terminal panel
- User can type into Vigil's terminal directly — equivalent to chat input
- Both paths (chat UI and terminal) write to the same PTY

### 7. Removed Code

| What | Where | Why |
|------|-------|-----|
| `invoke_vigil()` | `process/claude_cli.rs` | Replaced by `VigilManager::send_message()` |
| `parse_json_output()` | `process/claude_cli.rs` | Responses come from hook events |
| `write_mcp_config()` | `process/claude_cli.rs` | Moves to `VigilManager` startup |
| `VigilCliResult` struct | `process/claude_cli.rs` | No longer needed |
| `vigil_cli_mutex` | `deps.rs` | PTY stdin_tx serializes naturally |
| History replay (lines 97-133) | `api/vigil.rs` | Vigil maintains own context |
| `claude_cli.rs` file | `process/` | Entire file deleted (only contained Vigil invocation logic) |

### 8. Preserved Code

| What | Where | Why |
|------|-------|-----|
| Chat persistence | `services/vigil_chat.rs` | Still save messages for UI and restart recovery |
| MCP tools | `mcp.rs` | All 7 tools unchanged |
| Strategy prompt | `prompts/vigil-strategy.md` | Loaded at startup via `--append-system-prompt-file` |
| Telegram poller | `services/telegram_poller.rs` | Calls `process_chat()` which now uses VigilManager |
| Hook ingestion | `POST /events` | Still receives hook events from Claude Code |

### 9. Frontend Changes

Minimal:
- Vigil session should appear in SessionMonitor with a distinguishing label (e.g., "Vigil" instead of a prompt snippet)
- Clicking Vigil session opens the terminal panel showing Vigil's TUI

No changes to the chat UI — it still sends messages to `POST /api/vigil/chat` and receives text responses.

## Testing Strategy

**Unit tests:**
- `VigilManager::send_message()` with a `/bin/cat` PTY child — write message, simulate hook event, verify response received
- Restart logic — kill Vigil PTY, verify auto-restart with context injection
- Timeout — no hook event within timeout, verify error returned

**Integration tests:**
- Full flow: POST /api/vigil/chat → message appears in PTY output → hook event fires → response returned
- Terminal WebSocket connects to Vigil session and receives output

**E2E tests:**
- Spawn real Vigil with Claude Code, send a message, verify MCP tool calls fire and response comes back
