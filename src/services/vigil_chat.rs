//! Vigil chat history persistence.
//!
//! [`VigilChatStore`] provides CRUD operations over the `vigil_messages` table,
//! persisting the conversation between the user and the Vigil overseer.

#![allow(dead_code)] // Module is wired ahead of its route consumers.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Result};

// ---------------------------------------------------------------------------
// Domain model
// ---------------------------------------------------------------------------

/// A single message in the Vigil chat history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VigilMessage {
    pub(crate) id: i64,
    pub(crate) role: String,
    pub(crate) content: String,
    pub(crate) embedded_cards: Option<String>,
    pub(crate) created_at: i64,
}

// ---------------------------------------------------------------------------
// VigilChatStore
// ---------------------------------------------------------------------------

/// Manages Vigil chat messages in the database.
#[derive(Clone)]
pub(crate) struct VigilChatStore {
    db: Arc<SqliteDb>,
}

impl VigilChatStore {
    /// Create a new store backed by the given database.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>) -> Self {
        Self { db }
    }

    /// Save a new message and return it with its assigned ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the insert or subsequent read fails.
    pub(crate) async fn save_message(
        &self,
        role: &str,
        content: &str,
        embedded_cards: Option<&str>,
    ) -> Result<VigilMessage> {
        let now = chrono::Utc::now().timestamp_millis();

        let result = sqlx::query(
            "INSERT INTO vigil_messages (role, content, embedded_cards, created_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(role)
        .bind(content)
        .bind(embedded_cards)
        .bind(now)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        let id = result.last_insert_rowid();

        Ok(VigilMessage {
            id,
            role: role.to_owned(),
            content: content.to_owned(),
            embedded_cards: embedded_cards.map(String::from),
            created_at: now,
        })
    }

    /// List messages ordered by ID ascending, with pagination.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn list_messages(&self, limit: i64, offset: i64) -> Result<Vec<VigilMessage>> {
        let rows = sqlx::query(
            "SELECT id, role, content, embedded_cards, created_at \
             FROM vigil_messages ORDER BY id ASC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        let messages = rows
            .iter()
            .map(|row| VigilMessage {
                id: row.get("id"),
                role: row.get("role"),
                content: row.get("content"),
                embedded_cards: row.get("embedded_cards"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(messages)
    }

    /// Delete all messages from the chat history.
    ///
    /// # Errors
    ///
    /// Returns an error if the delete fails.
    pub(crate) async fn clear(&self) -> Result<()> {
        sqlx::query("DELETE FROM vigil_messages")
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
    async fn save_and_list_messages() {
        let (db, _dir) = test_db().await;
        let store = VigilChatStore::new(db);

        let m1 = store
            .save_message("user", "Hello Vigil", None)
            .await
            .unwrap();
        assert_eq!(m1.role, "user");
        assert_eq!(m1.content, "Hello Vigil");
        assert!(m1.embedded_cards.is_none());
        assert!(m1.id > 0);

        let cards = r#"[{"type":"session","id":"s1"}]"#;
        let m2 = store
            .save_message("assistant", "Here is the status", Some(cards))
            .await
            .unwrap();
        assert_eq!(m2.role, "assistant");
        assert_eq!(m2.embedded_cards.as_deref(), Some(cards));

        let messages = store.list_messages(100, 0).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id, m1.id);
        assert_eq!(messages[1].id, m2.id);
        assert_eq!(messages[0].content, "Hello Vigil");
        assert_eq!(messages[1].content, "Here is the status");
    }

    #[tokio::test]
    async fn list_with_limit_and_offset() {
        let (db, _dir) = test_db().await;
        let store = VigilChatStore::new(db);

        for i in 0..5 {
            store
                .save_message("user", &format!("msg {i}"), None)
                .await
                .unwrap();
        }

        let page = store.list_messages(2, 1).await.unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].content, "msg 1");
        assert_eq!(page[1].content, "msg 2");
    }

    #[tokio::test]
    async fn clear_removes_all() {
        let (db, _dir) = test_db().await;
        let store = VigilChatStore::new(db);

        store.save_message("user", "hi", None).await.unwrap();
        store
            .save_message("assistant", "hello", None)
            .await
            .unwrap();

        let before = store.list_messages(100, 0).await.unwrap();
        assert_eq!(before.len(), 2);

        store.clear().await.unwrap();

        let after = store.list_messages(100, 0).await.unwrap();
        assert!(after.is_empty());
    }
}
