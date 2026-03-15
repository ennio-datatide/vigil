# Vigil Persistent PTY — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Vigil's fire-and-forget `claude -p` calls with a single persistent interactive PTY session that stays alive for the daemon's lifetime.

**Architecture:** New `VigilManager` service spawns Vigil as a persistent PTY at daemon startup. User messages are written to the PTY, responses detected via `Stop` hook events. Auto-restart on death with context recovery from chat history. `claude_cli.rs` is deleted entirely.

**Tech Stack:** portable-pty (existing), tokio oneshot channels, axum event bus (existing)

**Spec:** `docs/superpowers/specs/2026-03-15-vigil-persistent-pty-design.md`

---

## File Structure

### Rust Daemon (apps/daemon/)

| File | Action | Responsibility |
|------|--------|---------------|
| `src/services/vigil_manager.rs` | Create | Owns Vigil PTY lifecycle: spawn, restart, send_message, hook listener |
| `src/api/vigil.rs` | Modify | Simplify process_chat() to use VigilManager::send_message() |
| `src/deps.rs` | Modify | Add vigil_manager field, remove vigil_cli_mutex |
| `src/lib.rs` | Modify | Start VigilManager at daemon startup |
| `src/process/claude_cli.rs` | Delete | Entire file — invoke_vigil() replaced by VigilManager |
| `src/process/mod.rs` or `lib.rs` | Modify | Remove `claude_cli` module declaration |
| `src/prompts/vigil-strategy.md` | Modify | Remove stateless framing |

### Frontend (apps/web/)

| File | Action | Responsibility |
|------|--------|---------------|
| `src/components/vigil/session-monitor.tsx` | Modify | Show "Vigil" label for Vigil session |
| `src/components/vigil/vigil-chat.tsx` | Modify | Handle 503 busy response |

---

## Chunk 1: VigilManager Service

### Task 1: Create VigilManager struct and spawn logic

**Files:**
- Create: `apps/daemon/src/services/vigil_manager.rs`

- [ ] **Step 1: Create the VigilManager struct**

```rust
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, oneshot};

use crate::config::Config;
use crate::db::sqlite::SqliteDb;
use crate::events::{AppEvent, EventBus};
use crate::process::output_manager::OutputManager;
use crate::process::pty_manager::{PtyHandle, PtyManager};

pub(crate) struct VigilManager {
    pty_manager: Arc<PtyManager>,
    output_manager: Arc<OutputManager>,
    event_bus: Arc<EventBus>,
    config: Arc<Config>,
    db: Arc<SqliteDb>,
    session_id: String,
    busy: AtomicBool,
    pending_response: Mutex<Option<oneshot::Sender<String>>>,
    vigil_dir: PathBuf,
}
```

- [ ] **Step 2: Implement new() and spawn_vigil()**

`new()` takes `AppDeps` reference, derives `vigil_dir` from `config.praefectus_home / "vigil"`.

`spawn_vigil()` does:
1. Create `vigil_dir` if missing
2. Write MCP config (moved from `claude_cli.rs:write_mcp_config`)
3. Copy strategy prompt to `vigil_dir/strategy.md`
4. Install hooks via `HookInstaller::install(&vigil_dir, &session_id, port)`
5. Allocate PTY via `portable_pty::native_pty_system().openpty()`
6. Spawn `claude` with args: `--mcp-config`, `--append-system-prompt-file`, `--verbose`, `--dangerously-skip-permissions`, `--tools ""`
7. Wire I/O via `wire_pty_io()` pattern (spawn_blocking reader/writer)
8. Register PTY handle in PtyManager under `session_id`

Use the same PTY spawning pattern from `agent_spawner.rs:spawn_claude_pty()` but with different args (no `-p`, add `--mcp-config`, `--tools ""`).

- [ ] **Step 3: Add module declaration**

In `apps/daemon/src/services/mod.rs` (or wherever services are declared), add:
```rust
pub(crate) mod vigil_manager;
```

- [ ] **Step 4: Verify it compiles**

Run: `cd apps/daemon && cargo check`

- [ ] **Step 5: Commit**

```bash
git add apps/daemon/src/services/vigil_manager.rs apps/daemon/src/services/mod.rs
git commit -m "feat: add VigilManager struct with PTY spawn logic"
```

---

### Task 2: Implement send_message() with hook-based response detection

**Files:**
- Modify: `apps/daemon/src/services/vigil_manager.rs`

- [ ] **Step 1: Implement send_message()**

```rust
pub(crate) async fn send_message(&self, message: &str) -> anyhow::Result<String> {
    // Check busy flag — return error if another message is in-flight
    if self.busy.swap(true, Ordering::Acquire) {
        return Err(anyhow::anyhow!("Vigil is processing another message"));
    }

    // Create response channel
    let (tx, rx) = oneshot::channel();
    *self.pending_response.lock().await = Some(tx);

    // Write message to Vigil PTY
    self.pty_manager
        .write(&self.session_id, format!("{message}\n").into_bytes())
        .await?;

    // Wait for Stop hook event with 600s timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(600),
        rx,
    ).await;

    self.busy.store(false, Ordering::Release);

    match result {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(_)) => Err(anyhow::anyhow!("Vigil session died while processing")),
        Err(_) => {
            // Timeout — clear pending response
            *self.pending_response.lock().await = None;
            Err(anyhow::anyhow!("Vigil response timeout (600s)"))
        }
    }
}
```

- [ ] **Step 2: Implement start_hook_listener()**

Subscribe to the event bus. When a `HookEvent` with `event_type == "Stop"` arrives for the Vigil session, extract the response text from the payload and send it through the pending channel.

```rust
pub(crate) fn start_hook_listener(self: &Arc<Self>) {
    let this = Arc::clone(self);
    let mut rx = this.event_bus.subscribe();

    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let AppEvent::HookEvent { session_id, event_type, payload } = &event {
                if session_id == &this.session_id && event_type == "Stop" {
                    // Extract response from Stop payload
                    let response = payload
                        .as_ref()
                        .and_then(|p| p.get("result"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();

                    let mut pending = this.pending_response.lock().await;
                    if let Some(tx) = pending.take() {
                        let _ = tx.send(response);
                    }
                }
            }
        }
    });
}
```

NOTE: The exact shape of the `Stop` hook payload must be verified by reading `hooks/installer.rs` and the Claude Code hook documentation. The `result` field path may differ. The implementer should check what data the `Stop` event actually contains and adjust the extraction logic accordingly.

- [ ] **Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/services/vigil_manager.rs
git commit -m "feat: add send_message() with hook-based response detection"
```

---

### Task 3: Implement lifecycle management (start, restart, shutdown)

**Files:**
- Modify: `apps/daemon/src/services/vigil_manager.rs`

- [ ] **Step 1: Implement start() — full startup sequence**

```rust
pub(crate) async fn start(self: &Arc<Self>) -> anyhow::Result<()> {
    self.spawn_vigil().await?;
    self.start_hook_listener();
    self.start_exit_monitor();
    // Wait for readiness (first Stop event or 30s timeout)
    self.wait_for_ready().await;
    Ok(())
}
```

- [ ] **Step 2: Implement start_exit_monitor()**

Polls `alive` flag (set by reader thread on EOF). On death:
1. Cancel any in-flight pending_response with error
2. Wait 2 seconds
3. Load last 10 chat messages from SQLite
4. Respawn Vigil PTY
5. Type context prompt into PTY
6. Persist "Vigil restarted" system message

```rust
fn start_exit_monitor(self: &Arc<Self>) {
    let this = Arc::clone(self);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            if this.pty_manager.is_alive(&this.session_id).await {
                continue;
            }

            tracing::warn!("Vigil PTY died, restarting...");

            // Cancel in-flight request
            if let Some(tx) = this.pending_response.lock().await.take() {
                let _ = tx.send("Vigil crashed, restarting...".to_string());
            }
            this.busy.store(false, Ordering::Release);

            // Wait before restart
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            // Respawn
            if let Err(e) = this.spawn_vigil().await {
                tracing::error!(error = %e, "Failed to restart Vigil");
                continue;
            }

            // Inject context from chat history
            this.inject_context().await;

            // Persist system message
            let chat_store = crate::services::vigil_chat::VigilChatStore::new(
                Arc::clone(&this.db),
            );
            let _ = chat_store.save_message("system", "Vigil restarted", None).await;
        }
    });
}
```

- [ ] **Step 3: Implement inject_context()**

Load last 10 messages, format as context prompt, type into PTY:

```rust
async fn inject_context(&self) {
    let chat_store = crate::services::vigil_chat::VigilChatStore::new(Arc::clone(&self.db));
    if let Ok(messages) = chat_store.list_messages(10, 0).await {
        if messages.is_empty() { return; }

        let mut context = String::from(
            "You are resuming after a restart. Recent conversation:\n\n"
        );
        for msg in messages.iter().rev() {
            let role = if msg.role == "user" { "User" } else { "You" };
            context.push_str(&format!("{role}: {}\n\n", msg.content));
        }

        let _ = self.pty_manager
            .write(&self.session_id, format!("{context}\n").into_bytes())
            .await;
    }
}
```

- [ ] **Step 4: Implement wait_for_ready()**

Wait for the first `Stop` event (Vigil initialization complete) or 30s timeout:

```rust
async fn wait_for_ready(&self) {
    let mut rx = self.event_bus.subscribe();
    let timeout = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        async {
            while let Ok(event) = rx.recv().await {
                if let AppEvent::HookEvent { session_id, event_type, .. } = &event {
                    if session_id == &self.session_id && event_type == "Stop" {
                        return;
                    }
                }
            }
        },
    );
    if timeout.await.is_err() {
        tracing::warn!("Vigil readiness timeout (30s) — proceeding anyway");
    }
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cd apps/daemon && cargo check`

- [ ] **Step 6: Commit**

```bash
git add apps/daemon/src/services/vigil_manager.rs
git commit -m "feat: add Vigil lifecycle management (start, restart, shutdown)"
```

---

## Chunk 2: Wire VigilManager into the Daemon

### Task 4: Add VigilManager to AppDeps and startup

**Files:**
- Modify: `apps/daemon/src/deps.rs`
- Modify: `apps/daemon/src/lib.rs`

- [ ] **Step 1: Add vigil_manager to AppDeps, remove vigil_cli_mutex**

In `deps.rs`:
- Remove `vigil_cli_mutex: Arc<Mutex<()>>` field (line 53) and its initialization (line 131)
- Add `vigil_manager: Arc<VigilManager>` field
- Initialize it in `AppDeps::new()`: `vigil_manager: Arc::new(VigilManager::new(&deps_partial))`

NOTE: `VigilManager::new()` needs deps that haven't been fully constructed yet. Use a two-phase init: create VigilManager with individual fields (pty_manager, output_manager, event_bus, config, db), not the full AppDeps. Or use `Arc::new_cyclic` or a `start()` method called after construction.

- [ ] **Step 2: Start VigilManager in lib.rs**

In `lib.rs`, after all services are initialized (around line 65), add:
```rust
let vigil_manager = Arc::clone(&deps.vigil_manager);
tokio::spawn(async move {
    if let Err(e) = vigil_manager.start().await {
        tracing::error!(error = %e, "Failed to start Vigil");
    }
});
```

- [ ] **Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/deps.rs apps/daemon/src/lib.rs
git commit -m "feat: wire VigilManager into AppDeps and daemon startup"
```

---

### Task 5: Rewrite process_chat() to use VigilManager

**Files:**
- Modify: `apps/daemon/src/api/vigil.rs`

- [ ] **Step 1: Simplify process_chat()**

Replace the current flow (history loading, MCP config writing, mutex acquisition, invoke_vigil()) with a simple call to `VigilManager::send_message()`:

```rust
pub(crate) async fn process_chat(
    deps: &AppDeps,
    message: &str,
    project_path: Option<&str>,
) -> anyhow::Result<ChatResult> {
    let vigil_chat_store = VigilChatStore::new(Arc::clone(&deps.db));

    // Persist user message
    vigil_chat_store.save_message("user", message, None).await?;

    // Activate project if specified
    if let Some(pp) = project_path {
        deps.vigil_service.ensure_vigil(pp).await;
    }

    // Send to Vigil via persistent PTY
    let response = deps.vigil_manager.send_message(message).await?;

    // Persist response
    vigil_chat_store.save_message("vigil", &response, None).await?;

    Ok(ChatResult {
        response,
        session_id: None,  // No longer tracked per-message
        hit_max_turns: false,
    })
}
```

- [ ] **Step 2: Handle 503 in the HTTP handler**

In the `chat()` handler, if `process_chat()` returns the "Vigil is processing" error, return HTTP 503:

```rust
Err(e) if e.to_string().contains("processing another message") => {
    (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": e.to_string() }))).into_response()
}
```

- [ ] **Step 3: Remove unused imports**

Remove imports for `invoke_vigil`, `write_mcp_config`, `VigilCliResult`, `Mutex`, and any history replay logic.

- [ ] **Step 4: Verify it compiles**

Run: `cd apps/daemon && cargo check`

- [ ] **Step 5: Commit**

```bash
git add apps/daemon/src/api/vigil.rs
git commit -m "feat: rewrite process_chat() to use VigilManager"
```

---

### Task 6: Delete claude_cli.rs and clean up references

**Files:**
- Delete: `apps/daemon/src/process/claude_cli.rs`
- Modify: `apps/daemon/src/process/mod.rs` (or `lib.rs` module declarations)

- [ ] **Step 1: Delete claude_cli.rs**

```bash
rm apps/daemon/src/process/claude_cli.rs
```

- [ ] **Step 2: Remove module declaration**

Find where `pub(crate) mod claude_cli;` is declared (in the process module) and remove it.

- [ ] **Step 3: Fix any remaining references**

Search for `claude_cli` across the daemon codebase. Remove any remaining imports or references. The Telegram poller calls `process_chat()` which no longer uses `claude_cli` internally, so it should be fine.

- [ ] **Step 4: Compile and fix errors**

Run: `cd apps/daemon && cargo build 2>&1`
Fix any remaining compilation errors.

- [ ] **Step 5: Run tests**

Run: `cd apps/daemon && cargo test`

- [ ] **Step 6: Run clippy**

Run: `cd apps/daemon && cargo clippy -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git add apps/daemon/
git commit -m "feat: delete claude_cli.rs, remove vigil_cli_mutex"
```

---

## Chunk 3: Strategy Prompt and Frontend

### Task 7: Update strategy prompt for persistent mode

**Files:**
- Modify: `apps/daemon/prompts/vigil-strategy.md`

- [ ] **Step 1: Read current strategy prompt**

Read `apps/daemon/prompts/vigil-strategy.md` to find stateless framing language.

- [ ] **Step 2: Update the prompt**

- Remove any "You have no knowledge" / "You are stateless" / "You cannot remember" language
- Add "You maintain conversation context across messages. You can reference earlier parts of the conversation."
- Keep ALL delegation rules (always spawn_worker, never answer directly)
- Keep ALL MCP tool descriptions and decision logic
- Keep ALL examples

- [ ] **Step 3: Commit**

```bash
git add apps/daemon/prompts/vigil-strategy.md
git commit -m "feat: update Vigil strategy prompt for persistent session mode"
```

---

### Task 8: Frontend — handle 503 busy and Vigil session label

**Files:**
- Modify: `apps/web/src/components/vigil/vigil-chat.tsx`
- Modify: `apps/web/src/components/vigil/session-monitor.tsx` (or `session-tree.tsx`)

- [ ] **Step 1: Handle 503 in vigil-chat.tsx**

In the `handleSend()` function, catch 503 errors and show a "Vigil is busy" message instead of a generic error:

```typescript
onError: (error: any) => {
  const isBusy = error?.response?.status === 503;
  addMessage({
    id: Date.now(),
    role: 'vigil',
    content: isBusy
      ? 'I\'m currently processing another request. Please wait a moment.'
      : 'Something went wrong. Please try again.',
    embeddedCards: null,
    createdAt: Date.now(),
  });
}
```

- [ ] **Step 2: Show "Vigil" label for Vigil session in SessionMonitor**

In `session-tree.tsx` or `session-monitor.tsx`, check if a session's ID is `"vigil"` (or has a special type) and show "Vigil Orchestrator" instead of the truncated prompt.

- [ ] **Step 3: Run biome**

Run: `npx biome check --write .`

- [ ] **Step 4: Commit**

```bash
git add apps/web/src/components/vigil/
git commit -m "feat: handle Vigil busy state and show Vigil session label"
```

---

### Task 9: Full build, test, and cleanup

**Files:** All

- [ ] **Step 1: Build Rust daemon**

Run: `cd apps/daemon && cargo build`

- [ ] **Step 2: Run Rust tests**

Run: `cd apps/daemon && cargo test`

- [ ] **Step 3: Run clippy**

Run: `cd apps/daemon && cargo clippy -- -D warnings`

- [ ] **Step 4: Build frontend**

Run: `npm run build`

- [ ] **Step 5: Run biome**

Run: `npx biome check --write .`

- [ ] **Step 6: Verify no dead code**

Search for any remaining references to:
- `invoke_vigil`
- `VigilCliResult`
- `vigil_cli_mutex`
- `claude_cli`
- `parse_json_output`

All should return zero results.

- [ ] **Step 7: Commit any final fixes**

```bash
git add -A
git commit -m "chore: final cleanup for Vigil persistent PTY"
```
