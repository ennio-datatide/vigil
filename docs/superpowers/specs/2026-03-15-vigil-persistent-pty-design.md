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
- Uses `spawn_claude_pty()` (existing function in `agent_spawner.rs`) or a dedicated PTY allocator in `VigilManager`
- Args: `--mcp-config <path>`, `--append-system-prompt-file <strategy.md>`, `--verbose`, `--dangerously-skip-permissions`, `--tools ""`
- `--tools ""` disables all built-in tools (Bash, Read, Write, etc.) so Vigil can only use its 7 MCP tools — same as the current `invoke_vigil()` behavior
- No `-p` flag — interactive mode
- `TERM=xterm-256color`, `env_remove("CLAUDE_CODE")`, `env_remove("CLAUDECODE")`, `env_remove("CLAUDE_CODE_ENTRYPOINT")`
- Working directory: `~/.praefectus/vigil/` — a dedicated directory for Vigil (not the daemon's CWD, which may have conflicting `.claude/` config)

**Hook installation:**
- Before spawning, call `HookInstaller::install("~/.praefectus/vigil/", "vigil", port)` to write hook scripts and `.claude/settings.json` into the Vigil working directory
- This ensures Claude Code emits hook events (including `Stop`) with the correct session ID
- Without this step, no hook events would fire and response detection would silently fail

**Registration:**
- PTY handle registered in `PtyManager` under well-known ID `"vigil"`
- Output streams to `OutputManager` so the terminal UI can connect via `/ws/terminal/vigil`
- Vigil session registered in the DB as a special session (can be identified by a fixed ID or a `type` field)

**Readiness detection:**
- After spawning, `VigilManager` waits for the first `Stop` hook event before accepting user messages
- This ensures Claude Code has finished its interactive startup (TUI initialization, system prompt loading) and is ready for input
- If no `Stop` event arrives within 30 seconds, log a warning and proceed anyway (best-effort)

### 2. Input Path (User Message → Vigil)

When a user sends a message via `POST /api/vigil/chat`:

1. `process_chat()` persists the user message to SQLite (unchanged)
2. Checks `VigilManager::is_busy()` — if another message is in-flight, return HTTP 503 "Vigil is processing another message"
3. Calls `VigilManager::send_message(text)` which writes the message to the Vigil PTY
4. Waits for the response (see section 3)
5. Persists the Vigil response to SQLite (unchanged)
6. Returns the response to the HTTP handler

**Concurrency control:** Only one message can be in-flight at a time. The `VigilManager` tracks a `busy: AtomicBool` flag. This replaces the old `vigil_cli_mutex` with a non-blocking check — callers get an immediate 503 instead of queuing behind a mutex. This is necessary because Claude Code's interactive mode processes one message at a time; interleaving would corrupt the conversation.

### 3. Output Path (Vigil Response → User)

Vigil's PTY output is raw TUI bytes (ANSI, cursor movement). For the chat UI, we need clean text responses.

**Response detection via hook events:**
Claude Code fires hook events at various points. The relevant terminal event is `Stop`, which fires once per turn completion (after all MCP tool calls finish). The `Stop` hook payload contains the assistant's response text.

**Flow:**
1. `VigilManager::send_message()` sets `busy=true`, creates a `oneshot::channel`
2. Stores the sender in `pending_response: Option<oneshot::Sender<String>>`
3. Writes the user message to the Vigil PTY
4. Awaits the receiver with a 600-second timeout
5. On completion, sets `busy=false` and returns the response

**Why 600 seconds:** Vigil may call `spawn_worker(wait: true)` which polls for up to 240 seconds, plus the worker's execution time, plus MCP overhead. A 300-second timeout would be paper-thin. 600 seconds provides sufficient margin.

**Hook event listener:**
The VigilManager subscribes to the event bus. When a `Stop` hook event arrives for the Vigil session ID:
1. Extract the response text from the hook payload
2. If `pending_response` has a sender, send the response and clear it
3. If no sender (direct terminal input, or event after timeout), ignore

**In-flight request on Vigil death:**
If Vigil dies while a message is in-flight, the reader thread sets `alive=false`. The VigilManager detects this, sends an error through the pending channel ("Vigil crashed, restarting..."), and proceeds to restart.

### 4. VigilManager Service

New service that owns the Vigil PTY lifecycle. Located in `apps/daemon/src/services/vigil_manager.rs`.

**Responsibilities:**
- Spawn Vigil PTY at startup (including hook installation, MCP config, strategy prompt)
- Monitor liveness and auto-restart on death
- Provide `send_message(text) -> Result<String>` method that writes to PTY and waits for response
- Track busy state for concurrency control
- Handle hook events to extract responses

**Struct:**
```
VigilManager {
    pty_manager: Arc<PtyManager>,
    output_manager: Arc<OutputManager>,
    event_bus: Arc<EventBus>,
    config: Arc<Config>,
    db: Arc<SqliteDb>,
    session_id: String,  // "vigil"
    busy: AtomicBool,
    pending_response: Mutex<Option<oneshot::Sender<String>>>,
    vigil_dir: PathBuf,  // ~/.praefectus/vigil/
}
```

**Key method:**
```
async fn send_message(&self, message: &str) -> Result<String> {
    if self.busy.swap(true, Ordering::Acquire) {
        return Err(anyhow!("Vigil is processing another message"));
    }

    let (tx, rx) = oneshot::channel();
    *self.pending_response.lock().await = Some(tx);

    self.pty_manager.write(&self.session_id, format!("{message}\n").into_bytes()).await?;

    let result = tokio::time::timeout(Duration::from_secs(600), rx).await;
    self.busy.store(false, Ordering::Release);

    match result {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(_)) => Err(anyhow!("Vigil session died while processing")),
        Err(_) => Err(anyhow!("Vigil response timeout (600s)")),
    }
}
```

**MCP config and strategy prompt:**
- `write_mcp_config()` moves from `claude_cli.rs` to `VigilManager` (or a private method on it)
- `daemon_url` is derived from `config.server_port`: `format!("http://localhost:{}", config.server_port)`
- Strategy prompt is copied from `prompts/vigil-strategy.md` to `~/.praefectus/vigil/strategy.md`

### 5. Lifecycle Management

**Startup:**
1. Create `~/.praefectus/vigil/` directory if it doesn't exist
2. Write MCP config file to `~/.praefectus/vigil/mcp-config.json`
3. Write strategy prompt file to `~/.praefectus/vigil/strategy.md`
4. Install hook scripts via `HookInstaller::install("~/.praefectus/vigil/", "vigil", port)`
5. Allocate PTY, spawn `claude` with config args, working directory `~/.praefectus/vigil/`
6. Register PTY handle as `"vigil"` in PtyManager
7. Register Vigil session in DB
8. Wait for readiness (first `Stop` event or 30-second timeout)
9. Start exit monitor task

**Restart on death:**
1. Exit monitor detects `alive=false` (reader thread EOF)
2. Cancel any in-flight `pending_response` with an error message
3. Wait 2 seconds
4. Load last 10 chat messages from SQLite
5. Format as context prompt:
   ```
   You are resuming after a restart. Recent conversation:

   User: ...
   You: ...
   User: ...
   ```
6. Respawn Vigil PTY (steps 1-8 from Startup, skipping file writes if already present)
7. Wait for readiness
8. Type context prompt into PTY as first input
9. Emit system message in chat: "Vigil restarted"

**Shutdown:**
- Daemon shutdown writes `/exit\n` to Vigil PTY
- Kill after 2-second grace period (drop master → SIGHUP → SIGKILL)

### 6. Terminal Access

- Vigil's session appears in SessionMonitor like any other session, with a "Vigil" label
- User can click it to watch Vigil think in real-time via the terminal panel
- Direct terminal input bypasses `process_chat()` — it writes to the PTY but the response is NOT persisted to chat history. This is by design: the terminal is for observation and debugging, the chat UI is the primary interface.

### 7. Strategy Prompt Changes

The current strategy prompt (`prompts/vigil-strategy.md`) contains stateless framing like "You have no knowledge" which was necessary when each message was a fresh `claude -p` invocation. In persistent mode, Vigil retains its full conversation context, so this language must be updated:

- Remove "You have no knowledge" / "You are stateless" language
- Add "You maintain conversation context across messages"
- Keep all delegation rules (always spawn_worker, never answer directly)
- Keep all MCP tool descriptions and decision logic

### 8. Removed Code

| What | Where | Why |
|------|-------|-----|
| `invoke_vigil()` | `process/claude_cli.rs` | Replaced by `VigilManager::send_message()` |
| `parse_json_output()` | `process/claude_cli.rs` | Responses come from hook events |
| `VigilCliResult` struct | `process/claude_cli.rs` | No longer needed |
| `vigil_cli_mutex` | `deps.rs` | Replaced by `VigilManager::busy` flag |
| History replay (lines 97-133) | `api/vigil.rs` | Vigil maintains own context |
| `claude_cli.rs` file | `process/` | Entire file deleted — `write_mcp_config()` moves to `vigil_manager.rs` |

### 9. Preserved Code

| What | Where | Why |
|------|-------|-----|
| Chat persistence | `services/vigil_chat.rs` | Still save messages for UI and restart recovery |
| MCP tools | `mcp.rs` | All 7 tools unchanged |
| Telegram poller | `services/telegram_poller.rs` | Calls `process_chat()` which now uses VigilManager |
| Hook ingestion | `POST /events` | Still receives hook events from Claude Code |

### 10. Frontend Changes

Minimal:
- Vigil session should appear in SessionMonitor with a distinguishing label (e.g., "Vigil" instead of a prompt snippet)
- Clicking Vigil session opens the terminal panel showing Vigil's TUI
- `POST /api/vigil/chat` returns HTTP 503 when Vigil is busy — frontend should show a "Vigil is busy" indicator

No changes to the chat UI message flow — it still sends messages to `POST /api/vigil/chat` and receives text responses.

## Testing Strategy

**Unit tests:**
- `VigilManager::send_message()` with a `/bin/cat` PTY child — write message, simulate `Stop` hook event, verify response received
- Busy flag — second concurrent `send_message()` returns error immediately
- Restart logic — kill Vigil PTY, verify in-flight request gets error, auto-restart fires, context injection works
- Timeout — no hook event within timeout, verify error returned and busy flag cleared

**Integration tests:**
- Full flow: POST /api/vigil/chat → message appears in PTY output → `Stop` hook event fires → response returned
- Terminal WebSocket connects to Vigil session and receives output
- Concurrent POST returns 503

**E2E tests:**
- Spawn real Vigil with Claude Code, send a message, verify MCP tool calls fire and response comes back
