//! Hybrid search service combining vector similarity and full-text search.
//!
//! [`MemorySearch`] runs both a vector nearest-neighbour query and a `BM25`
//! full-text query against `LanceDB`, then merges results using Reciprocal
//! Rank Fusion (RRF). The fused ranking surfaces documents that are
//! relevant across both retrieval signals.

#![allow(dead_code)] // Module is wired ahead of its route consumers.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use sqlx::Row;
use tracing::debug;

use crate::db::lance::{LanceDb, SearchResult};
use crate::db::models::Memory;
use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Result};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The `k` constant in the RRF formula.
///
/// `RRF(d) = sum(1 / (k + rank_i(d)))` over all result lists.
/// k = 60 is near-optimal per Cormack et al. (SIGIR 2009).
const RRF_K: f64 = 60.0;

/// How many candidates to fetch from each retrieval leg before fusion.
/// A generous over-fetch ensures we do not miss relevant documents that
/// appear in only one of the two result sets.
const CANDIDATE_MULTIPLIER: usize = 3;

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// A search result enriched with the full [`Memory`] metadata and a
/// relevance score produced by the ranking algorithm.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemorySearchResult {
    /// The full memory record from `SQLite`.
    pub memory: Memory,
    /// Fused relevance score (higher is better).
    pub score: f64,
}

// ---------------------------------------------------------------------------
// MemorySearch
// ---------------------------------------------------------------------------

/// Hybrid search service backed by `LanceDB` vectors and full-text search,
/// with `SQLite` metadata enrichment.
#[derive(Clone)]
pub(crate) struct MemorySearch {
    lance: LanceDb,
    db: Arc<SqliteDb>,
}

impl MemorySearch {
    /// Create a new search service.
    #[must_use]
    pub(crate) fn new(db: Arc<SqliteDb>, lance: LanceDb) -> Self {
        Self { lance, db }
    }

    /// Hybrid search: vector + FTS with RRF reranking.
    ///
    /// 1. Embeds `query` and runs a vector nearest-neighbour search.
    /// 2. Runs a `BM25` full-text search on the same query string.
    /// 3. Fuses both ranked lists via Reciprocal Rank Fusion.
    /// 4. Optionally filters by `project_path`.
    /// 5. Enriches each result with full metadata from `SQLite`.
    ///
    /// # Errors
    ///
    /// Returns an error if embedding, search, or `SQLite` lookup fails.
    pub(crate) async fn search(
        &self,
        query: &str,
        project_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemorySearchResult>> {
        let candidates = limit * CANDIDATE_MULTIPLIER;

        // Run both retrieval legs concurrently.
        let embedding = self.lance.embed(query).await?;
        let (vector_results, fts_results) = tokio::join!(
            self.lance.search(&embedding, candidates),
            self.lance.full_text_search(query, candidates),
        );

        let vector_results = vector_results?;
        let fts_results = fts_results.unwrap_or_else(|e| {
            // FTS may fail if no index exists yet (empty table). Degrade
            // gracefully to vector-only search.
            debug!("FTS search failed, falling back to vector-only: {e}");
            Vec::new()
        });

        // Apply project_path filter if requested.
        let vector_filtered = filter_by_project(&vector_results, project_path);
        let fts_filtered = filter_by_project(&fts_results, project_path);

        // Fuse rankings.
        let ranked = reciprocal_rank_fusion(&vector_filtered, &fts_filtered);

        // Enrich with `SQLite` metadata up to `limit`.
        self.enrich(ranked, limit).await
    }

    /// Vector-only search (no FTS component).
    ///
    /// Useful as a fallback or when the FTS index has not been built yet.
    ///
    /// # Errors
    ///
    /// Returns an error if embedding or search fails.
    pub(crate) async fn vector_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemorySearchResult>> {
        let embedding = self.lance.embed(query).await?;
        let results = self.lance.search(&embedding, limit).await?;

        // Convert vector distances to scores. L2 distance is lower-is-better,
        // so we invert to produce higher-is-better scores.
        let ranked: Vec<(String, f64)> = results
            .iter()
            .map(|r| {
                let score = 1.0 / (1.0 + f64::from(r.distance));
                (r.id.clone(), score)
            })
            .collect();

        self.enrich(ranked, limit).await
    }

    /// Look up each ranked id in `SQLite` to get the full [`Memory`] record.
    async fn enrich(
        &self,
        ranked: Vec<(String, f64)>,
        limit: usize,
    ) -> Result<Vec<MemorySearchResult>> {
        let mut results = Vec::with_capacity(limit.min(ranked.len()));

        for (id, score) in ranked.into_iter().take(limit) {
            let row = sqlx::query(
                "SELECT id, project_path, memory_type, content, source_session_id, \
                 importance, access_count, created_at, accessed_at \
                 FROM memories WHERE id = ?",
            )
            .bind(&id)
            .fetch_optional(self.db.pool())
            .await
            .map_err(DbError::from)?;

            if let Some(row) = row {
                let memory = row_to_memory(&row);
                results.push(MemorySearchResult { memory, score });
            }
        }

        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// RRF implementation
// ---------------------------------------------------------------------------

/// Compute Reciprocal Rank Fusion scores for documents appearing in one
/// or both ranked result lists.
///
/// Returns `(id, rrf_score)` pairs sorted by descending score.
#[allow(clippy::cast_precision_loss)] // Rank indices are small; precision loss is negligible.
fn reciprocal_rank_fusion(
    vector_results: &[&SearchResult],
    fts_results: &[&SearchResult],
) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();

    for (rank, result) in vector_results.iter().enumerate() {
        *scores.entry(result.id.clone()).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
    }

    for (rank, result) in fts_results.iter().enumerate() {
        *scores.entry(result.id.clone()).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
    }

    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
}

/// Filter search results by project path, returning references to matches.
fn filter_by_project<'a>(
    results: &'a [SearchResult],
    project_path: Option<&str>,
) -> Vec<&'a SearchResult> {
    match project_path {
        Some(path) => results
            .iter()
            .filter(|r| r.project_path == path)
            .collect(),
        None => results.iter().collect(),
    }
}

// ---------------------------------------------------------------------------
// Row mapping (duplicated from memory_store to avoid coupling)
// ---------------------------------------------------------------------------

/// Map a raw `SqliteRow` to a [`Memory`] domain model.
fn row_to_memory(row: &sqlx::sqlite::SqliteRow) -> Memory {
    use crate::db::models::MemoryType;

    let memory_type = match row.get::<String, _>("memory_type").as_str() {
        "decision" => MemoryType::Decision,
        "preference" => MemoryType::Preference,
        "pattern" => MemoryType::Pattern,
        "failure" => MemoryType::Failure,
        "todo" => MemoryType::Todo,
        _ => MemoryType::Fact,
    };

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::services::memory_store::{CreateMemoryInput, MemoryStore};
    use crate::db::models::MemoryType;

    /// Create an isolated test environment with SQLite, LanceDB, and both stores.
    async fn test_deps() -> (tempfile::TempDir, MemoryStore, MemorySearch) {
        let dir = tempfile::TempDir::new().unwrap();
        let config = Config::for_testing(dir.path());
        config.ensure_dirs().unwrap();

        let db = SqliteDb::connect(&config.db_path).await.unwrap();
        let db = Arc::new(db);
        let lance = LanceDb::connect(&config.lance_dir).await.unwrap();

        let store = MemoryStore::new(Arc::clone(&db), lance.clone());
        let search = MemorySearch::new(db, lance);

        (dir, store, search)
    }

    /// Insert test memories and build the FTS index.
    async fn seed_memories(store: &MemoryStore, search: &MemorySearch) -> Vec<Memory> {
        let inputs = vec![
            CreateMemoryInput {
                content: "Rust uses ownership and borrowing for memory safety".into(),
                memory_type: MemoryType::Fact,
                project_path: "/project/alpha".into(),
                source_session_id: None,
                importance: None,
            },
            CreateMemoryInput {
                content: "Python is a dynamically typed interpreted language".into(),
                memory_type: MemoryType::Fact,
                project_path: "/project/alpha".into(),
                source_session_id: None,
                importance: None,
            },
            CreateMemoryInput {
                content: "The borrow checker prevents data races at compile time".into(),
                memory_type: MemoryType::Pattern,
                project_path: "/project/beta".into(),
                source_session_id: None,
                importance: None,
            },
            CreateMemoryInput {
                content: "Always use cargo clippy before committing code".into(),
                memory_type: MemoryType::Preference,
                project_path: "/project/beta".into(),
                source_session_id: None,
                importance: None,
            },
        ];

        let mut memories = Vec::new();
        for input in &inputs {
            let m = store.create(input).await.unwrap();
            memories.push(m);
        }

        // Build FTS index after inserting data.
        search.lance.create_fts_index().await.unwrap();

        memories
    }

    #[tokio::test]
    async fn rrf_scoring_produces_correct_order() {
        // Pure unit test of the RRF function — no DB needed.
        let vec_results = vec![
            SearchResult { id: "a".into(), content: String::new(), project_path: String::new(), distance: 0.1 },
            SearchResult { id: "b".into(), content: String::new(), project_path: String::new(), distance: 0.2 },
            SearchResult { id: "c".into(), content: String::new(), project_path: String::new(), distance: 0.3 },
        ];
        let fts_results = vec![
            SearchResult { id: "b".into(), content: String::new(), project_path: String::new(), distance: 5.0 },
            SearchResult { id: "d".into(), content: String::new(), project_path: String::new(), distance: 3.0 },
            SearchResult { id: "a".into(), content: String::new(), project_path: String::new(), distance: 1.0 },
        ];

        let vec_refs: Vec<&SearchResult> = vec_results.iter().collect();
        let fts_refs: Vec<&SearchResult> = fts_results.iter().collect();
        let ranked = reciprocal_rank_fusion(&vec_refs, &fts_refs);

        // "b" appears as rank 1 in vector (score 1/62) and rank 0 in FTS (score 1/61)
        // "a" appears as rank 0 in vector (score 1/61) and rank 2 in FTS (score 1/63)
        // Both appear in both lists so they should have the highest scores.
        // "b" total: 1/62 + 1/61
        // "a" total: 1/61 + 1/63
        // "b" > "a" because 1/62 + 1/61 > 1/61 + 1/63
        assert_eq!(ranked[0].0, "b", "b should rank first (best combined rank)");
        assert_eq!(ranked[1].0, "a", "a should rank second");

        // "c" and "d" appear in only one list each.
        let remaining_ids: Vec<&str> = ranked[2..].iter().map(|(id, _)| id.as_str()).collect();
        assert!(remaining_ids.contains(&"c"));
        assert!(remaining_ids.contains(&"d"));

        // All scores should be positive.
        for (_, score) in &ranked {
            assert!(*score > 0.0);
        }
    }

    #[tokio::test]
    async fn vector_search_returns_relevant_results() {
        let (_dir, store, search) = test_deps().await;
        seed_memories(&store, &search).await;

        let results = search.vector_search("Rust memory safety", 5).await.unwrap();

        assert!(!results.is_empty(), "vector search should return results");
        // The most relevant result should mention Rust or memory safety.
        let top_content = &results[0].memory.content;
        assert!(
            top_content.contains("Rust") || top_content.contains("memory safety")
                || top_content.contains("borrow"),
            "top result should be relevant: {top_content}"
        );
        // Scores should be in descending order.
        for window in results.windows(2) {
            assert!(window[0].score >= window[1].score);
        }
    }

    #[tokio::test]
    async fn fts_search_returns_relevant_results() {
        let (_dir, store, search) = test_deps().await;
        seed_memories(&store, &search).await;

        let results = search.lance.full_text_search("ownership borrowing", 5).await.unwrap();

        assert!(!results.is_empty(), "FTS should return results");
        // At least one result should contain "ownership" or "borrowing".
        let has_match = results.iter().any(|r| {
            r.content.contains("ownership") || r.content.contains("borrowing")
        });
        assert!(has_match, "FTS should find content matching the query terms");
    }

    #[tokio::test]
    async fn hybrid_search_combines_and_reranks() {
        let (_dir, store, search) = test_deps().await;
        seed_memories(&store, &search).await;

        let results = search.search("Rust ownership borrow checker", None, 5).await.unwrap();

        assert!(!results.is_empty(), "hybrid search should return results");
        // Scores should be in descending order.
        for window in results.windows(2) {
            assert!(
                window[0].score >= window[1].score,
                "results should be sorted by score descending"
            );
        }
        // The top result should be about Rust/ownership/borrowing.
        let top = &results[0].memory.content;
        assert!(
            top.contains("Rust") || top.contains("borrow") || top.contains("ownership"),
            "top hybrid result should be relevant: {top}"
        );
    }

    #[tokio::test]
    async fn project_path_filtering_works() {
        let (_dir, store, search) = test_deps().await;
        seed_memories(&store, &search).await;

        let results = search
            .search("Rust borrow checker", Some("/project/beta"), 10)
            .await
            .unwrap();

        for result in &results {
            assert_eq!(
                result.memory.project_path, "/project/beta",
                "all results should belong to the filtered project"
            );
        }
    }

    #[tokio::test]
    async fn hybrid_search_on_empty_table() {
        let (_dir, _store, search) = test_deps().await;

        // Should not panic — gracefully returns empty.
        let results = search.search("anything", None, 5).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn create_fts_index_idempotent() {
        let (_dir, store, search) = test_deps().await;
        seed_memories(&store, &search).await;

        // Calling create_fts_index again should not fail.
        search.lance.create_fts_index().await.unwrap();
    }
}
