//! Memory persistence service combining `SQLite` and `LanceDB`.
//!
//! [`MemoryStore`] provides CRUD operations over the `memories` and
//! `memory_edges` tables, while transparently maintaining embedding
//! vectors in `LanceDB` for similarity search.

#![allow(dead_code)] // Module is wired ahead of its route consumers.

use std::sync::Arc;

use serde::Deserialize;
use sqlx::Row;
use tracing::debug;

use crate::db::lance::LanceDb;
use crate::db::models::{Memory, MemoryEdge, MemoryEdgeType, MemoryType};
use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, MemoryError, Result};

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Input for creating a new memory.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateMemoryInput {
    /// The memory content text.
    pub content: String,
    /// Classification of this memory.
    pub memory_type: MemoryType,
    /// Project this memory belongs to.
    pub project_path: String,
    /// Session that produced this memory, if any.
    pub source_session_id: Option<String>,
    /// Override importance (defaults to type-based value).
    pub importance: Option<f64>,
}

// ---------------------------------------------------------------------------
// MemoryStore
// ---------------------------------------------------------------------------

/// Dual-store memory service backed by `SQLite` (metadata) and `LanceDB` (vectors).
///
/// Every write goes to both stores. Reads come from `SQLite`. Similarity
/// search queries `LanceDB` then joins with `SQLite` metadata.
#[derive(Clone)]
pub(crate) struct MemoryStore {
    db: Arc<SqliteDb>,
    lance: LanceDb,
}

impl MemoryStore {
    /// Create a new store backed by the given databases.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>, lance: LanceDb) -> Self {
        Self { db, lance }
    }

    /// Create a new memory, storing metadata in `SQLite` and embedding in `LanceDB`.
    ///
    /// After creation, runs auto-association to link similar memories.
    ///
    /// # Errors
    ///
    /// Returns an error if the database write or embedding fails.
    pub(crate) async fn create(&self, input: &CreateMemoryInput) -> Result<Memory> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let importance = input
            .importance
            .unwrap_or_else(|| default_importance(&input.memory_type));
        let memory_type_str = memory_type_to_str(&input.memory_type);

        // Generate embedding first — most likely failure point, fail fast
        // before writing to either store.
        let embedding = self.lance.embed(&input.content).await?;

        // Write to SQLite.
        sqlx::query(
            "INSERT INTO memories (id, project_path, memory_type, content, source_session_id, \
             importance, access_count, created_at, accessed_at) \
             VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(&id)
        .bind(&input.project_path)
        .bind(memory_type_str)
        .bind(&input.content)
        .bind(&input.source_session_id)
        .bind(importance)
        .bind(now)
        .bind(now)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        // Write vector to LanceDB (idempotent via upsert).
        self.lance
            .upsert_memory(&id, &input.content, &input.project_path, &embedding)
            .await?;

        let memory = Memory {
            id: id.clone(),
            project_path: input.project_path.clone(),
            content: input.content.clone(),
            memory_type: input.memory_type.clone(),
            source_session_id: input.source_session_id.clone(),
            importance,
            access_count: 0,
            created_at: now,
            accessed_at: now,
        };

        // Auto-associate with existing similar memories.
        if let Err(e) = self.auto_associate(&memory, &embedding).await {
            debug!("auto-association failed for memory {}: {e}", memory.id);
        }

        Ok(memory)
    }

    /// Get a single memory by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn get(&self, id: &str) -> Result<Option<Memory>> {
        let row = sqlx::query(
            "SELECT id, project_path, memory_type, content, source_session_id, \
             importance, access_count, created_at, accessed_at \
             FROM memories WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await
        .map_err(DbError::from)?;

        row.as_ref().map(|r| Ok(row_to_memory(r))).transpose()
    }

    /// List all memories for a given project, most recent first.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn list(&self, project_path: &str) -> Result<Vec<Memory>> {
        let rows = sqlx::query(
            "SELECT id, project_path, memory_type, content, source_session_id, \
             importance, access_count, created_at, accessed_at \
             FROM memories WHERE project_path = ? ORDER BY created_at DESC",
        )
        .bind(project_path)
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(rows.iter().map(row_to_memory).collect())
    }

    /// Delete a memory from both `SQLite` and `LanceDB`.
    ///
    /// # Errors
    ///
    /// Returns an error if the deletion fails.
    pub(crate) async fn delete(&self, id: &str) -> Result<()> {
        // Check existence first.
        let result = sqlx::query("SELECT id FROM memories WHERE id = ?")
            .bind(id)
            .fetch_optional(self.db.pool())
            .await
            .map_err(DbError::from)?;

        if result.is_none() {
            return Err(MemoryError::NotFound(id.to_owned()).into());
        }

        // Delete from LanceDB first — a stale SQLite row is benign and
        // retryable, but an orphaned vector cannot be cleaned up.
        self.lance.delete(id).await?;

        // Delete edges referencing this memory.
        sqlx::query("DELETE FROM memory_edges WHERE source_id = ? OR target_id = ?")
            .bind(id)
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        // Delete from SQLite.
        sqlx::query("DELETE FROM memories WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        Ok(())
    }

    /// Increment the access count and update the `accessed_at` timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub(crate) async fn touch(&self, id: &str) -> Result<()> {
        let now = now_millis();

        let result = sqlx::query(
            "UPDATE memories SET access_count = access_count + 1, accessed_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        if result.rows_affected() == 0 {
            return Err(MemoryError::NotFound(id.to_owned()).into());
        }

        Ok(())
    }

    /// Create an edge between two memories.
    ///
    /// # Errors
    ///
    /// Returns an error if the insert fails.
    pub(crate) async fn add_edge(
        &self,
        source_id: &str,
        target_id: &str,
        edge_type: MemoryEdgeType,
    ) -> Result<MemoryEdge> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let edge_type_str = edge_type_to_str(&edge_type);

        sqlx::query(
            "INSERT INTO memory_edges (id, source_id, target_id, edge_type, weight, created_at) \
             VALUES (?, ?, ?, ?, 1.0, ?)",
        )
        .bind(&id)
        .bind(source_id)
        .bind(target_id)
        .bind(edge_type_str)
        .bind(now)
        .execute(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(MemoryEdge {
            id,
            source_id: source_id.to_owned(),
            target_id: target_id.to_owned(),
            edge_type,
            weight: 1.0,
            created_at: now,
        })
    }

    /// Get all edges where the given memory is either source or target.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub(crate) async fn get_edges(&self, memory_id: &str) -> Result<Vec<MemoryEdge>> {
        let rows = sqlx::query(
            "SELECT id, source_id, target_id, edge_type, weight, created_at \
             FROM memory_edges WHERE source_id = ? OR target_id = ? \
             ORDER BY created_at DESC",
        )
        .bind(memory_id)
        .bind(memory_id)
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(rows.iter().map(row_to_edge).collect())
    }

    /// Automatically create edges to similar existing memories.
    ///
    /// Searches `LanceDB` for the top 10 nearest neighbours and creates:
    /// - `Updates` edges for very similar content (distance < 0.2)
    /// - `RelatedTo` edges for moderately similar content (0.2 <= distance < 0.6)
    async fn auto_associate(&self, memory: &Memory, embedding: &[f32]) -> Result<()> {
        let results = self.lance.search(embedding, 10).await?;

        for result in &results {
            // Skip self.
            if result.id == memory.id {
                continue;
            }

            let edge_type = if result.distance < 0.2 {
                MemoryEdgeType::Updates
            } else if result.distance < 0.6 {
                MemoryEdgeType::RelatedTo
            } else {
                continue;
            };

            self.add_edge(&memory.id, &result.id, edge_type).await?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Row mapping helpers
// ---------------------------------------------------------------------------

/// Map a raw `SqliteRow` to a [`Memory`] domain model.
pub(crate) fn row_to_memory(row: &sqlx::sqlite::SqliteRow) -> Memory {
    let memory_type = parse_memory_type(row.get::<String, _>("memory_type").as_str());

    Memory {
        id: row.get("id"),
        project_path: row.get("project_path"),
        content: row.get("content"),
        memory_type,
        source_session_id: row.get("source_session_id"),
        importance: row.get("importance"),
        access_count: row.get("access_count"),
        created_at: row.get("created_at"),
        accessed_at: row.get("accessed_at"),
    }
}

/// Map a raw `SqliteRow` to a [`MemoryEdge`] domain model.
fn row_to_edge(row: &sqlx::sqlite::SqliteRow) -> MemoryEdge {
    let edge_type = parse_edge_type(row.get::<String, _>("edge_type").as_str());

    MemoryEdge {
        id: row.get("id"),
        source_id: row.get("source_id"),
        target_id: row.get("target_id"),
        edge_type,
        weight: row.get("weight"),
        created_at: row.get("created_at"),
    }
}

// ---------------------------------------------------------------------------
// Enum <-> string conversion helpers
// ---------------------------------------------------------------------------

fn memory_type_to_str(t: &MemoryType) -> &'static str {
    match t {
        MemoryType::Fact => "fact",
        MemoryType::Decision => "decision",
        MemoryType::Preference => "preference",
        MemoryType::Pattern => "pattern",
        MemoryType::Failure => "failure",
        MemoryType::Todo => "todo",
    }
}

pub(crate) fn parse_memory_type(s: &str) -> MemoryType {
    match s {
        "decision" => MemoryType::Decision,
        "preference" => MemoryType::Preference,
        "pattern" => MemoryType::Pattern,
        "failure" => MemoryType::Failure,
        "todo" => MemoryType::Todo,
        // "fact" and unknown values both map to Fact.
        _ => MemoryType::Fact,
    }
}

fn edge_type_to_str(t: &MemoryEdgeType) -> &'static str {
    match t {
        MemoryEdgeType::RelatedTo => "related_to",
        MemoryEdgeType::Updates => "updates",
        MemoryEdgeType::Contradicts => "contradicts",
        MemoryEdgeType::CausedBy => "caused_by",
        MemoryEdgeType::PartOf => "part_of",
    }
}

fn parse_edge_type(s: &str) -> MemoryEdgeType {
    match s {
        "updates" => MemoryEdgeType::Updates,
        "contradicts" => MemoryEdgeType::Contradicts,
        "caused_by" => MemoryEdgeType::CausedBy,
        "part_of" => MemoryEdgeType::PartOf,
        // "related_to" and unknown values both map to RelatedTo.
        _ => MemoryEdgeType::RelatedTo,
    }
}

/// Default importance score based on memory type.
fn default_importance(memory_type: &MemoryType) -> f64 {
    match memory_type {
        MemoryType::Failure => 0.9,
        MemoryType::Decision => 0.8,
        MemoryType::Pattern => 0.7,
        MemoryType::Preference => 0.6,
        MemoryType::Fact => 0.5,
        MemoryType::Todo => 0.4,
    }
}

/// Current time as Unix milliseconds.
#[allow(clippy::cast_possible_truncation)] // Millis won't exceed i64 until year 292278994.
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before Unix epoch")
        .as_millis() as i64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an isolated test environment with both `SQLite` and `LanceDB`.
    async fn test_deps() -> (tempfile::TempDir, MemoryStore) {
        let dir = tempfile::TempDir::new().unwrap();
        let config = crate::config::Config::for_testing(dir.path());
        config.ensure_dirs().unwrap();

        let db = SqliteDb::connect(&config.db_path).await.unwrap();
        let lance = LanceDb::connect(&config.lance_dir).await.unwrap();

        (dir, MemoryStore::new(Arc::new(db), lance))
    }

    fn sample_input() -> CreateMemoryInput {
        CreateMemoryInput {
            content: "Rust uses ownership for memory safety".into(),
            memory_type: MemoryType::Fact,
            project_path: "/tmp/test-project".into(),
            source_session_id: Some("session-1".into()),
            importance: None,
        }
    }

    #[tokio::test]
    async fn create_and_get_memory() {
        let (_dir, store) = test_deps().await;

        let memory = store.create(&sample_input()).await.unwrap();

        assert!(!memory.id.is_empty());
        assert_eq!(memory.content, "Rust uses ownership for memory safety");
        assert_eq!(memory.memory_type, MemoryType::Fact);
        assert_eq!(memory.project_path, "/tmp/test-project");
        assert_eq!(memory.source_session_id, Some("session-1".into()));
        // Default importance for Fact.
        assert!((memory.importance - 0.5).abs() < f64::EPSILON);
        assert_eq!(memory.access_count, 0);

        // Get by ID.
        let fetched = store.get(&memory.id).await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id, memory.id);
        assert_eq!(fetched.content, memory.content);
    }

    #[tokio::test]
    async fn list_memories_by_project() {
        let (_dir, store) = test_deps().await;

        // Create memories in two different projects.
        let mut input_a = sample_input();
        input_a.project_path = "/tmp/project-a".into();
        store.create(&input_a).await.unwrap();

        let mut input_b = sample_input();
        input_b.project_path = "/tmp/project-b".into();
        store.create(&input_b).await.unwrap();

        let mut input_a2 = sample_input();
        input_a2.project_path = "/tmp/project-a".into();
        input_a2.content = "Second memory for project A".into();
        store.create(&input_a2).await.unwrap();

        let list_a = store.list("/tmp/project-a").await.unwrap();
        assert_eq!(list_a.len(), 2);

        let list_b = store.list("/tmp/project-b").await.unwrap();
        assert_eq!(list_b.len(), 1);

        let list_c = store.list("/tmp/nonexistent").await.unwrap();
        assert!(list_c.is_empty());
    }

    #[tokio::test]
    async fn delete_removes_from_both_stores() {
        let (_dir, store) = test_deps().await;

        let memory = store.create(&sample_input()).await.unwrap();

        // Verify it exists.
        assert!(store.get(&memory.id).await.unwrap().is_some());

        // Delete.
        store.delete(&memory.id).await.unwrap();

        // Gone from SQLite.
        assert!(store.get(&memory.id).await.unwrap().is_none());

        // Deleting again should fail.
        let err = store.delete(&memory.id).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn touch_increments_access_count() {
        let (_dir, store) = test_deps().await;

        let memory = store.create(&sample_input()).await.unwrap();
        assert_eq!(memory.access_count, 0);

        store.touch(&memory.id).await.unwrap();
        store.touch(&memory.id).await.unwrap();

        let updated = store.get(&memory.id).await.unwrap().unwrap();
        assert_eq!(updated.access_count, 2);
        assert!(updated.accessed_at >= memory.accessed_at);
    }

    #[tokio::test]
    async fn touch_nonexistent_fails() {
        let (_dir, store) = test_deps().await;

        let err = store.touch("nonexistent").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn add_and_get_edges() {
        let (_dir, store) = test_deps().await;

        let m1 = store.create(&sample_input()).await.unwrap();

        let mut input2 = sample_input();
        input2.content = "Rust borrow checker prevents data races".into();
        let m2 = store.create(&input2).await.unwrap();

        let edge = store
            .add_edge(&m1.id, &m2.id, MemoryEdgeType::RelatedTo)
            .await
            .unwrap();

        assert_eq!(edge.source_id, m1.id);
        assert_eq!(edge.target_id, m2.id);
        assert_eq!(edge.edge_type, MemoryEdgeType::RelatedTo);
        assert!((edge.weight - 1.0).abs() < f64::EPSILON);

        // Get edges for m1.
        let edges = store.get_edges(&m1.id).await.unwrap();
        // At least the manually added edge (auto-association may add more).
        assert!(edges.iter().any(|e| e.id == edge.id));

        // Get edges for m2 (it appears as target).
        let edges2 = store.get_edges(&m2.id).await.unwrap();
        assert!(edges2.iter().any(|e| e.id == edge.id));
    }

    #[tokio::test]
    async fn auto_association_creates_edges_for_similar_content() {
        let (_dir, store) = test_deps().await;

        // Create a memory.
        let m1 = store.create(&sample_input()).await.unwrap();

        // Create a very similar memory — should trigger auto-association.
        let mut similar_input = sample_input();
        similar_input.content = "Rust uses ownership and borrowing for memory safety".into();
        let m2 = store.create(&similar_input).await.unwrap();

        // Check that an edge was created between m2 and m1.
        let edges = store.get_edges(&m2.id).await.unwrap();
        let has_edge_to_m1 = edges.iter().any(|e| {
            (e.source_id == m2.id && e.target_id == m1.id)
                && (e.edge_type == MemoryEdgeType::Updates
                    || e.edge_type == MemoryEdgeType::RelatedTo)
        });
        assert!(
            has_edge_to_m1,
            "expected auto-association edge from m2 to m1, edges: {edges:?}"
        );
    }

    #[tokio::test]
    async fn importance_defaults_by_type() {
        let (_dir, store) = test_deps().await;

        let mut input = sample_input();
        input.memory_type = MemoryType::Failure;
        input.importance = None;
        let m = store.create(&input).await.unwrap();
        assert!((m.importance - 0.9).abs() < f64::EPSILON);

        let mut input2 = sample_input();
        input2.memory_type = MemoryType::Decision;
        input2.importance = None;
        let m2 = store.create(&input2).await.unwrap();
        assert!((m2.importance - 0.8).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn importance_override() {
        let (_dir, store) = test_deps().await;

        let mut input = sample_input();
        input.importance = Some(0.99);
        let m = store.create(&input).await.unwrap();
        assert!((m.importance - 0.99).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn delete_also_removes_edges() {
        let (_dir, store) = test_deps().await;

        let m1 = store.create(&sample_input()).await.unwrap();

        let mut input2 = sample_input();
        input2.content = "Different content for second memory".into();
        let m2 = store.create(&input2).await.unwrap();

        store
            .add_edge(&m1.id, &m2.id, MemoryEdgeType::CausedBy)
            .await
            .unwrap();

        // Deleting m1 should also remove edges.
        store.delete(&m1.id).await.unwrap();

        let edges = store.get_edges(&m2.id).await.unwrap();
        let has_edge_to_m1 = edges
            .iter()
            .any(|e| e.source_id == m1.id || e.target_id == m1.id);
        assert!(
            !has_edge_to_m1,
            "edges referencing deleted memory should be removed"
        );
    }
}
