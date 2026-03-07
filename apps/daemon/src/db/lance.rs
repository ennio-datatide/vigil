//! `LanceDB` vector store wrapper with fastembed integration.
//!
//! Provides a [`LanceDb`] handle for embedding text, upserting memory
//! vectors, and performing nearest-neighbour searches.

#![allow(dead_code)] // Module is wired ahead of its consumers.

use std::path::Path;
use std::sync::Arc;

use arrow_array::{
    FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use futures::TryStreamExt;
use lancedb::connection::{ConnectBuilder, Connection};
use lancedb::database::CreateTableMode;
use lancedb::query::{ExecutableQuery, QueryBase};
use tracing::info;

use crate::error::MemoryError;

/// Embedding dimension for `AllMiniLML6V2`.
const EMBEDDING_DIM: i32 = 384;

/// Table name for memory vectors.
const MEMORIES_TABLE: &str = "memories";

/// A result from a vector similarity search.
#[derive(Clone, Debug)]
pub(crate) struct SearchResult {
    /// Memory id.
    pub id: String,
    /// Distance score (lower is more similar for L2).
    pub distance: f32,
}

/// `LanceDB` wrapper holding the connection and embedding model.
///
/// The struct is `Clone`-friendly via internal `Arc`s so it can be
/// cheaply shared across request handlers.
#[derive(Clone)]
pub(crate) struct LanceDb {
    conn: Connection,
    embedder: Arc<TextEmbedding>,
}

impl LanceDb {
    /// Connect to a `LanceDB` directory and ensure the `memories` table exists.
    ///
    /// If the table does not yet exist it is created with the expected schema.
    /// The `fastembed` model is downloaded on first use and cached locally.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::VectorStore`] if the connection or table
    /// creation fails, or [`MemoryError::Embedding`] if model loading fails.
    pub async fn connect(path: &Path) -> Result<Self, MemoryError> {
        let uri = path
            .to_str()
            .ok_or_else(|| MemoryError::VectorStore("non-UTF-8 path".into()))?;

        let conn = ConnectBuilder::new(uri)
            .execute()
            .await
            .map_err(|e| MemoryError::VectorStore(e.to_string()))?;

        let embedder = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true),
        )
        .map_err(|e| MemoryError::Embedding(e.to_string()))?;

        let db = Self {
            conn,
            embedder: Arc::new(embedder),
        };

        db.ensure_table().await?;

        info!("LanceDB connected at {uri}");
        Ok(db)
    }

    /// Generate an embedding vector for the given text.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Embedding`] on model failure.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryError> {
        let results = self
            .embedder
            .embed(vec![text], None)
            .map_err(|e| MemoryError::Embedding(e.to_string()))?;

        results
            .into_iter()
            .next()
            .ok_or_else(|| MemoryError::Embedding("empty embedding result".into()))
    }

    /// Insert or update a memory vector.
    ///
    /// Uses `LanceDB`'s `merge_insert` on the `id` column so that repeated
    /// calls with the same id replace the previous row.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::VectorStore`] on write failure.
    pub async fn upsert_memory(
        &self,
        id: &str,
        content: &str,
        project_path: &str,
        embedding: &[f32],
    ) -> Result<(), MemoryError> {
        let table = self.open_table().await?;
        let batch = Self::build_batch(id, content, project_path, embedding)?;
        let schema = batch.schema();
        let reader = Box::new(RecordBatchIterator::new(
            vec![Ok(batch)],
            schema,
        ));

        let mut builder = table.merge_insert(&["id"]);
        builder
            .when_matched_update_all(None)
            .when_not_matched_insert_all();
        builder
            .execute(reader)
            .await
            .map_err(|e| MemoryError::VectorStore(e.to_string()))?;

        Ok(())
    }

    /// Search for the nearest memory vectors.
    ///
    /// Returns up to `limit` results ordered by ascending distance.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::VectorStore`] on query failure.
    pub async fn search(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchResult>, MemoryError> {
        let table = self.open_table().await?;

        let mut stream = table
            .vector_search(embedding.to_vec())
            .map_err(|e| MemoryError::VectorStore(e.to_string()))?
            .limit(limit)
            .select(lancedb::query::Select::columns(&["id"]))
            .execute()
            .await
            .map_err(|e| MemoryError::VectorStore(e.to_string()))?;

        let mut results = Vec::new();
        while let Some(batch) = stream
            .try_next()
            .await
            .map_err(|e| MemoryError::VectorStore(e.to_string()))?
        {
            let ids = batch
                .column_by_name("id")
                .ok_or_else(|| MemoryError::VectorStore("missing id column".into()))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| MemoryError::VectorStore("id column is not a StringArray".into()))?;

            let distances = batch
                .column_by_name("_distance")
                .ok_or_else(|| MemoryError::VectorStore("missing _distance column".into()))?
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(|| {
                    MemoryError::VectorStore("_distance column is not Float32".into())
                })?;

            for i in 0..batch.num_rows() {
                results.push(SearchResult {
                    id: ids.value(i).to_owned(),
                    distance: distances.value(i),
                });
            }
        }

        Ok(results)
    }

    /// Delete a memory by its id.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::VectorStore`] on deletion failure.
    pub async fn delete(&self, id: &str) -> Result<(), MemoryError> {
        let table = self.open_table().await?;
        table
            .delete(&format!("id = '{id}'"))
            .await
            .map_err(|e| MemoryError::VectorStore(e.to_string()))?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Ensure the `memories` table exists, creating it if needed.
    async fn ensure_table(&self) -> Result<(), MemoryError> {
        let tables = self
            .conn
            .table_names()
            .execute()
            .await
            .map_err(|e| MemoryError::VectorStore(e.to_string()))?;

        if !tables.iter().any(|t| t == MEMORIES_TABLE) {
            let schema = Self::table_schema();
            self.conn
                .create_empty_table(MEMORIES_TABLE, schema)
                .mode(CreateTableMode::Create)
                .execute()
                .await
                .map_err(|e| MemoryError::VectorStore(e.to_string()))?;
        }

        Ok(())
    }

    /// Open the `memories` table.
    async fn open_table(&self) -> Result<lancedb::table::Table, MemoryError> {
        self.conn
            .open_table(MEMORIES_TABLE)
            .execute()
            .await
            .map_err(|e| MemoryError::VectorStore(e.to_string()))
    }

    /// Build the Arrow schema for the `memories` table.
    fn table_schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("project_path", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    EMBEDDING_DIM,
                ),
                false,
            ),
        ]))
    }

    /// Build a single-row `RecordBatch` for one memory.
    fn build_batch(
        id: &str,
        content: &str,
        project_path: &str,
        embedding: &[f32],
    ) -> Result<RecordBatch, MemoryError> {
        let id_arr = Arc::new(StringArray::from(vec![id]));
        let content_arr = Arc::new(StringArray::from(vec![content]));
        let project_path_arr = Arc::new(StringArray::from(vec![project_path]));

        let values = Arc::new(Float32Array::from(embedding.to_vec()));
        let field = Arc::new(Field::new("item", DataType::Float32, true));
        let vector_arr = Arc::new(FixedSizeListArray::new(field, EMBEDDING_DIM, values, None));

        RecordBatch::try_new(Self::table_schema(), vec![
            id_arr,
            content_arr,
            project_path_arr,
            vector_arr,
        ])
        .map_err(|e| MemoryError::VectorStore(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared helper — creates a `LanceDb` rooted in a temp directory.
    async fn setup() -> (tempfile::TempDir, LanceDb) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = LanceDb::connect(tmp.path()).await.expect("connect");
        (tmp, db)
    }

    #[tokio::test]
    async fn connect_creates_table() {
        let (_tmp, db) = setup().await;
        let tables = db.conn.table_names().execute().await.unwrap();
        assert!(tables.contains(&MEMORIES_TABLE.to_owned()));
    }

    #[tokio::test]
    async fn embed_produces_384_dim_vector() {
        let (_tmp, db) = setup().await;
        let vec = db.embed("hello world").unwrap();
        assert_eq!(vec.len(), 384);
    }

    #[tokio::test]
    async fn upsert_and_search_round_trip() {
        let (_tmp, db) = setup().await;

        let text = "Rust is a systems programming language";
        let embedding = db.embed(text).unwrap();

        db.upsert_memory("mem-1", text, "/project", &embedding)
            .await
            .unwrap();

        let results = db.search(&embedding, 5).await.unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "mem-1");
    }

    #[tokio::test]
    async fn upsert_overwrites_existing() {
        let (_tmp, db) = setup().await;

        let emb1 = db.embed("first version").unwrap();
        db.upsert_memory("mem-1", "first version", "/p", &emb1)
            .await
            .unwrap();

        let emb2 = db.embed("second version").unwrap();
        db.upsert_memory("mem-1", "second version", "/p", &emb2)
            .await
            .unwrap();

        let table = db.open_table().await.unwrap();
        let count = table.count_rows(None).await.unwrap();
        assert_eq!(count, 1, "upsert should replace, not duplicate");
    }

    #[tokio::test]
    async fn delete_removes_memory() {
        let (_tmp, db) = setup().await;

        let embedding = db.embed("to be deleted").unwrap();
        db.upsert_memory("del-1", "to be deleted", "/p", &embedding)
            .await
            .unwrap();

        db.delete("del-1").await.unwrap();

        let table = db.open_table().await.unwrap();
        let count = table.count_rows(None).await.unwrap();
        assert_eq!(count, 0);
    }
}
