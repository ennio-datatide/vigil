//! Session persistence and query service.
//!
//! [`SessionStore`] provides CRUD operations over the `sessions` table,
//! mapping between the `SQLite` TEXT/JSON columns and domain model types.

use std::sync::Arc;

use serde::Deserialize;
use sqlx::Row;

use crate::db::models::{
    AgentType, ExitReason, GitMetadata, Session, SessionRole, SessionStatus, SpawnType,
};
use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Result};

/// Input for creating a new session.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // `skip_permissions` is accepted from the API but not used in the store
pub struct CreateSessionInput {
    pub project_path: String,
    pub prompt: String,
    pub skill: Option<String>,
    pub role: Option<SessionRole>,
    pub parent_id: Option<String>,
    pub spawn_type: Option<SpawnType>,
    pub skip_permissions: Option<bool>,
    pub pipeline_id: Option<String>,
}

/// Manages session records in the database.
#[derive(Clone, Debug)]
pub(crate) struct SessionStore {
    db: Arc<SqliteDb>,
}

impl SessionStore {
    /// Create a new store backed by the given database.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>) -> Self {
        Self { db }
    }

    /// List all sessions, most recent first.
    ///
    /// # Errors
    ///
    /// Returns an error if the query or deserialization fails.
    pub(crate) async fn list(&self) -> Result<Vec<Session>> {
        let rows = sqlx::query(
            "SELECT id, project_path, worktree_path, tmux_session, prompt, skills_used, \
             status, agent_type, role, parent_id, spawn_type, spawn_result, retry_count, \
             started_at, ended_at, exit_reason, git_metadata, pipeline_id, pipeline_step_index \
             FROM sessions ORDER BY rowid DESC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        rows.iter().map(row_to_session).collect()
    }

    /// Get a single session by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the query or deserialization fails.
    pub(crate) async fn get(&self, id: &str) -> Result<Option<Session>> {
        let row = sqlx::query(
            "SELECT id, project_path, worktree_path, tmux_session, prompt, skills_used, \
             status, agent_type, role, parent_id, spawn_type, spawn_result, retry_count, \
             started_at, ended_at, exit_reason, git_metadata, pipeline_id, pipeline_step_index \
             FROM sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await
        .map_err(DbError::from)?;

        row.as_ref().map(row_to_session).transpose()
    }

    /// Insert a new session and return the created record.
    ///
    /// # Errors
    ///
    /// Returns an error if the insert fails.
    pub(crate) async fn create(&self, id: &str, input: &CreateSessionInput) -> Result<Session> {
        let skills_used_json: Option<String> = input
            .skill
            .as_ref()
            .map(|s| serde_json::to_string(&vec![s]).expect("vec serialization cannot fail"));

        let role_text: Option<&str> = input.role.as_ref().map(role_to_str);
        let spawn_type_text: Option<&str> = input.spawn_type.as_ref().map(spawn_type_to_str);

        sqlx::query(
            "INSERT INTO sessions (id, project_path, prompt, skills_used, status, agent_type, \
             role, parent_id, spawn_type, retry_count, pipeline_id) \
             VALUES (?, ?, ?, ?, 'queued', 'claude', ?, ?, ?, 0, ?)",
        )
        .bind(id)
        .bind(&input.project_path)
        .bind(&input.prompt)
        .bind(&skills_used_json)
        .bind(role_text)
        .bind(&input.parent_id)
        .bind(spawn_type_text)
        .bind(&input.pipeline_id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        // Return the freshly inserted row.
        self.get(id)
            .await?
            .ok_or_else(|| DbError::Sqlite(sqlx::Error::RowNotFound).into())
    }

    /// Update a session's status and optionally its exit reason and `ended_at`.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is not found or the update fails.
    pub(crate) async fn update_status(
        &self,
        id: &str,
        status: SessionStatus,
        exit_reason: Option<ExitReason>,
        ended_at: Option<i64>,
    ) -> Result<Session> {
        let status_text = status_to_str(&status);
        let exit_reason_text: Option<&str> = exit_reason.as_ref().map(exit_reason_to_str);

        let result = sqlx::query(
            "UPDATE sessions SET status = ?, exit_reason = ?, ended_at = ? WHERE id = ?",
        )
        .bind(status_text)
        .bind(exit_reason_text)
        .bind(ended_at)
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(crate::error::Error::NotFound(format!("session {id}")));
        }

        self.get(id)
            .await?
            .ok_or_else(|| crate::error::Error::NotFound(format!("session {id}")))
    }

    /// Transition a session to running state with worktree, start time, and git metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is not found or the update fails.
    #[allow(dead_code)] // Called by agent spawner (Task 1.12).
    pub(crate) async fn update_running(
        &self,
        id: &str,
        worktree_path: Option<&str>,
        started_at: i64,
        git_metadata: Option<&GitMetadata>,
    ) -> Result<Session> {
        let git_json: Option<String> = git_metadata
            .map(|m| serde_json::to_string(m).expect("GitMetadata serialization cannot fail"));

        let result = sqlx::query(
            "UPDATE sessions SET status = 'running', worktree_path = ?, started_at = ?, \
             git_metadata = ? WHERE id = ?",
        )
        .bind(worktree_path)
        .bind(started_at)
        .bind(&git_json)
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(crate::error::Error::NotFound(format!("session {id}")));
        }

        self.get(id)
            .await?
            .ok_or_else(|| crate::error::Error::NotFound(format!("session {id}")))
    }

    /// Reset a session back to queued state (for restart).
    ///
    /// # Errors
    ///
    /// Returns an error if the session is not found or the update fails.
    pub(crate) async fn reset_to_queued(&self, id: &str) -> Result<Session> {
        let result = sqlx::query(
            "UPDATE sessions SET status = 'queued', ended_at = NULL, exit_reason = NULL WHERE id = ?",
        )
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(crate::error::Error::NotFound(format!("session {id}")));
        }

        self.get(id)
            .await?
            .ok_or_else(|| crate::error::Error::NotFound(format!("session {id}")))
    }

    /// Set the `pipeline_step_index` on a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is not found or the update fails.
    #[allow(dead_code)] // Called by session manager (Task 1.13).
    pub(crate) async fn set_pipeline_step_index(
        &self,
        id: &str,
        step_index: i32,
    ) -> Result<Session> {
        let result =
            sqlx::query("UPDATE sessions SET pipeline_step_index = ? WHERE id = ?")
                .bind(step_index)
                .bind(id)
                .execute(self.db.pool())
                .await
                .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(crate::error::Error::NotFound(format!("session {id}")));
        }

        self.get(id)
            .await?
            .ok_or_else(|| crate::error::Error::NotFound(format!("session {id}")))
    }

    /// Hard-delete a session from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the delete fails.
    pub(crate) async fn delete(&self, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(crate::error::Error::NotFound(format!("session {id}")));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Row mapping helpers
// ---------------------------------------------------------------------------

/// Map a raw `sqlx::sqlite::SqliteRow` to a [`Session`] domain model.
pub(crate) fn row_to_session(row: &sqlx::sqlite::SqliteRow) -> Result<Session> {
    let skills_used: Option<Vec<String>> = row
        .get::<Option<String>, _>("skills_used")
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(DbError::from)?;

    let git_metadata: Option<GitMetadata> = row
        .get::<Option<String>, _>("git_metadata")
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(DbError::from)?;

    let status = parse_status(row.get::<String, _>("status").as_str());
    let agent_type = parse_agent_type(row.get::<String, _>("agent_type").as_str());
    let role: Option<SessionRole> = row
        .get::<Option<String>, _>("role")
        .map(|s| parse_role(&s));
    let exit_reason: Option<ExitReason> = row
        .get::<Option<String>, _>("exit_reason")
        .map(|s| parse_exit_reason(&s));
    let spawn_type: Option<SpawnType> = row
        .get::<Option<String>, _>("spawn_type")
        .map(|s| parse_spawn_type(&s));

    Ok(Session {
        id: row.get("id"),
        project_path: row.get("project_path"),
        worktree_path: row.get("worktree_path"),
        tmux_session: row.get("tmux_session"),
        prompt: row.get("prompt"),
        skills_used,
        status,
        agent_type,
        role,
        parent_id: row.get("parent_id"),
        spawn_type,
        spawn_result: row.get("spawn_result"),
        retry_count: row.get("retry_count"),
        started_at: row.get("started_at"),
        ended_at: row.get("ended_at"),
        exit_reason,
        git_metadata,
        pipeline_id: row.get("pipeline_id"),
        pipeline_step_index: row.get("pipeline_step_index"),
    })
}

// ---------------------------------------------------------------------------
// Enum ↔ string conversion helpers
// ---------------------------------------------------------------------------

fn status_to_str(s: &SessionStatus) -> &'static str {
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

fn parse_status(s: &str) -> SessionStatus {
    match s {
        "queued" => SessionStatus::Queued,
        "running" => SessionStatus::Running,
        "needs_input" => SessionStatus::NeedsInput,
        "auth_required" => SessionStatus::AuthRequired,
        "completed" => SessionStatus::Completed,
        "cancelled" => SessionStatus::Cancelled,
        "interrupted" => SessionStatus::Interrupted,
        _ => SessionStatus::Failed,
    }
}

fn parse_agent_type(s: &str) -> AgentType {
    match s {
        "codex" => AgentType::Codex,
        _ => AgentType::Claude,
    }
}

fn role_to_str(r: &SessionRole) -> &'static str {
    match r {
        SessionRole::Implementer => "implementer",
        SessionRole::Reviewer => "reviewer",
        SessionRole::Fixer => "fixer",
        SessionRole::Custom => "custom",
    }
}

fn parse_role(s: &str) -> SessionRole {
    match s {
        "implementer" => SessionRole::Implementer,
        "reviewer" => SessionRole::Reviewer,
        "fixer" => SessionRole::Fixer,
        _ => SessionRole::Custom,
    }
}

fn exit_reason_to_str(r: &ExitReason) -> &'static str {
    match r {
        ExitReason::Completed => "completed",
        ExitReason::Error => "error",
        ExitReason::UserCancelled => "user_cancelled",
        ExitReason::ChainTriggered => "chain_triggered",
    }
}

fn parse_exit_reason(s: &str) -> ExitReason {
    match s {
        "completed" => ExitReason::Completed,
        "user_cancelled" => ExitReason::UserCancelled,
        "chain_triggered" => ExitReason::ChainTriggered,
        _ => ExitReason::Error,
    }
}

fn parse_spawn_type(s: &str) -> SpawnType {
    match s {
        "branch" => SpawnType::Branch,
        _ => SpawnType::Worker,
    }
}

fn spawn_type_to_str(t: &SpawnType) -> &'static str {
    match t {
        SpawnType::Branch => "branch",
        SpawnType::Worker => "worker",
    }
}

/// Check whether a status represents a terminal (finished) state.
pub(crate) fn is_terminal_status(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Completed
            | SessionStatus::Failed
            | SessionStatus::Cancelled
            | SessionStatus::Interrupted
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sqlite::SqliteDb;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Create an isolated test database with migrations applied.
    ///
    /// Returns the `TempDir` handle alongside the db so the directory
    /// (and its file) lives as long as the test needs it.
    async fn test_db() -> (Arc<SqliteDb>, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let db = SqliteDb::connect(&db_path)
            .await
            .expect("failed to connect to test db");
        (Arc::new(db), dir)
    }

    fn sample_input() -> CreateSessionInput {
        CreateSessionInput {
            project_path: "/tmp/my-project".into(),
            prompt: "fix the bug".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        }
    }

    #[tokio::test]
    async fn create_and_list_sessions() {
        let (db, _dir) = test_db().await;
        let store = SessionStore::new(db);

        // Initially empty.
        let sessions = store.list().await.unwrap();
        assert!(sessions.is_empty());

        // Create two sessions.
        let s1 = store.create("id-1", &sample_input()).await.unwrap();
        assert_eq!(s1.id, "id-1");
        assert_eq!(s1.status, SessionStatus::Queued);
        assert_eq!(s1.agent_type, AgentType::Claude);
        assert_eq!(s1.retry_count, 0);

        let mut input2 = sample_input();
        input2.prompt = "add tests".into();
        input2.skill = Some("tdd".into());
        let s2 = store.create("id-2", &input2).await.unwrap();
        assert_eq!(s2.skills_used, Some(vec!["tdd".to_string()]));

        // List returns both, most recent first.
        let sessions = store.list().await.unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "id-2");
        assert_eq!(sessions[1].id, "id-1");
    }

    #[tokio::test]
    async fn get_session_found_and_not_found() {
        let (db, _dir) = test_db().await;
        let store = SessionStore::new(db);

        store.create("id-1", &sample_input()).await.unwrap();

        // Found.
        let found = store.get("id-1").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().prompt, "fix the bug");

        // Not found.
        let missing = store.get("nonexistent").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn cancel_session() {
        let (db, _dir) = test_db().await;
        let store = SessionStore::new(db);

        store.create("id-1", &sample_input()).await.unwrap();

        let now_ms = 1_700_000_000_000_i64;
        let cancelled = store
            .update_status(
                "id-1",
                SessionStatus::Cancelled,
                Some(ExitReason::UserCancelled),
                Some(now_ms),
            )
            .await
            .unwrap();

        assert_eq!(cancelled.status, SessionStatus::Cancelled);
        assert_eq!(cancelled.exit_reason, Some(ExitReason::UserCancelled));
        assert_eq!(cancelled.ended_at, Some(now_ms));
    }

    #[tokio::test]
    async fn delete_session() {
        let (db, _dir) = test_db().await;
        let store = SessionStore::new(db);

        store.create("id-1", &sample_input()).await.unwrap();

        store.delete("id-1").await.unwrap();

        let sessions = store.list().await.unwrap();
        assert!(sessions.is_empty());

        // Deleting again should fail.
        let err = store.delete("id-1").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn restart_session_valid_state() {
        let (db, _dir) = test_db().await;
        let store = SessionStore::new(db);

        store.create("id-1", &sample_input()).await.unwrap();

        // Move to completed first.
        store
            .update_status(
                "id-1",
                SessionStatus::Completed,
                Some(ExitReason::Completed),
                Some(1_700_000_000_000),
            )
            .await
            .unwrap();

        // Reset to queued.
        let restarted = store.reset_to_queued("id-1").await.unwrap();
        assert_eq!(restarted.status, SessionStatus::Queued);
        assert!(restarted.ended_at.is_none());
        assert!(restarted.exit_reason.is_none());
    }

    #[tokio::test]
    async fn restart_nonexistent_session_fails() {
        let (db, _dir) = test_db().await;
        let store = SessionStore::new(db);

        let err = store.reset_to_queued("nonexistent").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn create_with_role_and_parent() {
        let (db, _dir) = test_db().await;
        let store = SessionStore::new(db);

        let input = CreateSessionInput {
            project_path: "/tmp/proj".into(),
            prompt: "review code".into(),
            skill: None,
            role: Some(SessionRole::Reviewer),
            parent_id: Some("parent-123".into()),
            spawn_type: None,
            skip_permissions: Some(true),
            pipeline_id: Some("pipe-1".into()),
        };

        let session = store.create("id-1", &input).await.unwrap();
        assert_eq!(session.role, Some(SessionRole::Reviewer));
        assert_eq!(session.parent_id, Some("parent-123".into()));
        assert_eq!(session.pipeline_id, Some("pipe-1".into()));
    }
}
