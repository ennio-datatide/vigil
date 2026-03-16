//! Agent process spawning and lifecycle management.
//!
//! Spawns the `claude` CLI inside a real OS PTY via `portable-pty`, wires
//! output through the [`OutputManager`], and monitors process exit to update
//! session status.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::db::models::{ExitReason, GitMetadata, Session, SessionStatus};
use crate::db::sqlite::SqliteDb;
use crate::deps::AppDeps;
use crate::events::{AppEvent, EventBus};
use crate::process::output_manager::OutputManager;
use crate::process::pty_manager::{PtyHandle, PtyManager};
use crate::services::session_store::SessionStore;

/// Spawns and manages `claude` CLI processes for sessions.
pub(crate) struct AgentSpawner {
    config: Arc<Config>,
    db: Arc<SqliteDb>,
    event_bus: Arc<EventBus>,
    pty_manager: Arc<PtyManager>,
    output_manager: Arc<OutputManager>,
}

impl AgentSpawner {
    /// Create a new spawner from the shared application dependencies.
    #[must_use]
    pub(crate) fn new(deps: &AppDeps) -> Self {
        Self {
            config: Arc::clone(&deps.config),
            db: Arc::clone(&deps.db),
            event_bus: Arc::clone(&deps.event_bus),
            pty_manager: Arc::clone(&deps.pty_manager),
            output_manager: Arc::clone(&deps.output_manager),
        }
    }

    /// Spawn an interactive agent session inside a real PTY.
    ///
    /// 1. Creates a git worktree (if the project is a git repo).
    /// 2. Installs Claude Code hooks into the working directory.
    /// 3. Captures git metadata (repo name, branch, commit, remote).
    /// 4. Spawns `claude` inside a PTY via `portable-pty`.
    /// 5. Wires output to [`OutputManager`] and registers a [`PtyHandle`].
    /// 6. Sends the initial prompt as PTY input.
    /// 7. Monitors process exit and updates the session accordingly.
    ///
    /// # Errors
    ///
    /// Returns an error if worktree creation, hook installation, or process
    /// spawning fails.
    pub(crate) async fn spawn_interactive(
        &self,
        session: &Session,
        continue_session: bool,
    ) -> anyhow::Result<()> {
        let session_id = session.id.clone();

        // Prepare working directory, hooks, and DB state.
        let (work_dir, store) = self.prepare_session(session).await?;

        // Pre-create Claude Code project directory for this worktree so the
        // "trust this folder?" prompt is skipped on first run.
        pre_trust_directory(&work_dir);

        // Spawn claude inside a real PTY.
        let (master, child, reader, writer) =
            match spawn_claude_pty(&work_dir, continue_session) {
                Ok(result) => result,
                Err(e) => {
                    // PTY allocation failed — mark session as Failed.
                    let store2 = SessionStore::new(Arc::clone(&self.db));
                    let _ = store2
                        .update_status(
                            &session_id,
                            SessionStatus::Failed,
                            Some(ExitReason::Error),
                            Some(chrono::Utc::now().timestamp_millis()),
                        )
                        .await;
                    if let Ok(Some(updated)) = store2.get(&session_id).await {
                        let _ = self.event_bus.emit(AppEvent::SessionUpdate {
                            session: updated,
                        });
                    }
                    return Err(e);
                }
            };

        let alive = Arc::new(AtomicBool::new(true));

        // Wire PTY I/O: reader → output manager, writer ← stdin channel.
        let (stdin_tx, _reader_handle) = wire_pty_io(
            &session_id,
            reader,
            writer,
            Arc::clone(&self.output_manager),
            Arc::clone(&alive),
        );

        // Ensure output buffer exists for this session.
        self.output_manager.ensure_buffer(&session_id).await;

        // Register PTY handle.
        self.pty_manager
            .register(
                &session_id,
                PtyHandle {
                    stdin_tx: stdin_tx.clone(),
                    master,
                    child,
                    alive: Arc::clone(&alive),
                },
            )
            .await;

        // Send initial prompt after a delay to let the TUI initialize.
        // The trust dialog is skipped via pre_trust_directory() above.
        let prompt_text = session.prompt.clone();
        let tx = stdin_tx.clone();
        tokio::spawn(async move {
            // Wait for Claude Code TUI to finish rendering
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let _ = tx.send(format!("{prompt_text}\r").into_bytes()).await;
        });

        // Spawn exit monitor — polls alive flag set by reader thread on EOF.
        Self::spawn_exit_monitor(
            &session_id,
            alive,
            Arc::clone(&self.db),
            Arc::clone(&self.event_bus),
            Arc::clone(&self.pty_manager),
        );

        // Emit session spawned event.
        if let Ok(Some(updated)) = store.get(&session_id).await {
            let _ = self.event_bus.emit(AppEvent::SessionSpawned {
                session: updated,
            });
        }

        Ok(())
    }

    /// Spawn an interactive agent session for a pipeline step.
    ///
    /// Unlike [`spawn_interactive()`](Self::spawn_interactive), this skips
    /// worktree creation and runs directly in the project directory with a
    /// custom prompt.
    pub(crate) async fn spawn_interactive_pipeline_step(
        &self,
        session: &Session,
        prompt: &str,
    ) -> anyhow::Result<()> {
        let session_id = session.id.clone();
        let work_dir = session.project_path.clone();

        // Install hooks.
        crate::hooks::installer::HookInstaller::install(
            Path::new(&work_dir),
            &session_id,
            self.config.server_port,
        )?;

        // Capture git metadata.
        let git_metadata = Self::capture_git_metadata(&work_dir);

        // Update session to running in DB.
        let store = SessionStore::new(Arc::clone(&self.db));
        let now_ms = chrono::Utc::now().timestamp_millis();
        store
            .update_running(&session_id, None, now_ms, git_metadata.as_ref())
            .await?;

        // Spawn claude inside a real PTY (no worktree, runs in project dir).
        let (master, child, reader, writer) = spawn_claude_pty(&work_dir, false)?;

        let alive = Arc::new(AtomicBool::new(true));

        // Wire PTY I/O.
        let (stdin_tx, _reader_handle) = wire_pty_io(
            &session_id,
            reader,
            writer,
            Arc::clone(&self.output_manager),
            Arc::clone(&alive),
        );

        // Ensure output buffer exists.
        self.output_manager.ensure_buffer(&session_id).await;

        // Register PTY handle.
        self.pty_manager
            .register(
                &session_id,
                PtyHandle {
                    stdin_tx: stdin_tx.clone(),
                    master,
                    child,
                    alive: Arc::clone(&alive),
                },
            )
            .await;

        // Send the pipeline prompt as input.
        let prompt_bytes = format!("{prompt}\r");
        stdin_tx
            .send(prompt_bytes.into_bytes())
            .await
            .map_err(|_| anyhow::anyhow!("Failed to send pipeline prompt"))?;

        // Spawn exit monitor.
        Self::spawn_exit_monitor(
            &session_id,
            alive,
            Arc::clone(&self.db),
            Arc::clone(&self.event_bus),
            Arc::clone(&self.pty_manager),
        );

        // Emit session spawned event.
        if let Ok(Some(updated)) = store.get(&session_id).await {
            let _ = self.event_bus.emit(AppEvent::SessionSpawned {
                session: updated,
            });
        }

        Ok(())
    }

    /// Kill a running agent by terminating its process.
    #[allow(dead_code)] // Available for direct agent termination.
    pub(crate) async fn kill(&self, session_id: &str) {
        self.pty_manager.kill(session_id).await;
        tracing::info!(session_id, "agent kill requested");
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Create worktree, install hooks, capture git metadata, and update the DB.
    async fn prepare_session(
        &self,
        session: &Session,
    ) -> anyhow::Result<(String, SessionStore)> {
        let session_id = &session.id;

        // Create worktree (or fall back to project dir).
        let worktree_path = self.create_worktree(&session.project_path, session_id)?;
        let work_dir = worktree_path
            .clone()
            .unwrap_or_else(|| session.project_path.clone());

        // Install hooks.
        crate::hooks::installer::HookInstaller::install(
            Path::new(&work_dir),
            session_id,
            self.config.server_port,
        )?;

        // Capture git metadata.
        let git_metadata = Self::capture_git_metadata(&work_dir);

        // Update session in DB.
        let store = SessionStore::new(Arc::clone(&self.db));
        let now_ms = chrono::Utc::now().timestamp_millis();
        store
            .update_running(session_id, worktree_path.as_deref(), now_ms, git_metadata.as_ref())
            .await?;

        Ok((work_dir, store))
    }

    /// Spawn a task that polls the `alive` flag (set by the reader thread on
    /// EOF) and updates session status when the child exits.
    fn spawn_exit_monitor(
        session_id: &str,
        alive: Arc<AtomicBool>,
        db: Arc<SqliteDb>,
        event_bus: Arc<EventBus>,
        pty_manager: Arc<PtyManager>,
    ) {
        let sid = session_id.to_owned();
        tokio::spawn(async move {
            // Wait for alive=false (set by reader thread on EOF, or by kill()).
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
                if !alive.load(Ordering::Relaxed) {
                    break;
                }
            }

            let store = SessionStore::new(db);
            if let Ok(Some(session)) = store.get(&sid).await
                && (session.status == SessionStatus::Running
                    || session.status == SessionStatus::NeedsInput)
            {
                let ended_at = chrono::Utc::now().timestamp_millis();
                match store
                    .update_status(
                        &sid,
                        SessionStatus::Completed,
                        Some(ExitReason::Completed),
                        Some(ended_at),
                    )
                    .await
                {
                    Ok(updated) => {
                        let _ =
                            event_bus.emit(AppEvent::SessionUpdate { session: updated });
                    }
                    Err(e) => {
                        tracing::error!(
                            session_id = %sid,
                            error = %e,
                            "failed to update session on exit"
                        );
                    }
                }
            }

            pty_manager.remove(&sid).await;
            tracing::info!(session_id = %sid, "agent process exited");
        });
    }

    /// Create a git worktree for the session.
    ///
    /// Returns `Some(worktree_path)` if creation succeeds, or `None` if the
    /// project is not a git repo or worktree creation fails.
    fn create_worktree(
        &self,
        project_path: &str,
        session_id: &str,
    ) -> anyhow::Result<Option<String>> {
        let worktree_path = self.config.worktree_base.join(session_id);
        let branch_name = format!("praefectus/{session_id}");

        let output = std::process::Command::new("git")
            .args(["worktree", "add", "-b", &branch_name])
            .arg(&worktree_path)
            .current_dir(project_path)
            .output()?;

        if output.status.success() {
            tracing::info!(
                session_id,
                worktree = %worktree_path.display(),
                "created git worktree",
            );
            Ok(Some(worktree_path.display().to_string()))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                session_id,
                stderr = %stderr,
                "worktree creation failed, using project dir",
            );
            Ok(None)
        }
    }

    /// Capture git metadata from the working directory.
    ///
    /// Returns `None` if the directory is not a git repository.
    fn capture_git_metadata(work_dir: &str) -> Option<GitMetadata> {
        let run = |args: &[&str]| -> Option<String> {
            std::process::Command::new("git")
                .args(args)
                .current_dir(work_dir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };

        Some(GitMetadata {
            repo_name: run(&["rev-parse", "--show-toplevel"])?
                .rsplit('/')
                .next()?
                .to_string(),
            branch: run(&["rev-parse", "--abbrev-ref", "HEAD"])?,
            commit_hash: run(&["rev-parse", "HEAD"])?,
            remote_url: run(&["remote", "get-url", "origin"]),
        })
    }
}

// ---------------------------------------------------------------------------
// PTY spawning and I/O wiring
// ---------------------------------------------------------------------------

/// Result of PTY allocation: (master, child, reader, writer).
type PtySpawnResult = (
    Box<dyn portable_pty::MasterPty + Send>,
    Box<dyn portable_pty::Child + Send + Sync>,
    Box<dyn std::io::Read + Send>,
    Box<dyn std::io::Write + Send>,
);

/// Spawn `claude` inside a real PTY. Returns (master, child, reader, writer).
fn spawn_claude_pty(
    work_dir: &str,
    continue_session: bool,
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
    // Prevent nested Claude Code detection (both env var forms).
    cmd.env_remove("CLAUDE_CODE");
    cmd.env_remove("CLAUDE_CODE_ENTRYPOINT");
    cmd.env_remove("CLAUDECODE");

    if continue_session {
        cmd.arg("--continue");
    }
    // --verbose enables hook events that Vigil relies on for structured state tracking.
    cmd.arg("--verbose");
    cmd.arg("--dangerously-skip-permissions");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| anyhow::anyhow!("Failed to spawn claude in PTY: {e}"))?;

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
    // Sets alive=false on EOF so the exit monitor can detect child death.
    let sid = session_id.to_string();
    let reader_handle = tokio::task::spawn_blocking(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            use std::io::Read;
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break, // EOF or error — child exited
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    // IMPORTANT: Cannot use handle.block_on() inside spawn_blocking
                    // (panics with "Cannot block_on from within a tokio runtime").
                    // Use handle.spawn() to dispatch async work non-blocking.
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
        // Signal that the child process has exited.
        alive.store(false, Ordering::Relaxed);
    });

    (stdin_tx, reader_handle)
}

// ---------------------------------------------------------------------------
// Trust pre-creation
// ---------------------------------------------------------------------------

/// Pre-create the Claude Code project directory for a working directory so the
/// "trust this folder?" prompt is skipped. Claude Code stores per-project data
/// in `~/.claude/projects/<encoded-path>/`. The presence of this directory
/// signals that the project was previously trusted.
fn pre_trust_directory(work_dir: &str) {
    if let Some(home) = dirs::home_dir() {
        // Encode path: replace / with - and prepend -
        let encoded = work_dir.replace('/', "-");
        let project_dir = home.join(".claude").join("projects").join(&encoded);
        let _ = std::fs::create_dir_all(&project_dir);
    }
}

// ---------------------------------------------------------------------------
// Status block generation
// ---------------------------------------------------------------------------

/// Generate a status block showing active children of a parent session.
///
/// Returns `None` if the parent has no non-terminal children.
#[allow(dead_code)] // Called by sub-session orchestration (Task 3.4+).
pub(crate) async fn generate_children_status_block(
    db: &SqliteDb,
    parent_id: &str,
) -> anyhow::Result<Option<String>> {
    use sqlx::Row;

    let rows = sqlx::query(
        "SELECT id, spawn_type, status, prompt FROM sessions \
         WHERE parent_id = ? AND status NOT IN ('completed', 'failed', 'cancelled', 'interrupted') \
         ORDER BY rowid ASC",
    )
    .bind(parent_id)
    .fetch_all(db.pool())
    .await?;

    if rows.is_empty() {
        return Ok(None);
    }

    let mut lines = vec![format!("Active Children ({}):", rows.len())];
    for row in &rows {
        let id: String = row.get("id");
        let spawn_type: Option<String> = row.get("spawn_type");
        let status: String = row.get("status");
        let prompt: String = row.get("prompt");

        let short_id = &id[..id.len().min(8)];
        let type_label = spawn_type.as_deref().unwrap_or("child");
        let short_prompt = if prompt.len() > 50 {
            format!("{}...", &prompt[..47])
        } else {
            prompt
        };

        lines.push(format!(
            "  [{type_label}] {short_id} — {status} — \"{short_prompt}\""
        ));
    }

    Ok(Some(lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_git_metadata_on_this_repo() {
        // This test runs against the actual praefectus repo.
        let crate_dir = env!("CARGO_MANIFEST_DIR");
        let meta = AgentSpawner::capture_git_metadata(crate_dir);

        // If running inside a git repo, we should get metadata.
        if let Some(meta) = meta {
            assert!(!meta.repo_name.is_empty(), "repo_name should not be empty");
            assert!(!meta.branch.is_empty(), "branch should not be empty");
            assert!(
                meta.commit_hash.len() >= 40,
                "commit_hash should be a full SHA"
            );
        }
        // If not in a git repo (e.g., extracted tarball), None is acceptable.
    }

    #[test]
    fn capture_git_metadata_non_git_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let meta = AgentSpawner::capture_git_metadata(dir.path().to_str().unwrap());
        assert!(meta.is_none(), "non-git directory should return None");
    }

    // -----------------------------------------------------------------------
    // Status block tests
    // -----------------------------------------------------------------------

    use crate::db::models::{SessionStatus, SpawnType};
    use crate::services::session_store::{CreateSessionInput, SessionStore};
    use std::sync::Arc;

    /// Create an isolated test database with migrations applied.
    async fn test_db() -> (Arc<SqliteDb>, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let db = SqliteDb::connect(&db_path)
            .await
            .expect("failed to connect to test db");
        (Arc::new(db), dir)
    }

    /// Helper: insert a session directly.
    async fn insert_session(
        db: &Arc<SqliteDb>,
        id: &str,
        prompt: &str,
        parent_id: Option<&str>,
        spawn_type: Option<SpawnType>,
        status: SessionStatus,
    ) {
        let store = SessionStore::new(Arc::clone(db));
        let input = CreateSessionInput {
            project_path: "/tmp/test-project".into(),
            prompt: prompt.into(),
            skill: None,
            role: None,
            parent_id: parent_id.map(String::from),
            spawn_type,
            skip_permissions: None,
            pipeline_id: None,
        };
        store.create(id, &input).await.unwrap();
        if status != SessionStatus::Queued {
            store.update_status(id, status, None, None).await.unwrap();
        }
    }

    #[tokio::test]
    async fn status_block_with_active_children() {
        let (db, _dir) = test_db().await;

        // Create parent.
        insert_session(&db, "parent-1", "main task", None, None, SessionStatus::Running).await;

        // Create two active children.
        insert_session(
            &db,
            "abc12345-full-id",
            "Research memory patterns and find best approach",
            Some("parent-1"),
            Some(SpawnType::Branch),
            SessionStatus::Queued,
        )
        .await;
        insert_session(
            &db,
            "def67890-full-id",
            "Implement the cache layer for the system",
            Some("parent-1"),
            Some(SpawnType::Worker),
            SessionStatus::Running,
        )
        .await;

        let block = generate_children_status_block(&db, "parent-1")
            .await
            .unwrap();
        assert!(block.is_some(), "should generate a status block");

        let text = block.unwrap();
        assert!(
            text.contains("Active Children (2):"),
            "should show count of 2"
        );
        assert!(text.contains("[branch]"), "should show branch type");
        assert!(text.contains("[worker]"), "should show worker type");
        assert!(text.contains("abc12345"), "should show truncated id");
        assert!(text.contains("def67890"), "should show truncated id");
        assert!(text.contains("queued"), "should show queued status");
        assert!(text.contains("running"), "should show running status");
    }

    #[tokio::test]
    async fn status_block_no_active_children() {
        let (db, _dir) = test_db().await;

        // Create parent.
        insert_session(&db, "parent-1", "main task", None, None, SessionStatus::Running).await;

        // Create a completed child — should be excluded.
        insert_session(
            &db,
            "child-done",
            "finished work",
            Some("parent-1"),
            Some(SpawnType::Branch),
            SessionStatus::Completed,
        )
        .await;

        let block = generate_children_status_block(&db, "parent-1")
            .await
            .unwrap();
        assert!(
            block.is_none(),
            "should return None when all children are terminal"
        );
    }

    #[tokio::test]
    async fn status_block_no_children() {
        let (db, _dir) = test_db().await;

        // Create parent with no children at all.
        insert_session(&db, "parent-1", "main task", None, None, SessionStatus::Running).await;

        let block = generate_children_status_block(&db, "parent-1")
            .await
            .unwrap();
        assert!(
            block.is_none(),
            "should return None when no children exist"
        );
    }

    #[tokio::test]
    async fn status_block_truncates_long_prompts() {
        let (db, _dir) = test_db().await;

        insert_session(&db, "parent-1", "main task", None, None, SessionStatus::Running).await;

        let long_prompt = "A".repeat(100);
        insert_session(
            &db,
            "child-long",
            &long_prompt,
            Some("parent-1"),
            Some(SpawnType::Worker),
            SessionStatus::Running,
        )
        .await;

        let block = generate_children_status_block(&db, "parent-1")
            .await
            .unwrap();
        let text = block.unwrap();

        // Truncated prompt should be 47 chars + "..." = 50 chars total.
        assert!(
            text.contains("..."),
            "should truncate long prompts with ellipsis"
        );
        assert!(
            !text.contains(&"A".repeat(100)),
            "should not contain the full 100-char prompt"
        );
    }
}

#[cfg(test)]
mod pty_spawn_tests {
    use super::*;
    use crate::process::output_manager::OutputManager;
    use crate::process::pty_manager::{PtyHandle, PtyManager};
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_pty_spawn_and_output_capture() {
        let tmp = tempfile::TempDir::new().unwrap();
        let output_manager = Arc::new(OutputManager::new(tmp.path().to_path_buf()));
        let pty_manager = Arc::new(PtyManager::new());

        let session_id = "test-pty-spawn";
        let alive = Arc::new(AtomicBool::new(true));

        // Allocate PTY
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open pty");

        // Spawn /bin/sh
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd).expect("spawn sh");
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().expect("clone reader");
        let writer = pair.master.take_writer().expect("take writer");

        // Use wire_pty_io to set up I/O
        let (stdin_tx, _reader_handle) = wire_pty_io(
            session_id,
            reader,
            writer,
            Arc::clone(&output_manager),
            Arc::clone(&alive),
        );

        // Ensure output buffer exists
        output_manager.ensure_buffer(session_id).await;

        // Register PTY
        pty_manager
            .register(
                session_id,
                PtyHandle {
                    stdin_tx: stdin_tx.clone(),
                    master: pair.master,
                    child,
                    alive: Arc::clone(&alive),
                },
            )
            .await;

        // Send a command
        pty_manager
            .write(session_id, b"echo HELLO_PTY\n".to_vec())
            .await
            .unwrap();
        sleep(Duration::from_millis(300)).await;

        // Check output manager captured it
        let buf = output_manager.get_buffer(session_id).await;
        let raw = buf.unwrap_or_default();
        let output = String::from_utf8_lossy(&raw);
        assert!(
            output.contains("HELLO_PTY"),
            "Expected output in buffer, got: {output}"
        );

        // Cleanup — send exit, verify alive flag gets set to false
        pty_manager
            .write(session_id, b"exit\n".to_vec())
            .await
            .unwrap();
        sleep(Duration::from_millis(500)).await;
        assert!(
            !alive.load(Ordering::Relaxed),
            "alive should be false after exit"
        );

        pty_manager.kill(session_id).await;
    }
}
