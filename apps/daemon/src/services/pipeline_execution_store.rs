//! Pipeline execution persistence and query service.
//!
//! [`PipelineExecutionStore`] provides CRUD operations over the
//! `pipeline_executions` table, mapping between `SQLite` TEXT/JSON columns
//! and the [`PipelineExecution`] domain model.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::Row;
use uuid::Uuid;

use crate::db::models::{PipelineExecution, PipelineExecutionStatus};
use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Error, Result};

/// Manages pipeline execution records in the database.
#[derive(Clone, Debug)]
pub(crate) struct PipelineExecutionStore {
    db: Arc<SqliteDb>,
}

#[allow(dead_code)] // Methods used by pipeline runner and REST endpoints (Task 6+).
impl PipelineExecutionStore {
    /// Create a new store backed by the given database.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>) -> Self {
        Self { db }
    }

    /// Create a new pipeline execution in `queued` state.
    ///
    /// # Errors
    ///
    /// Returns an error if the insert fails.
    pub(crate) async fn create(
        &self,
        pipeline_id: &str,
        initial_prompt: &str,
        project_path: &str,
    ) -> Result<PipelineExecution> {
        let id = Uuid::new_v4().to_string();
        let now_ms = unix_ms();

        sqlx::query(
            "INSERT INTO pipeline_executions \
             (id, pipeline_id, status, initial_prompt, project_path, \
              current_step_index, step_sessions, step_outputs, created_at) \
             VALUES (?, ?, 'queued', ?, ?, 0, '{}', '{}', ?)",
        )
        .bind(&id)
        .bind(pipeline_id)
        .bind(initial_prompt)
        .bind(project_path)
        .bind(now_ms)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        self.get(&id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline execution {id} not found after insert")))
    }

    /// Get a single pipeline execution by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the query or deserialization fails.
    pub(crate) async fn get(&self, id: &str) -> Result<Option<PipelineExecution>> {
        let maybe_row = sqlx::query(
            "SELECT id, pipeline_id, status, initial_prompt, project_path, \
             current_step_index, step_sessions, step_outputs, created_at, completed_at \
             FROM pipeline_executions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await
        .map_err(DbError::from)?;

        match maybe_row {
            Some(ref row) => Ok(Some(row_to_execution(row)?)),
            None => Ok(None),
        }
    }

    /// List all pipeline executions, most recent first.
    ///
    /// # Errors
    ///
    /// Returns an error if the query or deserialization fails.
    pub(crate) async fn list(&self) -> Result<Vec<PipelineExecution>> {
        let rows = sqlx::query(
            "SELECT id, pipeline_id, status, initial_prompt, project_path, \
             current_step_index, step_sessions, step_outputs, created_at, completed_at \
             FROM pipeline_executions ORDER BY created_at DESC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        rows.iter().map(row_to_execution).collect()
    }

    /// Update the status of a pipeline execution.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] if the execution does not exist.
    pub(crate) async fn update_status(
        &self,
        id: &str,
        status: PipelineExecutionStatus,
    ) -> Result<PipelineExecution> {
        let status_text = execution_status_to_str(&status);

        let result = sqlx::query(
            "UPDATE pipeline_executions SET status = ? WHERE id = ?",
        )
        .bind(status_text)
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound(format!("pipeline execution {id}")));
        }

        self.get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline execution {id}")))
    }

    /// Set the current step index for a pipeline execution.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] if the execution does not exist.
    pub(crate) async fn set_current_step(
        &self,
        id: &str,
        step_index: i32,
    ) -> Result<PipelineExecution> {
        let result = sqlx::query(
            "UPDATE pipeline_executions SET current_step_index = ? WHERE id = ?",
        )
        .bind(step_index)
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound(format!("pipeline execution {id}")));
        }

        self.get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline execution {id}")))
    }

    /// Record a session ID for a given step (read-modify-write the JSON map).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] if the execution does not exist.
    pub(crate) async fn record_step_session(
        &self,
        id: &str,
        step_id: &str,
        session_id: &str,
    ) -> Result<()> {
        let exec = self
            .get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline execution {id}")))?;

        let mut sessions = exec.step_sessions;
        sessions.insert(step_id.to_string(), session_id.to_string());

        let json = serde_json::to_string(&sessions).map_err(DbError::Serialization)?;

        sqlx::query("UPDATE pipeline_executions SET step_sessions = ? WHERE id = ?")
            .bind(&json)
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        Ok(())
    }

    /// Record output text for a given step (read-modify-write the JSON map).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] if the execution does not exist.
    pub(crate) async fn record_step_output(
        &self,
        id: &str,
        step_id: &str,
        output: &str,
    ) -> Result<()> {
        let exec = self
            .get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline execution {id}")))?;

        let mut outputs = exec.step_outputs;
        outputs.insert(step_id.to_string(), output.to_string());

        let json = serde_json::to_string(&outputs).map_err(DbError::Serialization)?;

        sqlx::query("UPDATE pipeline_executions SET step_outputs = ? WHERE id = ?")
            .bind(&json)
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        Ok(())
    }

    /// Mark a pipeline execution as completed with the current timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] if the execution does not exist.
    pub(crate) async fn mark_completed(&self, id: &str) -> Result<PipelineExecution> {
        let now_ms = unix_ms();

        let result = sqlx::query(
            "UPDATE pipeline_executions SET status = 'completed', completed_at = ? WHERE id = ?",
        )
        .bind(now_ms)
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound(format!("pipeline execution {id}")));
        }

        self.get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline execution {id}")))
    }

    /// Mark a pipeline execution as failed with the current timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] if the execution does not exist.
    pub(crate) async fn mark_failed(&self, id: &str) -> Result<PipelineExecution> {
        let now_ms = unix_ms();

        let result = sqlx::query(
            "UPDATE pipeline_executions SET status = 'failed', completed_at = ? WHERE id = ?",
        )
        .bind(now_ms)
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound(format!("pipeline execution {id}")));
        }

        self.get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline execution {id}")))
    }
}

// ---------------------------------------------------------------------------
// Row mapping helpers
// ---------------------------------------------------------------------------

/// Map a raw `sqlx::sqlite::SqliteRow` to a [`PipelineExecution`] domain model.
fn row_to_execution(row: &sqlx::sqlite::SqliteRow) -> Result<PipelineExecution> {
    let step_sessions_json: String = row.get("step_sessions");
    let step_outputs_json: String = row.get("step_outputs");

    let step_sessions: HashMap<String, String> =
        serde_json::from_str(&step_sessions_json).map_err(DbError::Serialization)?;
    let step_outputs: HashMap<String, String> =
        serde_json::from_str(&step_outputs_json).map_err(DbError::Serialization)?;

    let status = parse_execution_status(row.get::<String, _>("status").as_str());

    Ok(PipelineExecution {
        id: row.get("id"),
        pipeline_id: row.get("pipeline_id"),
        status,
        initial_prompt: row.get("initial_prompt"),
        project_path: row.get("project_path"),
        current_step_index: row.get("current_step_index"),
        step_sessions,
        step_outputs,
        created_at: row.get("created_at"),
        completed_at: row.get("completed_at"),
    })
}

// ---------------------------------------------------------------------------
// Enum ↔ string conversion helpers
// ---------------------------------------------------------------------------

/// Convert a [`PipelineExecutionStatus`] to its database string representation.
fn execution_status_to_str(s: &PipelineExecutionStatus) -> &'static str {
    match s {
        PipelineExecutionStatus::Queued => "queued",
        PipelineExecutionStatus::Running => "running",
        PipelineExecutionStatus::Completed => "completed",
        PipelineExecutionStatus::Failed => "failed",
    }
}

/// Parse a database string into a [`PipelineExecutionStatus`].
fn parse_execution_status(s: &str) -> PipelineExecutionStatus {
    match s {
        "queued" => PipelineExecutionStatus::Queued,
        "running" => PipelineExecutionStatus::Running,
        "completed" => PipelineExecutionStatus::Completed,
        _ => PipelineExecutionStatus::Failed,
    }
}

/// Current time as Unix milliseconds.
fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
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

    async fn test_db() -> (Arc<SqliteDb>, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let db = SqliteDb::connect(&db_path)
            .await
            .expect("failed to connect to test db");
        (Arc::new(db), dir)
    }

    #[tokio::test]
    async fn create_and_get_execution() {
        let (db, _dir) = test_db().await;
        let store = PipelineExecutionStore::new(db);

        let exec = store
            .create("pipe-1", "build the feature", "/tmp/proj")
            .await
            .unwrap();

        assert!(!exec.id.is_empty());
        assert_eq!(exec.pipeline_id, "pipe-1");
        assert_eq!(exec.status, PipelineExecutionStatus::Queued);
        assert_eq!(exec.initial_prompt, "build the feature");
        assert_eq!(exec.project_path, "/tmp/proj");
        assert_eq!(exec.current_step_index, 0);
        assert!(exec.step_sessions.is_empty());
        assert!(exec.step_outputs.is_empty());
        assert!(exec.created_at > 0);
        assert!(exec.completed_at.is_none());

        // Fetch by id.
        let fetched = store.get(&exec.id).await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id, exec.id);
        assert_eq!(fetched.pipeline_id, "pipe-1");

        // Non-existent returns None.
        let missing = store.get("nonexistent").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn update_status() {
        let (db, _dir) = test_db().await;
        let store = PipelineExecutionStore::new(db);

        let exec = store
            .create("pipe-1", "prompt", "/tmp/proj")
            .await
            .unwrap();
        assert_eq!(exec.status, PipelineExecutionStatus::Queued);

        let updated = store
            .update_status(&exec.id, PipelineExecutionStatus::Running)
            .await
            .unwrap();
        assert_eq!(updated.status, PipelineExecutionStatus::Running);

        // Verify persisted.
        let fetched = store.get(&exec.id).await.unwrap().unwrap();
        assert_eq!(fetched.status, PipelineExecutionStatus::Running);
    }

    #[tokio::test]
    async fn record_step_session_and_output() {
        let (db, _dir) = test_db().await;
        let store = PipelineExecutionStore::new(db);

        let exec = store
            .create("pipe-1", "prompt", "/tmp/proj")
            .await
            .unwrap();

        // Record a step session.
        store
            .record_step_session(&exec.id, "step-1", "sess-abc")
            .await
            .unwrap();

        // Record a step output.
        store
            .record_step_output(&exec.id, "step-1", "all tests pass")
            .await
            .unwrap();

        // Advance the step index.
        let updated = store.set_current_step(&exec.id, 1).await.unwrap();
        assert_eq!(updated.current_step_index, 1);

        // Verify the maps.
        let fetched = store.get(&exec.id).await.unwrap().unwrap();
        assert_eq!(fetched.step_sessions.get("step-1").unwrap(), "sess-abc");
        assert_eq!(fetched.step_outputs.get("step-1").unwrap(), "all tests pass");
        assert_eq!(fetched.current_step_index, 1);
    }

    #[tokio::test]
    async fn mark_completed() {
        let (db, _dir) = test_db().await;
        let store = PipelineExecutionStore::new(db);

        let exec = store
            .create("pipe-1", "prompt", "/tmp/proj")
            .await
            .unwrap();

        let completed = store.mark_completed(&exec.id).await.unwrap();
        assert_eq!(completed.status, PipelineExecutionStatus::Completed);
        assert!(completed.completed_at.is_some());
        assert!(completed.completed_at.unwrap() > 0);
    }

    #[tokio::test]
    async fn mark_failed() {
        let (db, _dir) = test_db().await;
        let store = PipelineExecutionStore::new(db);

        let exec = store
            .create("pipe-1", "prompt", "/tmp/proj")
            .await
            .unwrap();

        let failed = store.mark_failed(&exec.id).await.unwrap();
        assert_eq!(failed.status, PipelineExecutionStatus::Failed);
        assert!(failed.completed_at.is_some());
    }

    #[tokio::test]
    async fn list_executions() {
        let (db, _dir) = test_db().await;
        let store = PipelineExecutionStore::new(db);

        // Initially empty.
        let execs = store.list().await.unwrap();
        assert!(execs.is_empty());

        // Create two executions.
        store
            .create("pipe-1", "first prompt", "/tmp/proj1")
            .await
            .unwrap();
        store
            .create("pipe-2", "second prompt", "/tmp/proj2")
            .await
            .unwrap();

        let execs = store.list().await.unwrap();
        assert_eq!(execs.len(), 2);
    }
}
