# Real PTY Terminal Access — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace pipe-based Claude Code spawning with real OS PTY allocation so every session feels like running `claude` locally, with interchangeable terminal and Vigil input.

**Architecture:** Real PTY via `portable-pty` crate. Single mpsc channel serializes writes from both WebSocket and Vigil. `spawn_blocking` bridges sync PTY I/O to tokio async. Frontend gets flexible split/fullscreen terminal panel on the dashboard page.

**Tech Stack:** portable-pty (Rust), xterm.js (existing), tokio spawn_blocking, axum WebSocket (existing)

**Spec:** `docs/superpowers/specs/2026-03-15-real-pty-terminal-design.md`

---

## File Structure

### Rust Daemon (apps/daemon/)

| File | Action | Responsibility |
|------|--------|---------------|
| `Cargo.toml` | Modify | Add `portable-pty` dependency |
| `src/process/pty_manager.rs` | Rewrite | New `PtyHandle` with `MasterPty`, `stdin_tx`, resize, graceful kill |
| `src/process/agent_spawner.rs` | Rewrite | PTY-based spawning, single output reader, `spawn_blocking` bridges |
| `src/process/output_manager.rs` | No changes | Confirmed: `append()` takes raw bytes, broadcasts, writes disk log — works unchanged with PTY output |
| `src/api/ws_terminal.rs` | Modify | Wire resize to `pty_manager.resize()` |

### Frontend (apps/web/)

| File | Action | Responsibility |
|------|--------|---------------|
| `src/app/dashboard/page.tsx` | Rewrite | Three layout states: vigil-only, split panel, fullscreen terminal |
| `src/lib/hooks/use-terminal.ts` | Modify | Remove read-only assumptions, resize on layout transitions |
| `src/components/dashboard/terminal-panel.tsx` | Modify | Add minimize/maximize/close buttons, session label |
| `src/components/vigil/session-monitor.tsx` | Modify | Click opens inline panel instead of navigating to detail page |
| `src/lib/stores/terminal-store.ts` | Create | Track active session, panel mode (closed/panel/fullscreen) |

---

## Chunk 1: PTY Manager Rewrite

### Task 1: Add portable-pty dependency

**Files:**
- Modify: `apps/daemon/Cargo.toml`

- [ ] **Step 1: Add portable-pty to Cargo.toml**

Add to `[dependencies]`:
```toml
portable-pty = "0.8"
```

- [ ] **Step 2: Verify it compiles**

Run: `cd apps/daemon && cargo check`
Expected: Compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add apps/daemon/Cargo.toml apps/daemon/Cargo.lock
git commit -m "chore: add portable-pty dependency"
```

---

### Task 2: Rewrite PtyHandle and PtyManager

**Files:**
- Modify: `apps/daemon/src/process/pty_manager.rs`

The current `PtyHandle` has `stdin_tx: mpsc::Sender<Vec<u8>>` and `alive: Arc<AtomicBool>`. The new version adds `master: Box<dyn MasterPty + Send>` for resize and a graceful kill sequence.

- [ ] **Step 1: Write failing tests for new PtyManager behavior**

Add tests at the bottom of `pty_manager.rs`. These test the new resize and kill behaviors. Use `/bin/cat` as a simple PTY child.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use portable_pty::{native_pty_system, PtySize, CommandBuilder};
    use std::io::Read;
    use tokio::time::{sleep, Duration};

    /// Helper: spawn /bin/cat inside a real PTY and register it.
    /// Returns (manager, session_id, reader) where reader is the master read handle.
    async fn spawn_cat_pty() -> (PtyManager, String, Box<dyn Read + Send>) {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .expect("open pty");

        let cmd = CommandBuilder::new("/bin/cat");
        let child = pair.slave.spawn_command(cmd).expect("spawn cat");
        drop(pair.slave); // Drop slave so EOF works

        let reader = pair.master.try_clone_reader().expect("clone reader");
        let writer = pair.master.try_clone_writer().expect("clone writer");

        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        let alive = Arc::new(AtomicBool::new(true));

        // Spawn writer drain thread (must use spawn_blocking for tokio context)
        tokio::task::spawn_blocking(move || {
            let mut writer = writer;
            while let Some(data) = stdin_rx.blocking_recv() {
                use std::io::Write;
                if writer.write_all(&data).is_err() {
                    break;
                }
            }
        });

        let manager = PtyManager::new();
        let session_id = "test-pty-session".to_string();

        manager.register(&session_id, PtyHandle {
            stdin_tx,
            master: pair.master,
            child: Box::new(child),
            alive: Arc::clone(&alive),
        }).await;

        (manager, session_id, reader)
    }

    #[tokio::test]
    async fn test_write_and_read_through_pty() {
        let (manager, sid, mut reader) = spawn_cat_pty().await;

        manager.write(&sid, b"hello\n".to_vec()).await.unwrap();

        // Give cat time to echo
        sleep(Duration::from_millis(100)).await;

        let mut buf = [0u8; 256];
        // Set non-blocking or use a timeout; for test simplicity, read what's available
        // cat echoes input back through the PTY
        let n = reader.read(&mut buf).unwrap();
        let output = String::from_utf8_lossy(&buf[..n]);
        assert!(output.contains("hello"), "Expected echoed input, got: {output}");

        manager.kill(&sid).await;
    }

    #[tokio::test]
    async fn test_resize() {
        let (manager, sid, _reader) = spawn_cat_pty().await;

        // Should not panic or error
        let result = manager.resize(&sid, 120, 40).await;
        assert!(result.is_ok(), "Resize should succeed");

        manager.kill(&sid).await;
    }

    #[tokio::test]
    async fn test_kill_marks_dead() {
        let (manager, sid, _reader) = spawn_cat_pty().await;

        assert!(manager.is_alive(&sid).await);
        manager.kill(&sid).await;
        // After kill, handle is removed
        assert!(!manager.is_alive(&sid).await);
    }

    #[tokio::test]
    async fn test_concurrent_writes() {
        let (manager, sid, mut reader) = spawn_cat_pty().await;
        let manager = Arc::new(manager);

        let m1 = Arc::clone(&manager);
        let s1 = sid.clone();
        let t1 = tokio::spawn(async move {
            for i in 0..10 {
                m1.write(&s1, format!("a{i}\n").into_bytes()).await.unwrap();
            }
        });

        let m2 = Arc::clone(&manager);
        let s2 = sid.clone();
        let t2 = tokio::spawn(async move {
            for i in 0..10 {
                m2.write(&s2, format!("b{i}\n").into_bytes()).await.unwrap();
            }
        });

        t1.await.unwrap();
        t2.await.unwrap();

        sleep(Duration::from_millis(200)).await;

        let mut buf = vec![0u8; 4096];
        let n = reader.read(&mut buf).unwrap();
        let output = String::from_utf8_lossy(&buf[..n]);

        // All 20 lines should appear (order may vary)
        for i in 0..10 {
            assert!(output.contains(&format!("a{i}")), "Missing a{i} in output");
            assert!(output.contains(&format!("b{i}")), "Missing b{i} in output");
        }

        manager.kill(&sid).await;
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd apps/daemon && cargo test pty_manager --lib -- --nocapture 2>&1 | head -50`
Expected: Compilation errors — `PtyHandle` doesn't have `master` or `child` fields yet.

- [ ] **Step 3: Rewrite PtyHandle struct and PtyManager methods**

Replace the `PtyHandle` struct and all `PtyManager` methods:

```rust
use portable_pty::MasterPty;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use std::collections::HashMap;

/// Handle to a running PTY session.
pub(crate) struct PtyHandle {
    /// Channel to serialize writes to the PTY master.
    pub stdin_tx: mpsc::Sender<Vec<u8>>,
    /// Master PTY handle — used for resize operations.
    pub master: Box<dyn MasterPty + Send>,
    /// Child process handle — used for forceful kill.
    pub child: Box<dyn portable_pty::Child + Send>,
    /// Whether the PTY process is still alive.
    pub alive: Arc<AtomicBool>,
}

pub(crate) struct PtyManager {
    ptys: RwLock<HashMap<String, PtyHandle>>,
}

impl PtyManager {
    pub(crate) fn new() -> Self {
        Self {
            ptys: RwLock::new(HashMap::new()),
        }
    }

    pub(crate) async fn register(&self, session_id: &str, handle: PtyHandle) {
        self.ptys.write().await.insert(session_id.to_string(), handle);
    }

    pub(crate) async fn is_alive(&self, session_id: &str) -> bool {
        self.ptys
            .read()
            .await
            .get(session_id)
            .map(|h| h.alive.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Write bytes to the PTY via the serialized channel.
    pub(crate) async fn write(&self, session_id: &str, data: Vec<u8>) -> anyhow::Result<()> {
        let ptys = self.ptys.read().await;
        let handle = ptys.get(session_id).ok_or_else(|| anyhow::anyhow!("no pty for session"))?;
        handle.stdin_tx.send(data).await.map_err(|_| anyhow::anyhow!("stdin channel closed"))?;
        Ok(())
    }

    /// Resize the PTY. Delivers SIGWINCH to the child process.
    pub(crate) async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        let mut ptys = self.ptys.write().await;
        let handle = ptys.get_mut(session_id).ok_or_else(|| anyhow::anyhow!("no pty for session"))?;
        handle.master.resize(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }).map_err(|e| anyhow::anyhow!("resize failed: {e}"))?;
        Ok(())
    }

    /// Graceful kill: drop master (SIGHUP), then SIGKILL fallback.
    pub(crate) async fn kill(&self, session_id: &str) {
        let handle = self.ptys.write().await.remove(session_id);
        if let Some(mut handle) = handle {
            handle.alive.store(false, Ordering::Relaxed);
            // Drop master — sends SIGHUP to child process group
            drop(handle.master);
            // Grace period then force kill
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let _ = handle.child.kill();
        }
    }

    pub(crate) async fn remove(&self, session_id: &str) {
        self.ptys.write().await.remove(session_id);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd apps/daemon && cargo test pty_manager --lib -- --nocapture`
Expected: All 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add apps/daemon/src/process/pty_manager.rs
git commit -m "feat: rewrite PtyManager with real PTY support via portable-pty"
```

---

### Task 3: Add spawn_claude_pty() and wire_pty_io() functions

**Files:**
- Modify: `apps/daemon/src/process/agent_spawner.rs`

Add the new PTY-based spawn and I/O wiring functions alongside the existing ones (don't delete old code yet).

- [ ] **Step 1: Write failing test for PTY-based spawning**

Add a test that spawns `/bin/sh` via the new PTY flow, writes a command, and verifies output arrives in the output manager. Use `tokio::task::spawn_blocking` (not `std::thread::spawn`) for the reader thread to ensure the tokio runtime handle is available.

```rust
#[cfg(test)]
mod pty_spawn_tests {
    use super::*;
    use crate::process::output_manager::OutputManager;
    use crate::process::pty_manager::{PtyManager, PtyHandle};
    use portable_pty::{native_pty_system, PtySize, CommandBuilder};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use tokio::time::{sleep, Duration};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_pty_spawn_and_output_capture() {
        let tmp = TempDir::new().unwrap();
        let output_manager = Arc::new(OutputManager::new(tmp.path().to_path_buf()));
        let pty_manager = Arc::new(PtyManager::new());

        let session_id = "test-pty-spawn";
        let alive = Arc::new(AtomicBool::new(true));

        // Allocate PTY
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .expect("open pty");

        // Spawn /bin/sh
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd).expect("spawn sh");
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().expect("clone reader");
        let writer = pair.master.try_clone_writer().expect("clone writer");

        // Use wire_pty_io to set up I/O
        let (stdin_tx, _reader_handle) = wire_pty_io(
            session_id, reader, writer,
            Arc::clone(&output_manager),
            Arc::clone(&alive),
        );

        // Register PTY
        pty_manager.register(session_id, PtyHandle {
            stdin_tx: stdin_tx.clone(),
            master: pair.master,
            child: Box::new(child),
            alive: Arc::clone(&alive),
        }).await;

        // Send a command
        pty_manager.write(session_id, b"echo HELLO_PTY\n".to_vec()).await.unwrap();
        sleep(Duration::from_millis(300)).await;

        // Check output manager captured it
        let buf = output_manager.get_buffer(session_id).await;
        let output = String::from_utf8_lossy(&buf.unwrap_or_default());
        assert!(output.contains("HELLO_PTY"), "Expected output in buffer, got: {output}");

        // Cleanup — send exit, verify alive flag gets set to false
        pty_manager.write(session_id, b"exit\n".to_vec()).await.unwrap();
        sleep(Duration::from_millis(500)).await;
        assert!(!alive.load(std::sync::atomic::Ordering::Relaxed), "alive should be false after exit");

        pty_manager.kill(session_id).await;
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd apps/daemon && cargo test pty_spawn_tests --lib -- --nocapture 2>&1 | head -30`
Expected: Fails — `wire_pty_io` function doesn't exist yet.

- [ ] **Step 3: Add spawn_claude_pty() function**

Add (don't replace yet) a new function in `agent_spawner.rs`:

```rust
use portable_pty::{native_pty_system, PtySize, CommandBuilder, MasterPty, Child as PtyChild};

/// Spawn claude inside a real PTY. Returns (master, child, reader, writer).
fn spawn_claude_pty(
    work_dir: &str,
    continue_session: bool,
) -> anyhow::Result<(Box<dyn MasterPty + Send>, Box<dyn PtyChild + Send>, Box<dyn std::io::Read + Send>, Box<dyn std::io::Write + Send>)> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| anyhow::anyhow!("PTY allocation failed: {e}"))?;

    let mut cmd = CommandBuilder::new("claude");
    cmd.cwd(work_dir);
    cmd.env("TERM", "xterm-256color");
    // Prevent nested Claude Code detection
    cmd.env_remove("CLAUDE_CODE");
    cmd.env_remove("CLAUDE_CODE_ENTRYPOINT");

    if continue_session {
        cmd.arg("--continue");
    }
    // --verbose enables hook events that Vigil relies on for structured state tracking
    cmd.arg("--verbose");
    cmd.arg("--dangerously-skip-permissions");

    let child = pair.slave.spawn_command(cmd)
        .map_err(|e| anyhow::anyhow!("Failed to spawn claude in PTY: {e}"))?;

    // Drop slave immediately — prevents EOF issues when child exits
    drop(pair.slave);

    let reader = pair.master.try_clone_reader()
        .map_err(|e| anyhow::anyhow!("Failed to clone PTY reader: {e}"))?;
    let writer = pair.master.try_clone_writer()
        .map_err(|e| anyhow::anyhow!("Failed to clone PTY writer: {e}"))?;

    Ok((pair.master, Box::new(child), reader, writer))
}
```

- [ ] **Step 4: Add wire_pty_io() function**

Add the I/O wiring function. The `alive` flag is passed in and set to `false` when the reader hits EOF (child exit). This is how the exit monitor detects natural process termination.

```rust
/// Wire PTY I/O: reader → output_manager, writer ← stdin_tx channel.
/// Sets `alive` to false when reader detects EOF (child exited).
fn wire_pty_io(
    session_id: &str,
    reader: Box<dyn std::io::Read + Send>,
    writer: Box<dyn std::io::Write + Send>,
    output_manager: Arc<OutputManager>,
    alive: Arc<AtomicBool>,
) -> (mpsc::Sender<Vec<u8>>, tokio::task::JoinHandle<()>) {
    let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

    // Writer drain thread (blocking)
    tokio::task::spawn_blocking(move || {
        let mut writer = writer;
        while let Some(data) = stdin_rx.blocking_recv() {
            use std::io::Write;
            if writer.write_all(&data).is_err() { break; }
        }
    });

    // Reader thread (blocking) → output manager
    // Sets alive=false on EOF so the exit monitor can detect child death.
    let sid = session_id.to_string();
    let reader_handle = tokio::task::spawn_blocking(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            use std::io::Read;
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF — child exited
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    // IMPORTANT: Cannot use handle.block_on() inside spawn_blocking
                    // (panics with "Cannot block_on from within a tokio runtime").
                    // Use handle.spawn() to dispatch async work non-blocking.
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        let om = Arc::clone(&output_manager);
                        let s = sid.clone();
                        let _ = handle.spawn(async move {
                            om.append(&s, &chunk).await;
                        });
                    }
                }
                Err(_) => break,
            }
        }
        // Signal that the child process has exited
        alive.store(false, std::sync::atomic::Ordering::Relaxed);
    });

    (stdin_tx, reader_handle)
}
```

- [ ] **Step 5: Run tests**

Run: `cd apps/daemon && cargo test pty_spawn_tests --lib -- --nocapture`
Expected: Pass — the test allocates a PTY, writes through it, and verifies output + alive flag.

- [ ] **Step 6: Commit**

```bash
git add apps/daemon/src/process/agent_spawner.rs
git commit -m "feat: add spawn_claude_pty() and wire_pty_io() for real PTY spawning"
```

---

### Task 4: Rewrite spawn_interactive() and exit monitor

**Files:**
- Modify: `apps/daemon/src/process/agent_spawner.rs`

Replace the existing `spawn_interactive()` to use the new PTY functions, and update the exit monitor to poll the `alive` flag (set by the reader thread on EOF).

- [ ] **Step 1: Rewrite spawn_interactive()**

Replace the existing method. Key changes: calls `spawn_claude_pty` (not `spawn_claude_process`), calls `wire_pty_io` (not `wire_process`), sends initial prompt as PTY input.

**IMPORTANT:** Match the existing `AppEvent::SessionSpawned` variant exactly — check `events.rs` for the field name. The current codebase uses `session: Session` (the full object), not `session_id: String`. Use whichever the codebase actually has:

```rust
pub(crate) async fn spawn_interactive(
    &self,
    session: &Session,
    continue_session: bool,
) -> anyhow::Result<()> {
    let work_dir = self.prepare_session(session).await?;

    let (master, child, reader, writer) = match spawn_claude_pty(&work_dir, continue_session) {
        Ok(result) => result,
        Err(e) => {
            // PTY allocation failed — mark session as Failed
            self.db.update_session_status(&session.id, "failed").await?;
            if let Ok(Some(updated)) = self.db.get_session(&session.id).await {
                self.event_bus.emit(AppEvent::SessionUpdate { session: updated });
            }
            return Err(e);
        }
    };

    let alive = Arc::new(AtomicBool::new(true));

    let (stdin_tx, _reader_handle) = wire_pty_io(
        &session.id,
        reader,
        writer,
        Arc::clone(&self.output_manager),
        Arc::clone(&alive),
    );

    // Register PTY handle
    self.pty_manager.register(&session.id, PtyHandle {
        stdin_tx: stdin_tx.clone(),
        master,
        child,
        alive: Arc::clone(&alive),
    }).await;

    // Send initial prompt as input (like a human typing it)
    let prompt = format!("{}\n", session.prompt);
    stdin_tx.send(prompt.into_bytes()).await
        .map_err(|_| anyhow::anyhow!("Failed to send initial prompt"))?;

    // Spawn exit monitor — polls alive flag set by reader thread
    Self::spawn_exit_monitor(
        &session.id,
        alive,
        Arc::clone(&self.db),
        Arc::clone(&self.event_bus),
        Arc::clone(&self.pty_manager),
    );

    // Emit event — match the existing AppEvent::SessionSpawned variant exactly
    // Check events.rs for the actual field names
    self.event_bus.emit(AppEvent::SessionSpawned {
        session_id: session.id.clone(),
    });

    Ok(())
}
```

- [ ] **Step 2: Rewrite spawn_exit_monitor()**

The exit monitor polls the `alive` flag which is set to `false` by the reader thread in `wire_pty_io` when it detects EOF:

```rust
fn spawn_exit_monitor(
    session_id: &str,
    alive: Arc<AtomicBool>,
    db: Arc<SqliteDb>,
    event_bus: Arc<EventBus>,
    pty_manager: Arc<PtyManager>,
) {
    let sid = session_id.to_string();
    tokio::spawn(async move {
        // Wait for alive=false (set by reader thread on EOF, or by kill())
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
            if !alive.load(Ordering::Relaxed) {
                break;
            }
        }

        if let Ok(Some(session)) = db.get_session(&sid).await {
            if session.status == "running" || session.status == "needs_input" {
                let _ = db.update_session_status(&sid, "completed").await;
                if let Ok(Some(updated)) = db.get_session(&sid).await {
                    event_bus.emit(AppEvent::SessionUpdate { session: updated });
                }
            }
        }

        pty_manager.remove(&sid).await;
    });
}
```

- [ ] **Step 3: Run tests**

Run: `cd apps/daemon && cargo test -- --nocapture 2>&1 | tail -30`
Expected: Pass.

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/process/agent_spawner.rs
git commit -m "feat: rewrite spawn_interactive() and exit monitor for PTY"
```

---

### Task 5: Remove old pipe-based code and unify pipeline steps

**Files:**
- Modify: `apps/daemon/src/process/agent_spawner.rs`

- [ ] **Step 1: Delete spawn_claude_process() (old pipe-based function)**

Remove the function that used `tokio::process::Command` with `-p` flag and `Stdio::piped()`.

- [ ] **Step 2: Delete wire_process() and spawn_output_reader()**

Remove the old functions that created separate stdout/stderr reader tasks.

- [ ] **Step 3: Unify spawn_interactive_pipeline_step()**

Replace `spawn_interactive_pipeline_step()` with a thin wrapper around `spawn_interactive()`. Pipeline steps skip worktree creation and use a different prompt format. Read the current callers (search for `spawn_interactive_pipeline_step` in the codebase) to understand how it's invoked and ensure the new flow handles the same inputs.

Keep a method like:
```rust
pub(crate) async fn spawn_pipeline_step(
    &self,
    session: &Session,
    prompt: &str,
) -> anyhow::Result<()> {
    // Same as spawn_interactive but:
    // - No worktree creation (runs in project dir)
    // - Custom prompt passed directly
    // Delegates to the same spawn_claude_pty + wire_pty_io
}
```

- [ ] **Step 4: Update existing tests**

Update tests in `agent_spawner.rs` (around lines 472-650) to match the new PTY-based flow. Remove tests that reference deleted functions.

- [ ] **Step 5: Run all tests**

Run: `cd apps/daemon && cargo test -- --nocapture 2>&1 | tail -30`
Expected: All pass.

- [ ] **Step 6: Run clippy**

Run: `cd apps/daemon && cargo clippy -- -D warnings`
Expected: Clean.

- [ ] **Step 7: Commit**

```bash
git add apps/daemon/src/process/agent_spawner.rs
git commit -m "feat: remove old pipe-based spawning, unify pipeline steps"
```

---

### Task 6: Wire resize in WebSocket terminal handler

**Files:**
- Modify: `apps/daemon/src/api/ws_terminal.rs`

- [ ] **Step 1: Update handle_terminal() Resize handler**

The current `Resize` handler calls `pty_manager.resize()` which is a no-op. After Task 2, `resize()` is async and returns `Result`. Update the call:

```rust
ClientMessage::Resize { cols, rows } => {
    let _ = deps.pty_manager.resize(&session_id, cols, rows).await;
}
```

Add `.await` if missing, and handle (ignore) the Result.

- [ ] **Step 2: Update Input handler for new write() signature**

The current `Input` handler calls `pty_manager.write()`. The new signature takes `Vec<u8>` and returns `Result`. Update:

```rust
ClientMessage::Input { data } => {
    let _ = deps.pty_manager.write(&session_id, data.into_bytes()).await;
}
```

Ensure the Result is handled (logged or ignored).

- [ ] **Step 3: Run tests**

Run: `cd apps/daemon && cargo test ws_terminal -- --nocapture`
Expected: Pass.

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/api/ws_terminal.rs
git commit -m "feat: wire real PTY resize and update write calls in WS handler"
```

---

### Task 7: Fix all remaining compile-breaking references

**Files:**
- Modify: any files that reference old `PtyHandle` struct fields or old spawn functions

- [ ] **Step 1: Compile the full project**

Run: `cd apps/daemon && cargo build 2>&1`
Fix any compilation errors propagating to:
- `deps.rs` (if it constructs PtyHandle)
- `sessions.rs` (cancel_session calls pty_manager.kill())
- `mcp.rs` (if it spawns sessions)
- `services/` (Vigil service, session manager)

The `PtyManager` public API (`write`, `kill`, `is_alive`, `resize`) keeps the same method names. The `PtyHandle` struct is only constructed in `agent_spawner.rs`. Most call sites should need minimal changes (adding `.await` on resize, handling `Result` on write).

- [ ] **Step 2: Fix each compilation error**

Iterate until `cargo build` succeeds.

- [ ] **Step 3: Verify output_manager.rs needs no changes**

The `OutputManager::append()` method takes raw bytes and broadcasts them — unchanged. Confirm it compiles and works with the new single-stream PTY output (no structural changes needed, as stated in the spec).

- [ ] **Step 4: Run full test suite**

Run: `cd apps/daemon && cargo test`
Expected: All tests pass.

- [ ] **Step 5: Run clippy**

Run: `cd apps/daemon && cargo clippy -- -D warnings`
Expected: Clean.

- [ ] **Step 6: Commit**

```bash
git add apps/daemon/
git commit -m "fix: update all references for new PTY-based PtyHandle"
```

---

## Chunk 2: Frontend Terminal Panel

### Task 8: Create terminal store

**Files:**
- Create: `apps/web/src/lib/stores/terminal-store.ts`

- [ ] **Step 1: Create the store**

```typescript
import { create } from "zustand";

type PanelMode = "closed" | "panel" | "fullscreen";

interface TerminalState {
  activeSessionId: string | null;
  panelMode: PanelMode;
  openSession: (sessionId: string) => void;
  closePanel: () => void;
  setPanelMode: (mode: PanelMode) => void;
  toggleFullscreen: () => void;
}

export const useTerminalStore = create<TerminalState>((set) => ({
  activeSessionId: null,
  panelMode: "closed",
  openSession: (sessionId) =>
    set({ activeSessionId: sessionId, panelMode: "panel" }),
  closePanel: () => set({ activeSessionId: null, panelMode: "closed" }),
  setPanelMode: (mode) => set({ panelMode: mode }),
  toggleFullscreen: () =>
    set((s) => ({
      panelMode: s.panelMode === "fullscreen" ? "panel" : "fullscreen",
    })),
}));
```

- [ ] **Step 2: Commit**

```bash
git add apps/web/src/lib/stores/terminal-store.ts
git commit -m "feat: add terminal store for panel mode management"
```

---

### Task 9: Update terminal-panel with minimize/maximize/close

**Files:**
- Modify: `apps/web/src/components/dashboard/terminal-panel.tsx`
- Modify: `apps/web/src/lib/hooks/use-terminal.ts`

- [ ] **Step 1: Read current terminal-panel.tsx and use-terminal.ts**

Read both files to understand the exact current structure before modifying.

- [ ] **Step 2: Add control buttons to terminal-panel header**

Import the terminal store and lucide icons. Add three buttons to the header (right side, next to the existing connection status indicator):

```typescript
import { useTerminalStore } from "@/lib/stores/terminal-store";
import { Minimize2, Maximize2, X } from "lucide-react";

// Inside the component:
const { panelMode, toggleFullscreen, closePanel, setPanelMode } = useTerminalStore();

// In the header JSX, after the connection status indicator:
<div className="flex items-center gap-1">
  {panelMode === "fullscreen" ? (
    <button onClick={() => setPanelMode("panel")} className="p-1 rounded hover:bg-surface-alt" title="Minimize">
      <Minimize2 className="w-4 h-4 text-text-muted" />
    </button>
  ) : (
    <button onClick={toggleFullscreen} className="p-1 rounded hover:bg-surface-alt" title="Maximize">
      <Maximize2 className="w-4 h-4 text-text-muted" />
    </button>
  )}
  <button onClick={closePanel} className="p-1 rounded hover:bg-surface-alt" title="Close">
    <X className="w-4 h-4 text-text-muted" />
  </button>
</div>
```

Also add a session label to the left side of the header (fetch from session store by sessionId prop).

- [ ] **Step 3: Remove read-only state for running sessions**

In `use-terminal.ts`, find the logic that writes "[Session ended — terminal is read-only]" (around the `pty_status` message handler, lines ~116-130). Keep this message ONLY when `ptyAlive` transitions to `false`. Remove any logic that blocks `onData` input based on session status — input should always be forwarded while the WebSocket is connected, regardless of status. The PTY itself enforces what's accepted.

- [ ] **Step 4: Verify resize fires on layout transitions**

The existing `ResizeObserver` in `use-terminal.ts` (lines ~172-181) watches the container div. When the panel switches between panel/fullscreen mode, the container resizes, triggering the observer → `fitAddon.fit()` → sends `{ type: 'resize', cols, rows }`. No additional code needed, but verify by reading the ResizeObserver setup.

- [ ] **Step 5: Commit**

```bash
git add apps/web/src/components/dashboard/terminal-panel.tsx apps/web/src/lib/hooks/use-terminal.ts
git commit -m "feat: add terminal panel controls and remove read-only for live sessions"
```

---

### Task 10: Update dashboard layout with three panel states

**Files:**
- Modify: `apps/web/src/app/dashboard/page.tsx`

- [ ] **Step 1: Read current dashboard page**

Read `apps/web/src/app/dashboard/page.tsx` (64 lines) to understand the current layout. Currently: VigilChat (left, flex-1) + SessionMonitor (right, 45% via framer-motion AnimatePresence). The `panelOpen` state controls SessionMonitor visibility.

- [ ] **Step 2: Add terminal store and TerminalPanel import**

```typescript
import { useTerminalStore } from "@/lib/stores/terminal-store";
import { TerminalPanel } from "@/components/dashboard/terminal-panel";

const { activeSessionId, panelMode, closePanel } = useTerminalStore();
```

- [ ] **Step 3: Implement fullscreen mode**

Wrap the entire return in a conditional. When `panelMode === "fullscreen"`, render only the terminal with a floating back button:

```tsx
if (panelMode === "fullscreen" && activeSessionId) {
  return (
    <div className="h-full w-full relative bg-background">
      <TerminalPanel sessionId={activeSessionId} />
      <button
        onClick={closePanel}
        className="absolute top-3 left-3 z-10 flex items-center gap-2 px-3 py-1.5 rounded-lg bg-surface/80 backdrop-blur border border-border text-sm text-text-muted hover:text-text transition-colors"
      >
        <ArrowLeft className="w-4 h-4" />
        Back to Vigil
      </button>
    </div>
  );
}
```

- [ ] **Step 4: Implement panel mode (split view)**

In the normal layout, when `panelMode === "panel"`, split the space between VigilChat and TerminalPanel. SessionMonitor collapses or overlays. Use the existing framer-motion spring animation pattern for the terminal panel entrance:

```tsx
<div className="flex h-full">
  {/* Vigil chat — shrinks when terminal panel is open */}
  <div className={panelMode === "panel" ? "w-1/2" : "flex-1"}>
    <VigilChat onSessionClick={() => setPanelOpen(true)} />
  </div>

  {/* Terminal panel — slides in from right */}
  <AnimatePresence>
    {panelMode === "panel" && activeSessionId && (
      <motion.div
        initial={{ width: 0, opacity: 0 }}
        animate={{ width: "50%", opacity: 1 }}
        exit={{ width: 0, opacity: 0 }}
        transition={{ type: "spring", stiffness: 300, damping: 30 }}
        className="border-l border-border overflow-hidden"
      >
        <TerminalPanel sessionId={activeSessionId} />
      </motion.div>
    )}
  </AnimatePresence>

  {/* SessionMonitor toggle — keep existing behavior but only when terminal panel is closed */}
  {panelMode === "closed" && (
    // ... existing SessionMonitor toggle + panel code
  )}
</div>
```

Preserve the existing SessionMonitor toggle button and AnimatePresence animation when no terminal is open.

- [ ] **Step 5: Commit**

```bash
git add apps/web/src/app/dashboard/page.tsx
git commit -m "feat: dashboard layout with three terminal panel states"
```

---

### Task 11: Update SessionMonitor to open inline panel

**Files:**
- Modify: `apps/web/src/components/vigil/session-monitor.tsx`
- Modify: `apps/web/src/components/vigil/session-tree.tsx`

- [ ] **Step 1: Read current session-tree.tsx click handler**

The current `SessionRow` navigates to `/dashboard/sessions/${session.id}` on click (line ~91 in session-tree.tsx).

- [ ] **Step 2: Replace navigation with terminal store action**

Instead of `router.push(...)`, call `useTerminalStore().openSession(session.id)`:

```typescript
import { useTerminalStore } from "@/lib/stores/terminal-store";

// In SessionRow:
const openSession = useTerminalStore((s) => s.openSession);

// Replace onClick:
onClick={() => openSession(session.id)}
```

This opens the terminal panel inline on the dashboard instead of navigating away.

- [ ] **Step 3: Highlight active session in the list**

Add visual indicator for which session is currently open in the terminal:

```typescript
const activeSessionId = useTerminalStore((s) => s.activeSessionId);
// Add highlight class when session.id === activeSessionId
```

- [ ] **Step 4: Commit**

```bash
git add apps/web/src/components/vigil/session-monitor.tsx apps/web/src/components/vigil/session-tree.tsx
git commit -m "feat: session clicks open inline terminal panel instead of navigating"
```

---

### Task 12: Update blocker-card terminal button

**Files:**
- Modify: `apps/web/src/components/vigil/cards/blocker-card.tsx`

- [ ] **Step 1: Update "Open terminal" button**

Replace the navigation link to session detail with `openSession()`:

```typescript
import { useTerminalStore } from "@/lib/stores/terminal-store";

const openSession = useTerminalStore((s) => s.openSession);

// Replace the "Open terminal" button onClick:
onClick={() => card.sessionId && openSession(card.sessionId)}
```

- [ ] **Step 2: Commit**

```bash
git add apps/web/src/components/vigil/cards/blocker-card.tsx
git commit -m "feat: blocker card opens inline terminal panel"
```

---

## Chunk 3: Integration & Testing

### Task 13: Verify Vigil can write to PTY

**Files:**
- Modify: `apps/daemon/src/services/` (whichever service handles Vigil → session interaction)

- [ ] **Step 1: Identify where Vigil sends input to sessions**

Search for `pty_manager.write` across the daemon codebase to find all callers. Vigil likely writes to sessions via `pty_manager.write()` for pipeline steps and blocker replies. The `PtyManager::write()` signature changed from `(&str, &[u8]) -> bool` to `(&str, Vec<u8>) -> Result<()>`. Update all callers to:
- Pass `Vec<u8>` instead of `&[u8]` (use `.to_vec()` or `.into_bytes()`)
- Handle the `Result` (log errors or use `let _ =`)

- [ ] **Step 2: Write integration test for Vigil → PTY write**

```rust
#[tokio::test]
async fn test_vigil_writes_to_pty_session() {
    // Set up PTY with /bin/cat
    // Simulate Vigil writing a reply
    // Verify the reply appears in the output manager's buffer
    // This proves the same write path works for both WS and Vigil
    let tmp = TempDir::new().unwrap();
    let output_manager = Arc::new(OutputManager::new(tmp.path().to_path_buf()));
    let pty_manager = Arc::new(PtyManager::new());
    let alive = Arc::new(AtomicBool::new(true));

    // ... (same PTY setup as Task 3 test)
    // Write from "Vigil" (just pty_manager.write directly)
    pty_manager.write("session", b"vigil reply\n".to_vec()).await.unwrap();
    // Verify it echoes back through the output manager
}
```

- [ ] **Step 3: Commit**

```bash
git add apps/daemon/src/
git commit -m "feat: verify and update Vigil PTY write integration"
```

---

### Task 14: Add WebSocket round-trip integration test

**Files:**
- Modify: `apps/daemon/src/api/ws_terminal.rs` (add test) or new test file

- [ ] **Step 1: Write WebSocket → PTY → output round-trip test**

Test the full flow: connect WebSocket, send input message, verify output message comes back. This validates the reconnection/replay behavior with richer PTY output.

```rust
#[tokio::test]
async fn test_ws_terminal_round_trip() {
    // 1. Set up a test app with a PTY session running /bin/cat
    // 2. Connect WebSocket to /ws/terminal/{session_id}
    // 3. Receive pty_status message (alive: true)
    // 4. Send { type: "input", data: "test\n" }
    // 5. Receive { type: "output", data: "..." } containing "test"
    // 6. Disconnect WebSocket
    // 7. Reconnect — verify disk log replay includes previous output
}
```

Use the existing test patterns in the codebase (`app.inject()` for HTTP, `tokio-tungstenite` or equivalent for WebSocket).

- [ ] **Step 2: Write resize propagation test**

```rust
#[tokio::test]
async fn test_ws_resize_propagates() {
    // 1. Set up PTY session
    // 2. Connect WebSocket
    // 3. Send { type: "resize", cols: 120, rows: 40 }
    // 4. Verify no error (resize is best-effort)
}
```

- [ ] **Step 3: Run tests**

Run: `cd apps/daemon && cargo test ws_terminal -- --nocapture`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/api/ws_terminal.rs
git commit -m "test: add WebSocket terminal round-trip and resize tests"
```

---

### Task 15: Keep session detail page working

**Files:**
- Verify: `apps/web/src/app/dashboard/sessions/[id]/page.tsx`

The session detail page (`/dashboard/sessions/[id]`) still works for direct URL access or deep linking. It shows the same TerminalPanel. No changes needed unless it imports something that was removed.

- [ ] **Step 1: Verify it compiles**

Run: `cd apps/web && npm run build`
Expected: Builds cleanly. If import errors, fix them.

- [ ] **Step 2: Commit if changes needed**

---

### Task 16: Full build and test

**Files:** All

- [ ] **Step 1: Build Rust daemon**

Run: `cd apps/daemon && cargo build`
Expected: Clean build.

- [ ] **Step 2: Run Rust tests**

Run: `cd apps/daemon && cargo test`
Expected: All pass.

- [ ] **Step 3: Run clippy**

Run: `cd apps/daemon && cargo clippy -- -D warnings`
Expected: Clean.

- [ ] **Step 4: Build frontend**

Run: `cd apps/web && npm run build`
Expected: Clean build.

- [ ] **Step 5: Run biome**

Run: `npx biome check --write .`
Expected: Clean.

- [ ] **Step 6: Manual smoke test**

Start the daemon and web app:
```bash
npm run dev
```

1. Open browser to localhost:3000
2. Send a task via Vigil chat
3. Click the session in SessionMonitor → terminal panel opens
4. Verify Claude Code TUI renders with colors, panels, markdown
5. Type in the terminal → input reaches Claude Code
6. Maximize terminal → full-screen mode
7. Minimize → back to split
8. Close → panel dismissed
9. Verify Vigil can still answer blockers
10. Verify session completes and terminal shows read-only state

- [ ] **Step 7: Final commit**

```bash
git add -A
git commit -m "feat: complete real PTY terminal access implementation"
```
