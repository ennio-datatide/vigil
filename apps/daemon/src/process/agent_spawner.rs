//! Agent process spawning and lifecycle management.
//!
//! Creates git worktrees, installs hooks, spawns the `claude` CLI as a child
//! process with piped I/O, and handles process exit to update session status.

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::process::{Child, Command};

use crate::config::Config;
use crate::db::models::{ExitReason, GitMetadata, Session, SessionStatus};
use crate::db::sqlite::SqliteDb;
use crate::deps::AppDeps;
use crate::events::{AppEvent, EventBus};
use crate::process::output_manager::OutputManager;
use crate::process::pty_manager::{PtyHandle, PtyManager};
use crate::services::session_store::SessionStore;

/// Spawns and manages `claude` CLI processes for sessions.
#[allow(dead_code)] // Constructed by session manager (Task 1.13).
pub(crate) struct AgentSpawner {
    config: Arc<Config>,
    db: Arc<SqliteDb>,
    event_bus: Arc<EventBus>,
    pty_manager: Arc<PtyManager>,
    output_manager: Arc<OutputManager>,
}

#[allow(dead_code)] // Methods called by session manager (Task 1.13).
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

    /// Spawn an interactive agent session.
    ///
    /// 1. Creates a git worktree (if the project is a git repo).
    /// 2. Installs Claude Code hooks into the working directory.
    /// 3. Captures git metadata (repo name, branch, commit, remote).
    /// 4. Spawns `claude` as a child process with piped stdin/stdout/stderr.
    /// 5. Wires output to [`OutputManager`] and registers a [`PtyHandle`].
    /// 6. Monitors process exit and updates the session accordingly.
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

        // Build and spawn the claude process.
        let child = Self::spawn_claude_process(&work_dir, &session.prompt, continue_session)?;

        // Wire I/O and monitor the process.
        self.wire_process(child, &session_id).await;

        // Emit session spawned event.
        if let Ok(Some(updated)) = store.get(&session_id).await {
            let _ = self.event_bus.emit(AppEvent::SessionSpawned { session: updated });
        }

        Ok(())
    }

    /// Spawn an interactive agent session for a pipeline step.
    ///
    /// Unlike [`spawn_interactive()`](Self::spawn_interactive), this uses
    /// interactive mode (no `-p` flag) with real stdin piping so the terminal
    /// WebSocket can send user input. No worktree is created — the session
    /// runs directly in the project directory.
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

        // Spawn claude in interactive mode (no -p flag).
        let mut child = Command::new("claude")
            .args(["--dangerously-skip-permissions"])
            .current_dir(&work_dir)
            .env_remove("CLAUDECODE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Take ownership of child I/O handles.
        let child_stdin = child.stdin.take().expect("stdin was piped");
        let child_stdout = child.stdout.take().expect("stdout was piped");
        let child_stderr = child.stderr.take().expect("stderr was piped");

        let alive = Arc::new(AtomicBool::new(true));
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

        self.pty_manager
            .register(
                &session_id,
                PtyHandle {
                    stdin_tx,
                    alive: Arc::clone(&alive),
                },
            )
            .await;
        self.output_manager.ensure_buffer(&session_id).await;

        // Spawn stdin writer: sends prompt first, then forwards WebSocket input.
        let prompt_bytes = format!("{prompt}\n");
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let mut stdin_writer = tokio::io::BufWriter::new(child_stdin);

            // Send the initial prompt.
            if let Err(e) = stdin_writer.write_all(prompt_bytes.as_bytes()).await {
                tracing::error!(error = %e, "failed to write prompt to stdin");
                return;
            }
            let _ = stdin_writer.flush().await;

            // Forward WebSocket input to stdin.
            while let Some(data) = stdin_rx.recv().await {
                if stdin_writer.write_all(&data).await.is_err() {
                    break;
                }
                let _ = stdin_writer.flush().await;
            }
        });

        // Read stdout → output manager.
        Self::spawn_output_reader(child_stdout, &session_id, Arc::clone(&self.output_manager));

        // Read stderr → output manager.
        Self::spawn_output_reader(child_stderr, &session_id, Arc::clone(&self.output_manager));

        // Monitor process exit.
        Self::spawn_exit_monitor(
            child,
            &session_id,
            Arc::clone(&alive),
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

    /// Build and spawn the `claude` CLI as a child process with piped I/O.
    ///
    /// Uses `-p` (print mode) so the process runs the prompt and exits
    /// when done, instead of sitting in interactive mode forever.
    fn spawn_claude_process(
        work_dir: &str,
        prompt: &str,
        continue_session: bool,
    ) -> anyhow::Result<Child> {
        let mut args: Vec<String> = vec![
            "-p".to_string(),
            prompt.to_string(),
            "--verbose".to_string(),
            "--dangerously-skip-permissions".to_string(),
        ];
        if continue_session {
            args.push("--continue".to_string());
        }

        let child = Command::new("claude")
            .args(&args)
            .current_dir(work_dir)
            // Clear CLAUDECODE so the worker doesn't think it's nested
            // inside another Claude Code session.
            .env_remove("CLAUDECODE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(child)
    }

    /// Register PTY handle, wire stdout/stderr, and spawn the exit-monitoring
    /// task. Stdin is null since we use `-p` mode.
    async fn wire_process(&self, mut child: Child, session_id: &str) {
        let child_stdout = child.stdout.take().expect("stdout was piped");
        let child_stderr = child.stderr.take().expect("stderr was piped");

        // Set up PTY handle (no stdin channel needed for -p mode, but we
        // keep the registration so the terminal UI shows alive status).
        let alive = Arc::new(AtomicBool::new(true));
        let (stdin_tx, _stdin_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1);

        self.pty_manager
            .register(session_id, PtyHandle { stdin_tx, alive: Arc::clone(&alive) })
            .await;
        self.output_manager.ensure_buffer(session_id).await;

        // Read stdout -> output manager (chunk-based, not line-based).
        Self::spawn_output_reader(child_stdout, session_id, Arc::clone(&self.output_manager));

        // Read stderr -> output manager.
        Self::spawn_output_reader(child_stderr, session_id, Arc::clone(&self.output_manager));

        // Monitor process exit.
        Self::spawn_exit_monitor(
            child,
            session_id,
            Arc::clone(&alive),
            Arc::clone(&self.db),
            Arc::clone(&self.event_bus),
            Arc::clone(&self.pty_manager),
        );
    }

    /// Spawn a task that reads chunks from an async reader and appends to the
    /// output manager. Uses raw byte reads instead of line-based buffering so
    /// streaming output (ANSI sequences, progress) appears immediately.
    fn spawn_output_reader<R>(reader: R, session_id: &str, output_manager: Arc<OutputManager>)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let sid = session_id.to_owned();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        output_manager.append(&sid, &buf[..n]).await;
                    }
                }
            }
        });
    }

    /// Spawn a task that waits for the child process to exit and updates
    /// the session status in the database.
    fn spawn_exit_monitor(
        mut child: Child,
        session_id: &str,
        alive: Arc<AtomicBool>,
        db: Arc<SqliteDb>,
        event_bus: Arc<EventBus>,
        pty_manager: Arc<PtyManager>,
    ) {
        let sid = session_id.to_owned();
        tokio::spawn(async move {
            let exit_status = child.wait().await;
            alive.store(false, Ordering::Relaxed);

            let (status, exit_reason) = match exit_status {
                Ok(s) if s.success() => (SessionStatus::Completed, ExitReason::Completed),
                _ => (SessionStatus::Failed, ExitReason::Error),
            };

            let ended_at = chrono::Utc::now().timestamp_millis();
            let store = SessionStore::new(db);

            match store.update_status(&sid, status, Some(exit_reason), Some(ended_at)).await {
                Ok(updated) => {
                    let _ = event_bus.emit(AppEvent::SessionUpdate { session: updated });
                }
                Err(e) => {
                    tracing::error!(session_id = %sid, error = %e, "failed to update session on exit");
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
            assert!(meta.commit_hash.len() >= 40, "commit_hash should be a full SHA");
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
        assert!(text.contains("Active Children (2):"), "should show count of 2");
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
        assert!(block.is_none(), "should return None when all children are terminal");
    }

    #[tokio::test]
    async fn status_block_no_children() {
        let (db, _dir) = test_db().await;

        // Create parent with no children at all.
        insert_session(&db, "parent-1", "main task", None, None, SessionStatus::Running).await;

        let block = generate_children_status_block(&db, "parent-1")
            .await
            .unwrap();
        assert!(block.is_none(), "should return None when no children exist");
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
        assert!(text.contains("..."), "should truncate long prompts with ellipsis");
        assert!(
            !text.contains(&"A".repeat(100)),
            "should not contain the full 100-char prompt"
        );
    }
}
