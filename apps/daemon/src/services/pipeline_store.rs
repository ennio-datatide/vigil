//! Pipeline persistence and query service.
//!
//! [`PipelineStore`] provides CRUD operations over the `pipelines` table,
//! mapping between `SQLite` columns and the [`Pipeline`] domain model.
//! The `steps` and `edges` columns are stored as JSON text.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::Row;
use uuid::Uuid;

use crate::db::models::{Pipeline, PipelineEdge, PipelineStep};
use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Error, Result};

/// Input for creating a new pipeline.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreatePipelineInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<PipelineStep>,
    pub edges: Vec<PipelineEdge>,
    #[serde(default)]
    pub is_default: bool,
}

/// Input for updating an existing pipeline.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdatePipelineInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub steps: Option<Vec<PipelineStep>>,
    pub edges: Option<Vec<PipelineEdge>>,
    pub is_default: Option<bool>,
}

/// Manages pipeline records in the database.
#[derive(Clone, Debug)]
pub(crate) struct PipelineStore {
    db: Arc<SqliteDb>,
}

impl PipelineStore {
    /// Create a new store backed by the given database.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>) -> Self {
        Self { db }
    }

    /// List all pipelines, ordered by creation time descending.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn list(&self) -> Result<Vec<Pipeline>> {
        let rows = sqlx::query(
            "SELECT id, name, description, steps, edges, is_default, created_at, updated_at \
             FROM pipelines ORDER BY created_at DESC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        rows.iter().map(row_to_pipeline).collect()
    }

    /// Get a pipeline by its id.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn get(&self, id: &str) -> Result<Option<Pipeline>> {
        let maybe_row = sqlx::query(
            "SELECT id, name, description, steps, edges, is_default, created_at, updated_at \
             FROM pipelines WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await
        .map_err(DbError::from)?;

        match maybe_row {
            Some(ref row) => Ok(Some(row_to_pipeline(row)?)),
            None => Ok(None),
        }
    }

    /// Create a new pipeline and return the created record.
    ///
    /// # Errors
    ///
    /// Returns an error if the insert fails or JSON serialization fails.
    pub(crate) async fn create(&self, input: CreatePipelineInput) -> Result<Pipeline> {
        let id = Uuid::new_v4().to_string();
        let now_ms = unix_ms();
        let steps_json =
            serde_json::to_string(&input.steps).map_err(DbError::Serialization)?;
        let edges_json =
            serde_json::to_string(&input.edges).map_err(DbError::Serialization)?;
        let is_default_int: i32 = i32::from(input.is_default);

        sqlx::query(
            "INSERT INTO pipelines (id, name, description, steps, edges, is_default, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.description)
        .bind(&steps_json)
        .bind(&edges_json)
        .bind(is_default_int)
        .bind(now_ms)
        .bind(now_ms)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        // Return the freshly inserted row.
        self.get(&id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline {id} not found after insert")))
    }

    /// Update an existing pipeline by id. Only provided fields are updated.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] if the pipeline does not exist.
    /// Returns an error if the query or serialization fails.
    pub(crate) async fn update(&self, id: &str, input: UpdatePipelineInput) -> Result<Pipeline> {
        let existing = self
            .get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline {id} not found")))?;

        let name = input.name.unwrap_or(existing.name);
        let description = input.description.unwrap_or(existing.description);
        let steps = input.steps.unwrap_or(existing.steps);
        let edges = input.edges.unwrap_or(existing.edges);
        let is_default = input.is_default.unwrap_or(existing.is_default);

        let steps_json =
            serde_json::to_string(&steps).map_err(DbError::Serialization)?;
        let edges_json =
            serde_json::to_string(&edges).map_err(DbError::Serialization)?;
        let is_default_int: i32 = i32::from(is_default);
        let now_ms = unix_ms();

        sqlx::query(
            "UPDATE pipelines SET name = ?, description = ?, steps = ?, edges = ?, \
             is_default = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&name)
        .bind(&description)
        .bind(&steps_json)
        .bind(&edges_json)
        .bind(is_default_int)
        .bind(now_ms)
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        self.get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("pipeline {id} not found after update")))
    }

    /// Delete a pipeline by id.
    ///
    /// Returns [`Error::BadRequest`] if this is the only default pipeline.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or the deletion is not allowed.
    pub(crate) async fn delete(&self, id: &str) -> Result<()> {
        // Check if this pipeline is a default pipeline.
        let pipeline = self.get(id).await?;

        if let Some(ref p) = pipeline
            && p.is_default
        {
            // Count how many default pipelines exist.
            let row = sqlx::query(
                "SELECT COUNT(*) as cnt FROM pipelines WHERE is_default = 1",
            )
            .fetch_one(self.db.pool())
            .await
            .map_err(DbError::from)?;

            let count: i32 = row.get("cnt");
            if count <= 1 {
                return Err(Error::BadRequest(
                    "Cannot delete the only default pipeline".to_string(),
                ));
            }
        }

        sqlx::query("DELETE FROM pipelines WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Row mapping helpers
// ---------------------------------------------------------------------------

/// Map a raw `sqlx::sqlite::SqliteRow` to a [`Pipeline`] domain model.
fn row_to_pipeline(row: &sqlx::sqlite::SqliteRow) -> Result<Pipeline> {
    let steps_json: String = row.get("steps");
    let edges_json: String = row.get("edges");
    let is_default_int: i32 = row.get("is_default");

    let steps: Vec<PipelineStep> =
        serde_json::from_str(&steps_json).map_err(DbError::Serialization)?;
    let edges: Vec<PipelineEdge> =
        serde_json::from_str(&edges_json).map_err(DbError::Serialization)?;

    Ok(Pipeline {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        steps,
        edges,
        is_default: is_default_int != 0,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
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
    use crate::db::models::Position;
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

    fn sample_steps() -> Vec<PipelineStep> {
        vec![PipelineStep {
            id: "step-1".to_string(),
            label: "Implement".to_string(),
            prompt: "Write the code".to_string(),
            skill: None,
            position: Position { x: 0.0, y: 0.0 },
        }]
    }

    fn sample_edges() -> Vec<PipelineEdge> {
        vec![]
    }

    fn sample_input(name: &str, is_default: bool) -> CreatePipelineInput {
        CreatePipelineInput {
            name: name.to_string(),
            description: String::new(),
            steps: sample_steps(),
            edges: sample_edges(),
            is_default,
        }
    }

    #[tokio::test]
    async fn create_and_list_pipelines() {
        let (db, _dir) = test_db().await;
        let store = PipelineStore::new(db);

        // Initially empty.
        let pipelines = store.list().await.unwrap();
        assert!(pipelines.is_empty());

        // Create two pipelines.
        let p1 = store.create(sample_input("Pipeline A", false)).await.unwrap();
        assert_eq!(p1.name, "Pipeline A");
        assert!(!p1.is_default);
        assert!(!p1.id.is_empty());
        assert!(p1.created_at > 0);
        assert_eq!(p1.created_at, p1.updated_at);
        assert_eq!(p1.steps.len(), 1);
        assert_eq!(p1.steps[0].label, "Implement");

        let p2 = store.create(sample_input("Pipeline B", true)).await.unwrap();
        assert_eq!(p2.name, "Pipeline B");
        assert!(p2.is_default);

        // List returns both, most recently created first.
        let pipelines = store.list().await.unwrap();
        assert_eq!(pipelines.len(), 2);
    }

    #[tokio::test]
    async fn get_pipeline_by_id() {
        let (db, _dir) = test_db().await;
        let store = PipelineStore::new(db);

        let created = store.create(sample_input("Test", false)).await.unwrap();

        let found = store.get(&created.id).await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.id, created.id);
        assert_eq!(found.name, "Test");

        // Non-existent returns None.
        let missing = store.get("nonexistent-id").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn update_pipeline() {
        let (db, _dir) = test_db().await;
        let store = PipelineStore::new(db);

        let created = store.create(sample_input("Original", false)).await.unwrap();

        // Small delay to ensure updated_at differs.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let updated = store
            .update(
                &created.id,
                UpdatePipelineInput {
                    name: Some("Renamed".to_string()),
                    description: Some("Updated description".to_string()),
                    steps: None,
                    edges: None,
                    is_default: Some(true),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.description, "Updated description");
        assert!(updated.is_default);
        assert!(updated.updated_at >= created.updated_at);
        // Steps should be unchanged.
        assert_eq!(updated.steps.len(), 1);

        // Update non-existent pipeline returns NotFound.
        let result = store
            .update(
                "nonexistent",
                UpdatePipelineInput {
                    name: Some("Nope".to_string()),
                    description: None,
                    steps: None,
                    edges: None,
                    is_default: None,
                },
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_pipeline() {
        let (db, _dir) = test_db().await;
        let store = PipelineStore::new(db);

        let p = store.create(sample_input("Deletable", false)).await.unwrap();
        assert_eq!(store.list().await.unwrap().len(), 1);

        store.delete(&p.id).await.unwrap();
        assert!(store.list().await.unwrap().is_empty());

        // Deleting a non-existent pipeline should not error.
        store.delete("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn cannot_delete_only_default_pipeline() {
        let (db, _dir) = test_db().await;
        let store = PipelineStore::new(db);

        // Create a single default pipeline.
        let default = store.create(sample_input("Default", true)).await.unwrap();

        // Trying to delete it should fail.
        let result = store.delete(&default.id).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Cannot delete the only default pipeline"),
            "unexpected error: {err_msg}"
        );

        // Still exists.
        assert_eq!(store.list().await.unwrap().len(), 1);

        // Create a second default pipeline, then deleting one should work.
        let _default2 = store.create(sample_input("Default 2", true)).await.unwrap();
        store.delete(&default.id).await.unwrap();
        assert_eq!(store.list().await.unwrap().len(), 1);
    }
}
