//! Periodic worktree cleanup service.
//!
//! Runs on a background timer to garbage-collect git worktrees from
//! completed sessions that are older than a retention period.

#![allow(dead_code)] // Service is wired up in Task 1.16.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::db::sqlite::SqliteDb;
use crate::deps::AppDeps;

/// Returns the current Unix timestamp in milliseconds.
fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

/// Periodically removes git worktrees from old, terminal sessions.
///
/// Worktrees are only removed if the session is in a terminal state,
/// ended more than 24 hours ago, and has no uncommitted changes.
pub(crate) struct CleanupService {
    #[allow(dead_code)] // Will be used for worktree_base path in future.
    config: Arc<Config>,
    db: Arc<SqliteDb>,
}

impl CleanupService {
    /// Create a new cleanup service from the shared application dependencies.
    #[must_use]
    pub(crate) fn new(deps: &AppDeps) -> Self {
        Self {
            config: deps.config.clone(),
            db: deps.db.clone(),
        }
    }

    /// Start the periodic cleanup loop as a background task.
    ///
    /// Runs every hour. Returns a [`JoinHandle`](tokio::task::JoinHandle)
    /// that the caller should store and abort on shutdown.
    pub(crate) fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3600));
            loop {
                interval.tick().await;
                if let Err(e) = self.cleanup_worktrees().await {
                    tracing::error!(error = %e, "worktree cleanup failed");
                }
            }
        })
    }

    /// Remove worktrees from old terminal sessions.
    async fn cleanup_worktrees(&self) -> anyhow::Result<()> {
        let retention_ms: i64 = 24 * 60 * 60 * 1000; // 24 hours
        let cutoff = unix_ms() - retention_ms;

        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, worktree_path FROM sessions \
             WHERE status IN ('completed', 'failed', 'cancelled', 'interrupted') \
               AND ended_at IS NOT NULL AND ended_at < ? \
               AND worktree_path IS NOT NULL",
        )
        .bind(cutoff)
        .fetch_all(self.db.pool())
        .await?;

        for (session_id, worktree_path) in rows {
            let Some(path) = worktree_path else { continue };
            let path = std::path::Path::new(&path);

            if !path.exists() {
                continue;
            }

            // Check for unmerged changes.
            let has_changes = std::process::Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(path)
                .output()
                .map(|o| !o.stdout.is_empty())
                .unwrap_or(true); // Skip if we can't check.

            if has_changes {
                tracing::info!(%session_id, "skipping worktree with unmerged changes");
                continue;
            }

            // Remove the worktree.
            let result = std::process::Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(path)
                .output();

            match result {
                Ok(output) if output.status.success() => {
                    tracing::info!(%session_id, "removed worktree");
                }
                Ok(output) => {
                    tracing::warn!(
                        %session_id,
                        stderr = %String::from_utf8_lossy(&output.stderr),
                        "worktree removal failed"
                    );
                }
                Err(e) => {
                    tracing::warn!(%session_id, error = %e, "failed to run git worktree remove");
                }
            }
        }

        Ok(())
    }
}
