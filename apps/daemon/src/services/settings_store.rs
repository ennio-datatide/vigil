//! Key-value settings persistence.
//!
//! [`SettingsStore`] provides generic get/set access to the `settings` table,
//! used primarily for Telegram notification configuration.

use std::sync::Arc;

use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Result};

/// Generic key-value store backed by the `settings` table.
#[derive(Clone, Debug)]
pub(crate) struct SettingsStore {
    db: Arc<SqliteDb>,
}

impl SettingsStore {
    /// Create a new store backed by the given database.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>) -> Self {
        Self { db }
    }

    /// Retrieve a value by key.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn get(&self, key: &str) -> Result<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM settings WHERE key = ?")
                .bind(key)
                .fetch_optional(self.db.pool())
                .await
                .map_err(DbError::from)?;

        Ok(row.map(|(v,)| v))
    }

    /// Insert or update a value by key.
    ///
    /// # Errors
    ///
    /// Returns an error if the upsert fails.
    pub(crate) async fn set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(())
    }
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
    async fn get_missing_key_returns_none() {
        let (db, _dir) = test_db().await;
        let store = SettingsStore::new(db);

        let result = store.get("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn set_and_get_value() {
        let (db, _dir) = test_db().await;
        let store = SettingsStore::new(db);

        store.set("telegram", r#"{"enabled":true}"#).await.unwrap();

        let value = store.get("telegram").await.unwrap();
        assert_eq!(value, Some(r#"{"enabled":true}"#.to_string()));
    }

    #[tokio::test]
    async fn set_overwrites_existing_value() {
        let (db, _dir) = test_db().await;
        let store = SettingsStore::new(db);

        store.set("key", "v1").await.unwrap();
        store.set("key", "v2").await.unwrap();

        let value = store.get("key").await.unwrap();
        assert_eq!(value, Some("v2".to_string()));
    }
}
