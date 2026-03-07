//! Sub-session spawning service.
//!
//! [`SubSessionService`] handles spawning child sessions (branch or worker)
//! from a running parent session, enforcing concurrency limits and managing
//! the lifecycle of child sessions.

#![allow(dead_code)] // Service is constructed by later tasks (Task 3.4).

use std::sync::Arc;

use serde::Deserialize;

use crate::db::models::{Session, SessionStatus, SpawnType};
use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Result};
use crate::events::{AppEvent, EventBus};
use crate::services::session_store::{row_to_session, CreateSessionInput, SessionStore};

/// Maximum number of concurrent branch children per parent session.
const MAX_BRANCHES: usize = 3;

/// Maximum number of concurrent worker children per parent session.
const MAX_WORKERS: usize = 5;

/// Maximum total concurrent (non-terminal) sessions across the entire system.
const MAX_TOTAL_CONCURRENT: usize = 10;

/// Input for spawning a sub-session.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpawnInput {
    /// Whether to spawn a branch (read-only fork) or worker (independent executor).
    pub spawn_type: SpawnType,
    /// The prompt/task for the child session.
    pub prompt: String,
    /// Optional skill to apply.
    pub skill: Option<String>,
}

/// Service for spawning and managing child sessions.
///
/// Branches are read-only forks of the parent context, used for thinking and
/// research. Workers are independent task executors with their own worktrees,
/// used for parallel implementation work.
pub(crate) struct SubSessionService {
    db: Arc<SqliteDb>,
    event_bus: Arc<EventBus>,
}

impl SubSessionService {
    /// Create a new sub-session service from shared dependencies.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>, event_bus: Arc<EventBus>) -> Self {
        Self { db, event_bus }
    }

    /// Spawn a child session from a parent.
    ///
    /// Validates that:
    /// 1. The parent session exists and is running.
    /// 2. The per-type concurrency limit (3 branches, 5 workers) is not exceeded.
    /// 3. The global concurrent session limit (10) is not exceeded.
    ///
    /// Creates the child session in the database with the parent's `project_path`,
    /// sets `parent_id` and `spawn_type`, and emits a [`AppEvent::ChildSpawned`] event.
    ///
    /// The actual process spawning (via `AgentSpawner`) is left to the caller so
    /// that this service can be tested without a real `claude` binary.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails or the database operation fails.
    pub(crate) async fn spawn(
        &self,
        parent_id: &str,
        input: &SpawnInput,
    ) -> Result<Session> {
        let store = SessionStore::new(Arc::clone(&self.db));

        // 1. Validate parent exists and is running.
        let parent = store
            .get(parent_id)
            .await?
            .ok_or_else(|| crate::error::Error::NotFound(format!("session {parent_id}")))?;

        if parent.status != SessionStatus::Running {
            return Err(crate::error::Error::BadRequest(format!(
                "parent session {parent_id} is not running (status: {})",
                status_display(&parent.status),
            )));
        }

        // 2. Check per-type limit.
        let active_count = self.count_active_children(parent_id, &input.spawn_type).await?;
        let limit = match input.spawn_type {
            SpawnType::Branch => MAX_BRANCHES,
            SpawnType::Worker => MAX_WORKERS,
        };
        if active_count >= limit {
            return Err(crate::error::Error::BadRequest(format!(
                "parent {parent_id} already has {active_count}/{limit} active {} children",
                spawn_type_label(&input.spawn_type),
            )));
        }

        // 3. Check global concurrent session limit.
        let total_active = self.count_all_active_sessions().await?;
        if total_active >= MAX_TOTAL_CONCURRENT {
            return Err(crate::error::Error::BadRequest(format!(
                "global concurrent session limit reached ({total_active}/{MAX_TOTAL_CONCURRENT})",
            )));
        }

        // 4. Create child session with spawn_type.
        let child_id = uuid::Uuid::new_v4().to_string();
        let create_input = CreateSessionInput {
            project_path: parent.project_path.clone(),
            prompt: input.prompt.clone(),
            skill: input.skill.clone(),
            role: None,
            parent_id: Some(parent_id.to_owned()),
            spawn_type: Some(input.spawn_type.clone()),
            skip_permissions: None,
            pipeline_id: None,
        };

        let child = store.create(&child_id, &create_input).await?;

        // 5. Emit event.
        let _ = self.event_bus.emit(AppEvent::ChildSpawned {
            parent_id: parent_id.to_owned(),
            child_id: child_id.clone(),
        });

        Ok(child)
    }

    /// List all children of a session, most recent first.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn list_children(&self, parent_id: &str) -> Result<Vec<Session>> {
        let rows = sqlx::query(
            "SELECT id, project_path, worktree_path, tmux_session, prompt, skills_used, \
             status, agent_type, role, parent_id, spawn_type, spawn_result, retry_count, \
             started_at, ended_at, exit_reason, git_metadata, pipeline_id, pipeline_step_index \
             FROM sessions WHERE parent_id = ? ORDER BY rowid DESC",
        )
        .bind(parent_id)
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        rows.iter().map(row_to_session).collect()
    }

    /// Store the result of a completed child session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is not found or the update fails.
    pub(crate) async fn store_result(&self, child_id: &str, result: &str) -> Result<()> {
        let rows_affected = sqlx::query("UPDATE sessions SET spawn_result = ? WHERE id = ?")
            .bind(result)
            .bind(child_id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?
            .rows_affected();

        if rows_affected == 0 {
            return Err(crate::error::Error::NotFound(format!("session {child_id}")));
        }

        Ok(())
    }

    /// Count active (non-terminal) children of a parent, filtered by spawn type.
    async fn count_active_children(
        &self,
        parent_id: &str,
        spawn_type: &SpawnType,
    ) -> Result<usize> {
        let spawn_type_str = match spawn_type {
            SpawnType::Branch => "branch",
            SpawnType::Worker => "worker",
        };

        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sessions \
             WHERE parent_id = ? AND spawn_type = ? \
             AND status NOT IN ('completed', 'failed', 'cancelled', 'interrupted')",
        )
        .bind(parent_id)
        .bind(spawn_type_str)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from)?;

        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let result = count.max(0) as usize;
        Ok(result)
    }

    /// Count all non-terminal sessions system-wide.
    async fn count_all_active_sessions(&self) -> Result<usize> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sessions \
             WHERE status NOT IN ('completed', 'failed', 'cancelled', 'interrupted')",
        )
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from)?;

        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let result = count.max(0) as usize;
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

fn status_display(s: &SessionStatus) -> &'static str {
    match s {
        SessionStatus::Queued => "queued",
        SessionStatus::Running => "running",
        SessionStatus::NeedsInput => "needs_input",
        SessionStatus::AuthRequired => "auth_required",
        SessionStatus::Completed => "completed",
        SessionStatus::Failed => "failed",
        SessionStatus::Cancelled => "cancelled",
        SessionStatus::Interrupted => "interrupted",
    }
}

fn spawn_type_label(t: &SpawnType) -> &'static str {
    match t {
        SpawnType::Branch => "branch",
        SpawnType::Worker => "worker",
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::ExitReason;
    use crate::db::sqlite::SqliteDb;
    use crate::events::EventBus;
    use tempfile::TempDir;

    /// Create an isolated test database with migrations applied.
    async fn test_db() -> (Arc<SqliteDb>, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let db = SqliteDb::connect(&db_path)
            .await
            .expect("failed to connect to test db");
        (Arc::new(db), dir)
    }

    /// Create a sub-session service for testing.
    fn test_service(db: Arc<SqliteDb>) -> SubSessionService {
        let event_bus = Arc::new(EventBus::new(64));
        SubSessionService::new(db, event_bus)
    }

    /// Helper: create a parent session in running state.
    async fn create_running_parent(db: &Arc<SqliteDb>, id: &str) {
        let store = SessionStore::new(Arc::clone(db));
        let input = CreateSessionInput {
            project_path: "/tmp/test-project".into(),
            prompt: "parent task".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        };
        store.create(id, &input).await.unwrap();
        store
            .update_status(id, SessionStatus::Running, None, None)
            .await
            .unwrap();
    }

    /// Helper: create a parent session in a non-running state.
    async fn create_completed_parent(db: &Arc<SqliteDb>, id: &str) {
        let store = SessionStore::new(Arc::clone(db));
        let input = CreateSessionInput {
            project_path: "/tmp/test-project".into(),
            prompt: "parent task".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        };
        store.create(id, &input).await.unwrap();
        store
            .update_status(
                id,
                SessionStatus::Completed,
                Some(ExitReason::Completed),
                Some(1_700_000_000_000),
            )
            .await
            .unwrap();
    }

    fn branch_input() -> SpawnInput {
        SpawnInput {
            spawn_type: SpawnType::Branch,
            prompt: "research the architecture".into(),
            skill: None,
        }
    }

    fn worker_input() -> SpawnInput {
        SpawnInput {
            spawn_type: SpawnType::Worker,
            prompt: "implement the feature".into(),
            skill: Some("tdd".into()),
        }
    }

    #[tokio::test]
    async fn spawn_branch_child() {
        let (db, _dir) = test_db().await;
        let service = test_service(Arc::clone(&db));

        create_running_parent(&db, "parent-1").await;

        let child = service.spawn("parent-1", &branch_input()).await.unwrap();

        assert_eq!(child.parent_id, Some("parent-1".into()));
        assert_eq!(child.spawn_type, Some(SpawnType::Branch));
        assert_eq!(child.project_path, "/tmp/test-project");
        assert_eq!(child.prompt, "research the architecture");
        assert_eq!(child.status, SessionStatus::Queued);
    }

    #[tokio::test]
    async fn spawn_worker_child() {
        let (db, _dir) = test_db().await;
        let service = test_service(Arc::clone(&db));

        create_running_parent(&db, "parent-1").await;

        let child = service.spawn("parent-1", &worker_input()).await.unwrap();

        assert_eq!(child.parent_id, Some("parent-1".into()));
        assert_eq!(child.spawn_type, Some(SpawnType::Worker));
        assert_eq!(child.skills_used, Some(vec!["tdd".to_string()]));
    }

    #[tokio::test]
    async fn list_children_returns_only_children_of_parent() {
        let (db, _dir) = test_db().await;
        let service = test_service(Arc::clone(&db));

        create_running_parent(&db, "parent-1").await;
        create_running_parent(&db, "parent-2").await;

        // Spawn children under different parents.
        service.spawn("parent-1", &branch_input()).await.unwrap();
        service.spawn("parent-1", &worker_input()).await.unwrap();
        service.spawn("parent-2", &branch_input()).await.unwrap();

        let children_1 = service.list_children("parent-1").await.unwrap();
        assert_eq!(children_1.len(), 2);
        for child in &children_1 {
            assert_eq!(child.parent_id, Some("parent-1".into()));
        }

        let children_2 = service.list_children("parent-2").await.unwrap();
        assert_eq!(children_2.len(), 1);
        assert_eq!(children_2[0].parent_id, Some("parent-2".into()));
    }

    #[tokio::test]
    async fn store_result_updates_spawn_result() {
        let (db, _dir) = test_db().await;
        let service = test_service(Arc::clone(&db));

        create_running_parent(&db, "parent-1").await;

        let child = service.spawn("parent-1", &branch_input()).await.unwrap();

        service
            .store_result(&child.id, "The architecture uses a layered pattern.")
            .await
            .unwrap();

        // Verify via list_children.
        let children = service.list_children("parent-1").await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(
            children[0].spawn_result,
            Some("The architecture uses a layered pattern.".into())
        );
    }

    #[tokio::test]
    async fn exceeding_branch_limit_returns_error() {
        let (db, _dir) = test_db().await;
        let service = test_service(Arc::clone(&db));

        create_running_parent(&db, "parent-1").await;

        // Spawn MAX_BRANCHES branch children.
        for _ in 0..MAX_BRANCHES {
            service.spawn("parent-1", &branch_input()).await.unwrap();
        }

        // The next one should fail.
        let result = service.spawn("parent-1", &branch_input()).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("3/3"),
            "error should mention the limit: {err_msg}"
        );
    }

    #[tokio::test]
    async fn exceeding_worker_limit_returns_error() {
        let (db, _dir) = test_db().await;
        let service = test_service(Arc::clone(&db));

        create_running_parent(&db, "parent-1").await;

        // Spawn MAX_WORKERS worker children.
        for _ in 0..MAX_WORKERS {
            service.spawn("parent-1", &worker_input()).await.unwrap();
        }

        // The next one should fail.
        let result = service.spawn("parent-1", &worker_input()).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("5/5"),
            "error should mention the limit: {err_msg}"
        );
    }

    #[tokio::test]
    async fn spawning_from_non_running_parent_returns_error() {
        let (db, _dir) = test_db().await;
        let service = test_service(Arc::clone(&db));

        create_completed_parent(&db, "parent-1").await;

        let result = service.spawn("parent-1", &branch_input()).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not running"),
            "error should say not running: {err_msg}"
        );
    }

    #[tokio::test]
    async fn spawning_from_nonexistent_parent_returns_not_found() {
        let (db, _dir) = test_db().await;
        let service = test_service(Arc::clone(&db));

        let result = service.spawn("nonexistent", &branch_input()).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not found"),
            "error should say not found: {err_msg}"
        );
    }

    #[tokio::test]
    async fn store_result_for_nonexistent_session_returns_error() {
        let (db, _dir) = test_db().await;
        let service = test_service(Arc::clone(&db));

        let result = service.store_result("nonexistent", "some result").await;
        assert!(result.is_err());
    }
}
