//! Project persistence and query service.
//!
//! [`ProjectStore`] provides CRUD operations over the `projects` table,
//! mapping between `SQLite` columns and the [`Project`] domain model.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::Row;

use crate::db::models::Project;
use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Result};

/// Manages project records in the database.
#[derive(Clone, Debug)]
pub(crate) struct ProjectStore {
    db: Arc<SqliteDb>,
}

impl ProjectStore {
    /// Create a new store backed by the given database.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>) -> Self {
        Self { db }
    }

    /// List all registered projects, most recently used first.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn list(&self) -> Result<Vec<Project>> {
        let rows = sqlx::query(
            "SELECT path, name, skills_dir, last_used_at \
             FROM projects ORDER BY last_used_at DESC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(rows.iter().map(row_to_project).collect())
    }

    /// Register (or re-register) a project and return the created record.
    ///
    /// Uses `INSERT OR REPLACE` so re-registering an existing project
    /// updates its `last_used_at` timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error if the insert fails.
    pub(crate) async fn create(&self, path: &str, name: &str) -> Result<Project> {
        let now_ms = unix_ms();

        sqlx::query(
            "INSERT OR REPLACE INTO projects (path, name, last_used_at) \
             VALUES (?, ?, ?)",
        )
        .bind(path)
        .bind(name)
        .bind(now_ms)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        // Return the freshly inserted/replaced row.
        let row = sqlx::query(
            "SELECT path, name, skills_dir, last_used_at FROM projects WHERE path = ?",
        )
        .bind(path)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(row_to_project(&row))
    }

    /// Delete a project by its path.
    ///
    /// # Errors
    ///
    /// Returns an error if the delete fails.
    pub(crate) async fn delete(&self, path: &str) -> Result<()> {
        sqlx::query("DELETE FROM projects WHERE path = ?")
            .bind(path)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Row mapping helpers
// ---------------------------------------------------------------------------

/// Map a raw `sqlx::sqlite::SqliteRow` to a [`Project`] domain model.
fn row_to_project(row: &sqlx::sqlite::SqliteRow) -> Project {
    Project {
        path: row.get("path"),
        name: row.get("name"),
        skills_dir: row.get("skills_dir"),
        last_used_at: row.get("last_used_at"),
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

    /// Create an isolated test database with migrations applied.
    async fn test_db() -> (Arc<SqliteDb>, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let db = SqliteDb::connect(&db_path)
            .await
            .expect("failed to connect to test db");
        (Arc::new(db), dir)
    }

    #[tokio::test]
    async fn create_and_list_projects() {
        let (db, _dir) = test_db().await;
        let store = ProjectStore::new(db);

        // Initially empty.
        let projects = store.list().await.unwrap();
        assert!(projects.is_empty());

        // Create two projects.
        let p1 = store.create("/tmp/project-a", "project-a").await.unwrap();
        assert_eq!(p1.path, "/tmp/project-a");
        assert_eq!(p1.name, "project-a");
        assert!(p1.last_used_at.is_some());
        assert!(p1.skills_dir.is_none());

        // Small delay so last_used_at differs for ordering.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let p2 = store.create("/tmp/project-b", "project-b").await.unwrap();
        assert_eq!(p2.path, "/tmp/project-b");

        // List returns both, most recently used first.
        let projects = store.list().await.unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].path, "/tmp/project-b");
        assert_eq!(projects[1].path, "/tmp/project-a");
    }

    #[tokio::test]
    async fn delete_project() {
        let (db, _dir) = test_db().await;
        let store = ProjectStore::new(db);

        store.create("/tmp/project-a", "project-a").await.unwrap();
        assert_eq!(store.list().await.unwrap().len(), 1);

        store.delete("/tmp/project-a").await.unwrap();
        assert!(store.list().await.unwrap().is_empty());

        // Deleting a non-existent project should not error (no rows affected is fine).
        store.delete("/tmp/nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn reregister_existing_project_updates_last_used_at() {
        let (db, _dir) = test_db().await;
        let store = ProjectStore::new(db);

        let p1 = store.create("/tmp/project-a", "project-a").await.unwrap();
        let first_used = p1.last_used_at.unwrap();

        // Small delay to ensure timestamp differs.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Re-register with a different name.
        let p2 = store
            .create("/tmp/project-a", "project-a-renamed")
            .await
            .unwrap();
        assert_eq!(p2.name, "project-a-renamed");
        assert!(p2.last_used_at.unwrap() >= first_used);

        // Should still be only one project.
        let projects = store.list().await.unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "project-a-renamed");
    }
}
