//! Startup recovery service.
//!
//! On daemon startup, finds sessions stuck in `running` or `queued` status
//! (orphaned from a previous crash) and marks them as `interrupted`.

#![allow(dead_code)] // Service is wired up in Task 1.16.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::sqlite::SqliteDb;
use crate::deps::AppDeps;
use crate::events::EventBus;

/// Returns the current Unix timestamp in milliseconds.
fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

/// Recovers orphaned sessions on startup.
///
/// Sessions left in `running` or `queued` state from a previous daemon crash
/// are transitioned to `interrupted` so they can be retried or cleaned up.
pub(crate) struct RecoveryService {
    db: Arc<SqliteDb>,
    #[allow(dead_code)] // Will be used to emit recovery events in future.
    event_bus: Arc<EventBus>,
}

impl RecoveryService {
    /// Create a new recovery service from the shared application dependencies.
    #[must_use]
    pub(crate) fn new(deps: &AppDeps) -> Self {
        Self {
            db: deps.db.clone(),
            event_bus: deps.event_bus.clone(),
        }
    }

    /// Run recovery -- mark orphaned sessions as interrupted.
    ///
    /// Returns the number of sessions that were recovered.
    ///
    /// # Errors
    ///
    /// Returns an error if the database update fails.
    pub(crate) async fn run(&self) -> anyhow::Result<u64> {
        let now = unix_ms();

        let result = sqlx::query(
            "UPDATE sessions SET status = 'interrupted', ended_at = ?, exit_reason = 'error' \
             WHERE status IN ('running', 'queued')",
        )
        .bind(now)
        .execute(self.db.pool())
        .await?;

        let count = result.rows_affected();
        if count > 0 {
            tracing::info!(count, "recovered orphaned sessions");
        }

        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::SessionStatus;
    use crate::services::session_store::{CreateSessionInput, SessionStore};
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

    fn sample_input() -> CreateSessionInput {
        CreateSessionInput {
            project_path: "/tmp/proj".into(),
            prompt: "do work".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        }
    }

    fn make_service(db: &Arc<SqliteDb>, _dir: &TempDir) -> RecoveryService {
        let event_bus = Arc::new(EventBus::new(64));
        RecoveryService {
            db: db.clone(),
            event_bus,
        }
    }

    #[tokio::test]
    async fn recovers_running_sessions() {
        let (db, dir) = test_db().await;
        let store = SessionStore::new(db.clone());

        // Create a session and move it to running.
        store.create("s-1", &sample_input()).await.unwrap();
        store
            .update_status("s-1", SessionStatus::Running, None, None)
            .await
            .unwrap();

        let service = make_service(&db, &dir);
        let count = service.run().await.unwrap();
        assert_eq!(count, 1);

        let session = store.get("s-1").await.unwrap().unwrap();
        assert_eq!(session.status, SessionStatus::Interrupted);
        assert!(session.ended_at.is_some());
    }

    #[tokio::test]
    async fn recovers_queued_sessions() {
        let (db, dir) = test_db().await;
        let store = SessionStore::new(db.clone());

        // Queued sessions are created by default.
        store.create("s-1", &sample_input()).await.unwrap();
        store.create("s-2", &sample_input()).await.unwrap();

        let service = make_service(&db, &dir);
        let count = service.run().await.unwrap();
        assert_eq!(count, 2);

        let s1 = store.get("s-1").await.unwrap().unwrap();
        let s2 = store.get("s-2").await.unwrap().unwrap();
        assert_eq!(s1.status, SessionStatus::Interrupted);
        assert_eq!(s2.status, SessionStatus::Interrupted);
    }

    #[tokio::test]
    async fn does_not_touch_completed_or_failed_sessions() {
        let (db, dir) = test_db().await;
        let store = SessionStore::new(db.clone());

        store.create("s-done", &sample_input()).await.unwrap();
        store
            .update_status(
                "s-done",
                SessionStatus::Completed,
                Some(crate::db::models::ExitReason::Completed),
                Some(1_000_000),
            )
            .await
            .unwrap();

        store.create("s-fail", &sample_input()).await.unwrap();
        store
            .update_status(
                "s-fail",
                SessionStatus::Failed,
                Some(crate::db::models::ExitReason::Error),
                Some(2_000_000),
            )
            .await
            .unwrap();

        let service = make_service(&db, &dir);
        let count = service.run().await.unwrap();
        assert_eq!(count, 0);

        let s_done = store.get("s-done").await.unwrap().unwrap();
        let s_fail = store.get("s-fail").await.unwrap().unwrap();
        assert_eq!(s_done.status, SessionStatus::Completed);
        assert_eq!(s_fail.status, SessionStatus::Failed);
    }
}
