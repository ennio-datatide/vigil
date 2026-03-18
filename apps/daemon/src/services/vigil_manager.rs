//! Vigil persistent PTY session manager.
//!
//! Owns the lifecycle of a single long-lived Claude Code interactive PTY
//! session used by the Vigil orchestrator. User messages are written to the
//! PTY and responses are detected via `Stop` hook events.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::config::Config;
use crate::db::sqlite::SqliteDb;
use crate::events::{AppEvent, EventBus};
use crate::process::output_manager::OutputManager;
use crate::process::pty_manager::{PtyHandle, PtyManager};

/// Well-known session ID for the Vigil PTY.
const VIGIL_SESSION_ID: &str = "vigil";

/// Manages the persistent Vigil PTY session.
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

impl VigilManager {
    /// Create a new `VigilManager` from individual dependencies.
    ///
    /// Does NOT spawn the PTY — call [`start()`](Self::start) after construction.
    #[must_use]
    pub(crate) fn new(
        pty_manager: Arc<PtyManager>,
        output_manager: Arc<OutputManager>,
        event_bus: Arc<EventBus>,
        config: Arc<Config>,
        db: Arc<SqliteDb>,
    ) -> Self {
        let vigil_dir = config.vigil_home.join("vigil");
        Self {
            pty_manager,
            output_manager,
            event_bus,
            config,
            db,
            session_id: VIGIL_SESSION_ID.to_owned(),
            busy: AtomicBool::new(false),
            pending_response: Mutex::new(None),
            vigil_dir,
        }
    }

    /// Full startup sequence: spawn PTY, start listeners, wait for readiness.
    ///
    /// # Errors
    ///
    /// Returns an error if PTY spawning fails.
    pub(crate) async fn start(self: &Arc<Self>) -> anyhow::Result<()> {
        self.spawn_vigil().await?;
        self.start_hook_listener();
        self.start_exit_monitor();
        self.wait_for_ready().await;
        Ok(())
    }

    /// Send a user message to Vigil and wait for the response.
    ///
    /// Returns the response text extracted from the `Stop` hook event.
    ///
    /// # Errors
    ///
    /// Returns an error if Vigil is already processing, the PTY write fails,
    /// Vigil dies during processing, or the 600-second timeout is reached.
    pub(crate) async fn send_message(&self, message: &str) -> anyhow::Result<String> {
        // Check busy flag — return error if another message is in-flight.
        if self.busy.swap(true, Ordering::Acquire) {
            return Err(anyhow::anyhow!("Vigil is processing another message"));
        }

        // Create response channel.
        let (tx, rx) = oneshot::channel();
        *self.pending_response.lock().await = Some(tx);

        // Write message to Vigil PTY.
        if let Err(e) = self
            .pty_manager
            .write(&self.session_id, format!("{message}\r").into_bytes())
            .await
        {
            self.busy.store(false, Ordering::Release);
            *self.pending_response.lock().await = None;
            return Err(e);
        }

        // Wait for Stop hook event with 600s timeout.
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(600), rx).await;

        self.busy.store(false, Ordering::Release);

        match result {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(anyhow::anyhow!("Vigil session died while processing")),
            Err(_) => {
                // Timeout — clear pending response.
                *self.pending_response.lock().await = None;
                Err(anyhow::anyhow!("Vigil response timeout (600s)"))
            }
        }
    }

    /// Returns `true` if Vigil is currently processing a message.
    #[must_use]
    #[allow(dead_code)] // Available for status checks.
    pub(crate) fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Spawn the Vigil PTY process.
    async fn spawn_vigil(&self) -> anyhow::Result<()> {
        // Ensure vigil directory exists.
        std::fs::create_dir_all(&self.vigil_dir)
            .map_err(|e| anyhow::anyhow!("failed to create vigil dir: {e}"))?;

        // Initialize git repo if not already present — Claude Code requires
        // a git repo to read project-level hooks from .claude/settings.json.
        let git_dir = self.vigil_dir.join(".git");
        if !git_dir.exists() {
            let _ = std::process::Command::new("git")
                .args(["init"])
                .current_dir(&self.vigil_dir)
                .output();
        }

        // Write MCP config.
        let mcp_config_path = self.vigil_dir.join("mcp-config.json");
        let daemon_url = format!("http://localhost:{}", self.config.server_port);
        write_mcp_config(&mcp_config_path, &daemon_url)?;

        // Write strategy prompt.
        let strategy_path = self.vigil_dir.join("strategy.md");
        let strategy_content = include_str!("../../prompts/vigil-strategy.md");
        std::fs::write(&strategy_path, strategy_content)
            .map_err(|e| anyhow::anyhow!("failed to write strategy prompt: {e}"))?;

        // Install hooks.
        crate::hooks::installer::HookInstaller::install(
            &self.vigil_dir,
            &self.session_id,
            self.config.server_port,
        )?;

        // Commit config files so Claude Code recognizes this as a project.
        let _ = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&self.vigil_dir)
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "vigil config"])
            .current_dir(&self.vigil_dir)
            .output();

        // Spawn PTY.
        let (master, child, reader, writer) =
            spawn_vigil_pty(&self.vigil_dir, &mcp_config_path, &strategy_path)?;

        let alive = Arc::new(AtomicBool::new(true));

        // Wire PTY I/O.
        let (stdin_tx, _reader_handle) = wire_pty_io(
            &self.session_id,
            reader,
            writer,
            Arc::clone(&self.output_manager),
            Arc::clone(&alive),
        );

        // Ensure output buffer exists.
        self.output_manager.ensure_buffer(&self.session_id).await;

        // Register PTY handle.
        self.pty_manager
            .register(
                &self.session_id,
                PtyHandle {
                    stdin_tx,
                    master,
                    child,
                    alive,
                },
            )
            .await;

        tracing::info!(session_id = %self.session_id, "Vigil PTY spawned");

        // Auto-accept the "trust this folder" prompt that Claude Code shows
        // on first run in a new directory. The TUI default selection is
        // "Yes, I trust this folder" — just send Enter to confirm.
        let pty_mgr = Arc::clone(&self.pty_manager);
        let sid = self.session_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let _ = pty_mgr.write(&sid, b"\r".to_vec()).await;
        });

        Ok(())
    }

    /// Subscribe to the event bus and listen for `Stop` hook events.
    fn start_hook_listener(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let mut rx = this.event_bus.subscribe();

        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if let AppEvent::HookEvent {
                    session_id,
                    event_type,
                    payload,
                } = &event
                    && session_id == &this.session_id
                    && event_type == "Stop"
                {
                    // Extract response from Stop payload.
                    // The hook payload wraps the Claude Code hook data under
                    // `data.stop_hook_result.result` or directly as `result`.
                    // The Stop hook payload has `last_assistant_message`
                    // containing Claude's response text.
                    let response = payload
                        .as_ref()
                        .and_then(|p| {
                            p.get("last_assistant_message")
                                .and_then(|r| r.as_str())
                        })
                        .unwrap_or("")
                        .to_string();

                    let mut pending = this.pending_response.lock().await;
                    if let Some(tx) = pending.take() {
                        let _ = tx.send(response);
                    }
                }
            }
        });
    }

    /// Monitor for Vigil PTY death and auto-restart.
    fn start_exit_monitor(self: &Arc<Self>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                if this.pty_manager.is_alive(&this.session_id).await {
                    continue;
                }

                tracing::warn!("Vigil PTY died, restarting...");

                // Cancel in-flight request.
                if let Some(tx) = this.pending_response.lock().await.take() {
                    let _ = tx.send("Vigil crashed, restarting...".to_string());
                }
                this.busy.store(false, Ordering::Release);

                // Wait before restart.
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                // Respawn.
                if let Err(e) = this.spawn_vigil().await {
                    tracing::error!(error = %e, "Failed to restart Vigil");
                    continue;
                }

                // Inject context from chat history.
                this.inject_context().await;

                // Persist system message.
                let chat_store =
                    crate::services::vigil_chat::VigilChatStore::new(Arc::clone(&this.db));
                let _ = chat_store.save_message("system", "Vigil restarted", None).await;
            }
        });
    }

    /// Inject recent chat history into the PTY after a restart.
    async fn inject_context(&self) {
        use std::fmt::Write as _;

        let chat_store =
            crate::services::vigil_chat::VigilChatStore::new(Arc::clone(&self.db));
        if let Ok(messages) = chat_store.list_messages(10, 0).await {
            if messages.is_empty() {
                return;
            }

            let mut context = String::from(
                "You are resuming after a restart. Recent conversation:\n\n",
            );
            for msg in messages.iter().rev() {
                let role = if msg.role == "user" { "User" } else { "You" };
                let _ = write!(context, "{role}: {}\n\n", msg.content);
            }

            let _ = self
                .pty_manager
                .write(&self.session_id, format!("{context}\r").into_bytes())
                .await;
        }
    }

    /// Wait for the first `Stop` event (indicating Vigil is ready) or timeout.
    async fn wait_for_ready(&self) {
        let mut rx = self.event_bus.subscribe();
        let sid = self.session_id.clone();
        let timeout = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            async move {
                while let Ok(event) = rx.recv().await {
                    if let AppEvent::HookEvent {
                        session_id,
                        event_type,
                        ..
                    } = &event
                        && session_id == &sid
                        && event_type == "Stop"
                    {
                        return;
                    }
                }
            },
        );
        if timeout.await.is_err() {
            tracing::warn!("Vigil readiness timeout (30s) — proceeding anyway");
        }
    }
}

// ---------------------------------------------------------------------------
// PTY spawning
// ---------------------------------------------------------------------------

/// Result of PTY allocation: (master, child, reader, writer).
type PtySpawnResult = (
    Box<dyn portable_pty::MasterPty + Send>,
    Box<dyn portable_pty::Child + Send + Sync>,
    Box<dyn std::io::Read + Send>,
    Box<dyn std::io::Write + Send>,
);

/// Spawn `claude` in interactive mode inside a real PTY for Vigil.
///
/// Unlike the regular `spawn_claude_pty()` in `agent_spawner.rs`, this adds
/// `--mcp-config`, `--tools ""`, and `--append-system-prompt-file` arguments,
/// and does NOT use `-p` (print mode).
fn spawn_vigil_pty(
    work_dir: &Path,
    mcp_config_path: &Path,
    strategy_path: &Path,
) -> anyhow::Result<PtySpawnResult> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| anyhow::anyhow!("PTY allocation failed: {e}"))?;

    let mut cmd = CommandBuilder::new("claude");
    cmd.cwd(work_dir);
    cmd.env("TERM", "xterm-256color");
    // Prevent nested Claude Code detection.
    cmd.env_remove("CLAUDE_CODE");
    cmd.env_remove("CLAUDE_CODE_ENTRYPOINT");
    cmd.env_remove("CLAUDECODE");

    // Vigil-specific args.
    cmd.arg("--mcp-config");
    cmd.arg(mcp_config_path.to_string_lossy().as_ref());
    cmd.arg("--append-system-prompt-file");
    cmd.arg(strategy_path.to_string_lossy().as_ref());
    cmd.arg("--verbose");
    cmd.arg("--dangerously-skip-permissions");
    // Disable all built-in tools so Vigil only uses MCP tools.
    cmd.arg("--tools");
    cmd.arg("");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| anyhow::anyhow!("Failed to spawn Vigil in PTY: {e}"))?;

    // Drop slave immediately — prevents EOF issues when child exits.
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| anyhow::anyhow!("Failed to clone PTY reader: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| anyhow::anyhow!("Failed to take PTY writer: {e}"))?;

    Ok((pair.master, child, reader, writer))
}

/// Wire PTY I/O: reader -> `OutputManager`, writer <- `stdin_tx` channel.
/// Sets `alive` to false when reader detects EOF (child exited).
fn wire_pty_io(
    session_id: &str,
    reader: Box<dyn std::io::Read + Send>,
    writer: Box<dyn std::io::Write + Send>,
    output_manager: Arc<OutputManager>,
    alive: Arc<AtomicBool>,
) -> (mpsc::Sender<Vec<u8>>, tokio::task::JoinHandle<()>) {
    let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(64);

    // Writer drain thread (blocking).
    tokio::task::spawn_blocking(move || {
        let mut writer = writer;
        while let Some(data) = stdin_rx.blocking_recv() {
            use std::io::Write;
            if writer.write_all(&data).is_err() {
                break;
            }
        }
    });

    // Reader thread (blocking) -> output manager.
    let sid = session_id.to_string();
    let reader_handle = tokio::task::spawn_blocking(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            use std::io::Read;
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    if let Ok(rt) = tokio::runtime::Handle::try_current() {
                        let om = Arc::clone(&output_manager);
                        let s = sid.clone();
                        drop(rt.spawn(async move {
                            om.append(&s, &chunk).await;
                        }));
                    }
                }
            }
        }
        alive.store(false, Ordering::Relaxed);
    });

    (stdin_tx, reader_handle)
}

// ---------------------------------------------------------------------------
// MCP config writer (moved from claude_cli.rs)
// ---------------------------------------------------------------------------

/// Write the MCP config JSON file for Vigil.
///
/// The config tells `claude` to spawn `pf mcp-serve` as a
/// subprocess, connecting via stdio transport.
fn write_mcp_config(path: &Path, daemon_url: &str) -> anyhow::Result<()> {
    let config = serde_json::json!({
        "mcpServers": {
            "vigil": {
                "command": "pf",
                "args": ["mcp-serve", "--daemon-url", daemon_url],
            }
        }
    });

    let content = serde_json::to_string_pretty(&config)?;
    std::fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("failed to write MCP config: {e}"))?;

    tracing::debug!(path = %path.display(), "wrote MCP config");

    Ok(())
}
