//! Notification persistence and query service.
//!
//! [`NotificationStore`] provides CRUD operations over the `notifications` table,
//! mapping between `SQLite` columns and domain model types.

use std::sync::Arc;

use sqlx::Row;

use crate::db::models::{Notification, NotificationType};
use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Result};

/// Manages notification records in the database.
#[derive(Clone, Debug)]
pub(crate) struct NotificationStore {
    db: Arc<SqliteDb>,
}

impl NotificationStore {
    /// Create a new store backed by the given database.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>) -> Self {
        Self { db }
    }

    /// List notifications, optionally filtering to unread only.
    ///
    /// Results are ordered by `id DESC` (most recent first).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn list(&self, unread_only: bool) -> Result<Vec<Notification>> {
        let rows = if unread_only {
            sqlx::query(
                "SELECT id, session_id, type, message, sent_at, read_at \
                 FROM notifications WHERE read_at IS NULL ORDER BY id DESC",
            )
            .fetch_all(self.db.pool())
            .await
            .map_err(DbError::from)?
        } else {
            sqlx::query(
                "SELECT id, session_id, type, message, sent_at, read_at \
                 FROM notifications ORDER BY id DESC",
            )
            .fetch_all(self.db.pool())
            .await
            .map_err(DbError::from)?
        };

        Ok(rows.iter().map(row_to_notification).collect())
    }

    /// Insert a new notification and return the created record.
    ///
    /// # Errors
    ///
    /// Returns an error if the insert fails.
    pub(crate) async fn create(
        &self,
        session_id: &str,
        notification_type: NotificationType,
        message: &str,
    ) -> Result<Notification> {
        let type_text = notification_type_to_str(&notification_type);
        let now_ms = unix_ms();

        let result = sqlx::query(
            "INSERT INTO notifications (session_id, type, message, sent_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(type_text)
        .bind(message)
        .bind(now_ms)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        let id = result.last_insert_rowid();

        let row = sqlx::query(
            "SELECT id, session_id, type, message, sent_at, read_at \
             FROM notifications WHERE id = ?",
        )
        .bind(id)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(row_to_notification(&row))
    }

    /// Mark a single notification as read. Returns `None` if not found.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn mark_read(&self, id: i64) -> Result<Option<Notification>> {
        let now_ms = unix_ms();

        let result = sqlx::query("UPDATE notifications SET read_at = ? WHERE id = ?")
            .bind(now_ms)
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        let row = sqlx::query(
            "SELECT id, session_id, type, message, sent_at, read_at \
             FROM notifications WHERE id = ?",
        )
        .bind(id)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(Some(row_to_notification(&row)))
    }

    /// Mark all unread notifications as read. Returns the count of updated rows.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn mark_all_read(&self) -> Result<u64> {
        let now_ms = unix_ms();

        let result =
            sqlx::query("UPDATE notifications SET read_at = ? WHERE read_at IS NULL")
                .bind(now_ms)
                .execute(self.db.pool())
                .await
                .map_err(DbError::from)?;

        Ok(result.rows_affected())
    }
}

// ---------------------------------------------------------------------------
// Row mapping helpers
// ---------------------------------------------------------------------------

/// Map a raw `sqlx::sqlite::SqliteRow` to a [`Notification`] domain model.
fn row_to_notification(row: &sqlx::sqlite::SqliteRow) -> Notification {
    let notification_type = parse_notification_type(row.get::<String, _>("type").as_str());

    Notification {
        id: row.get("id"),
        session_id: row.get("session_id"),
        notification_type,
        message: row.get("message"),
        sent_at: row.get("sent_at"),
        read_at: row.get("read_at"),
    }
}

// ---------------------------------------------------------------------------
// Enum <-> string conversion helpers
// ---------------------------------------------------------------------------

fn notification_type_to_str(t: &NotificationType) -> &'static str {
    match t {
        NotificationType::NeedsInput => "needs_input",
        NotificationType::Error => "error",
        NotificationType::AuthRequired => "auth_required",
        NotificationType::ChainComplete => "chain_complete",
        NotificationType::SessionDone => "session_done",
    }
}

fn parse_notification_type(s: &str) -> NotificationType {
    match s {
        "needs_input" => NotificationType::NeedsInput,
        "auth_required" => NotificationType::AuthRequired,
        "chain_complete" => NotificationType::ChainComplete,
        "session_done" => NotificationType::SessionDone,
        _ => NotificationType::Error,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Current time as Unix milliseconds.
fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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
    async fn create_and_list_notifications() {
        let (db, _dir) = test_db().await;
        let store = NotificationStore::new(db);

        // Initially empty.
        let notifications = store.list(false).await.unwrap();
        assert!(notifications.is_empty());

        // Create two notifications.
        let n1 = store
            .create("session-1", NotificationType::SessionDone, "Session completed")
            .await
            .unwrap();
        assert_eq!(n1.session_id, "session-1");
        assert_eq!(n1.notification_type, NotificationType::SessionDone);
        assert_eq!(n1.message, "Session completed");
        assert!(n1.sent_at.is_some());
        assert!(n1.read_at.is_none());

        let n2 = store
            .create("session-2", NotificationType::Error, "Something went wrong")
            .await
            .unwrap();
        assert_eq!(n2.notification_type, NotificationType::Error);

        // List returns both, most recent first.
        let notifications = store.list(false).await.unwrap();
        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0].id, n2.id);
        assert_eq!(notifications[1].id, n1.id);
    }

    #[tokio::test]
    async fn filter_unread_only() {
        let (db, _dir) = test_db().await;
        let store = NotificationStore::new(db);

        let n1 = store
            .create("s1", NotificationType::SessionDone, "Done")
            .await
            .unwrap();
        store
            .create("s2", NotificationType::NeedsInput, "Input needed")
            .await
            .unwrap();

        // Mark first as read.
        store.mark_read(n1.id).await.unwrap();

        // Unread only should return 1.
        let unread = store.list(true).await.unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].session_id, "s2");

        // All should return 2.
        let all = store.list(false).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn mark_single_notification_as_read() {
        let (db, _dir) = test_db().await;
        let store = NotificationStore::new(db);

        let n = store
            .create("s1", NotificationType::SessionDone, "Done")
            .await
            .unwrap();
        assert!(n.read_at.is_none());

        let updated = store.mark_read(n.id).await.unwrap().unwrap();
        assert!(updated.read_at.is_some());
        assert_eq!(updated.id, n.id);

        // Non-existent returns None.
        let missing = store.mark_read(99999).await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn mark_all_as_read() {
        let (db, _dir) = test_db().await;
        let store = NotificationStore::new(db);

        store
            .create("s1", NotificationType::SessionDone, "Done 1")
            .await
            .unwrap();
        store
            .create("s2", NotificationType::Error, "Error 1")
            .await
            .unwrap();
        store
            .create("s3", NotificationType::NeedsInput, "Input")
            .await
            .unwrap();

        let updated = store.mark_all_read().await.unwrap();
        assert_eq!(updated, 3);

        // All should now be read.
        let unread = store.list(true).await.unwrap();
        assert!(unread.is_empty());

        // Calling again should update 0.
        let updated_again = store.mark_all_read().await.unwrap();
        assert_eq!(updated_again, 0);
    }

    #[tokio::test]
    async fn test_notification_types_roundtrip() {
        let (db, _dir) = test_db().await;
        let store = NotificationStore::new(db);

        let types = vec![
            NotificationType::NeedsInput,
            NotificationType::Error,
            NotificationType::AuthRequired,
            NotificationType::ChainComplete,
            NotificationType::SessionDone,
        ];

        for nt in types {
            let n = store.create("s1", nt.clone(), "test").await.unwrap();
            assert_eq!(n.notification_type, nt);
        }
    }
}
