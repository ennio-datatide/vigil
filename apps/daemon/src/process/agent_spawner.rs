//! Agent process spawning and lifecycle management.
//!
//! Creates git worktrees, installs hooks, spawns the `claude` CLI as a child
//! process with piped I/O, and handles process exit to update session status.

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
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
        let child = Self::spawn_claude_process(&work_dir, continue_session)?;

        // Wire I/O and monitor the process.
        self.wire_process(child, &session_id, &session.prompt).await;

        // Emit session spawned event.
        if let Ok(Some(updated)) = store.get(&session_id).await {
            let _ = self.event_bus.emit(AppEvent::SessionSpawned { session: updated });
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
    fn spawn_claude_process(work_dir: &str, continue_session: bool) -> anyhow::Result<Child> {
        let mut args: Vec<&str> = vec!["--verbose", "--dangerously-skip-permissions"];
        if continue_session {
            args.push("--continue");
        }

        let child = Command::new("claude")
            .args(&args)
            .current_dir(work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(child)
    }

    /// Register PTY handle, wire stdin/stdout/stderr, send prompt, and
    /// spawn the exit-monitoring task.
    async fn wire_process(&self, mut child: Child, session_id: &str, prompt: &str) {
        let child_stdin = child.stdin.take().expect("stdin was piped");
        let child_stdout = child.stdout.take().expect("stdout was piped");
        let child_stderr = child.stderr.take().expect("stderr was piped");

        // Set up PTY handle.
        let alive = Arc::new(AtomicBool::new(true));
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

        self.pty_manager
            .register(session_id, PtyHandle { stdin_tx, alive: Arc::clone(&alive) })
            .await;
        self.output_manager.ensure_buffer(session_id).await;

        // Forward stdin_rx -> child stdin.
        let stdin_alive = Arc::clone(&alive);
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let mut writer = child_stdin;
            while let Some(data) = stdin_rx.recv().await {
                if !stdin_alive.load(Ordering::Relaxed) {
                    break;
                }
                if writer.write_all(&data).await.is_err() {
                    break;
                }
                let _ = writer.flush().await;
            }
        });

        // Read stdout -> output manager.
        Self::spawn_output_reader(child_stdout, session_id, Arc::clone(&self.output_manager));

        // Read stderr -> output manager.
        Self::spawn_output_reader(child_stderr, session_id, Arc::clone(&self.output_manager));

        // Send the prompt after a brief delay.
        let prompt_owned = prompt.to_owned();
        let pty_mgr = Arc::clone(&self.pty_manager);
        let sid = session_id.to_owned();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let mut data = prompt_owned.into_bytes();
            data.push(b'\n');
            pty_mgr.write(&sid, &data).await;
        });

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

    /// Spawn a task that reads lines from an async reader and appends to the
    /// output manager.
    fn spawn_output_reader<R>(reader: R, session_id: &str, output_manager: Arc<OutputManager>)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let sid = session_id.to_owned();
        tokio::spawn(async move {
            let buf_reader = BufReader::new(reader);
            let mut lines = buf_reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut data = line.into_bytes();
                data.push(b'\n');
                output_manager.append(&sid, &data).await;
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
}
