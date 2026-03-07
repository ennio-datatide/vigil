//! Memory importance decay and pruning service.
//!
//! Runs periodically to decay memory importance scores based on type and
//! access patterns, and prunes memories that fall below a minimum threshold.
//! `Failure` memories are exempt from decay and never pruned.

use std::sync::Arc;
use std::time::Duration;

use tracing::{error, info};

use crate::db::lance::LanceDb;
use crate::db::models::{Memory, MemoryType};
use crate::db::sqlite::SqliteDb;
use crate::deps::AppDeps;
use crate::error::{DbError, Result};

use super::memory_store::row_to_memory;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Memories with importance below this threshold are pruned.
const PRUNE_THRESHOLD: f64 = 0.05;

/// Decay rate per hour for `Decision` and `Pattern` memories (slow).
const DECAY_SLOW: f64 = 0.001;

/// Decay rate per hour for `Preference` and `Fact` memories (medium).
const DECAY_MEDIUM: f64 = 0.005;

/// Decay rate per hour for `Todo` memories (fast).
const DECAY_FAST: f64 = 0.01;

/// Milliseconds per hour.
const MS_PER_HOUR: f64 = 3_600_000.0;

// ---------------------------------------------------------------------------
// DecayReport
// ---------------------------------------------------------------------------

/// Summary of a single decay cycle.
#[derive(Debug)]
#[allow(dead_code, clippy::struct_field_names)]
pub(crate) struct DecayReport {
    /// Total number of memories examined.
    pub processed: usize,
    /// Number of memories whose importance was updated.
    pub decayed: usize,
    /// Number of memories pruned (deleted from both stores).
    pub pruned: usize,
}

// ---------------------------------------------------------------------------
// MemoryDecayService
// ---------------------------------------------------------------------------

/// Periodic background service that decays memory importance and prunes
/// low-value memories from both `SQLite` and `LanceDB`.
pub(crate) struct MemoryDecayService {
    db: Arc<SqliteDb>,
    lance: LanceDb,
}

impl MemoryDecayService {
    /// Create a new decay service from the shared application dependencies.
    #[must_use]
    pub(crate) fn new(deps: &AppDeps) -> Self {
        Self {
            db: deps.db.clone(),
            lance: deps.lance.clone(),
        }
    }

    /// Create a new decay service from raw components (useful for testing).
    #[cfg(test)]
    fn from_parts(db: Arc<SqliteDb>, lance: LanceDb) -> Self {
        Self { db, lance }
    }

    /// Start the periodic decay loop as a background task.
    ///
    /// Runs every hour. Returns a [`JoinHandle`](tokio::task::JoinHandle)
    /// that the caller should store and abort on shutdown.
    pub(crate) fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3600));
            loop {
                interval.tick().await;
                match self.run_cycle().await {
                    Ok(report) => info!(?report, "memory decay cycle complete"),
                    Err(e) => error!(error = %e, "memory decay cycle failed"),
                }
            }
        })
    }

    /// Run one decay cycle across all memories.
    ///
    /// For each non-`Failure` memory, computes a new importance based on
    /// elapsed time, type-specific decay rate, and access-count boost.
    /// Memories falling below [`PRUNE_THRESHOLD`] are deleted from both stores.
    ///
    /// # Errors
    ///
    /// Returns an error if database queries fail.
    pub(crate) async fn run_cycle(&self) -> Result<DecayReport> {
        self.run_cycle_at(now_millis()).await
    }

    /// Run a decay cycle using the given timestamp as "now".
    ///
    /// Extracted for testability so tests can control the clock.
    async fn run_cycle_at(&self, now: i64) -> Result<DecayReport> {
        let memories = self.list_all().await?;

        let mut decayed = 0;
        let mut pruned = 0;

        for memory in &memories {
            // Failure memories never decay.
            if memory.memory_type == MemoryType::Failure {
                continue;
            }

            #[allow(clippy::cast_precision_loss)] // Millis delta and access counts are small.
            let hours_since_access = (now - memory.accessed_at) as f64 / MS_PER_HOUR;
            let decay_rate = decay_rate_for(&memory.memory_type);
            #[allow(clippy::cast_precision_loss)]
            let boost = (1.0 + memory.access_count as f64).log2() * 0.01;
            let new_importance = (memory.importance - decay_rate * hours_since_access + boost).max(0.0);

            if new_importance < PRUNE_THRESHOLD {
                self.delete_memory(&memory.id).await?;
                pruned += 1;
            } else if (new_importance - memory.importance).abs() > f64::EPSILON {
                self.update_importance(&memory.id, new_importance).await?;
                decayed += 1;
            }
        }

        Ok(DecayReport {
            processed: memories.len(),
            decayed,
            pruned,
        })
    }

    /// Fetch all memories from `SQLite`.
    async fn list_all(&self) -> Result<Vec<Memory>> {
        let rows = sqlx::query(
            "SELECT id, project_path, memory_type, content, source_session_id, \
             importance, access_count, created_at, accessed_at \
             FROM memories",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(rows.iter().map(row_to_memory).collect())
    }

    /// Update the importance score for a single memory.
    async fn update_importance(&self, id: &str, importance: f64) -> Result<()> {
        sqlx::query("UPDATE memories SET importance = ? WHERE id = ?")
            .bind(importance)
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        Ok(())
    }

    /// Delete a memory from both stores, including its edges.
    async fn delete_memory(&self, id: &str) -> Result<()> {
        // Delete edges first.
        sqlx::query("DELETE FROM memory_edges WHERE source_id = ? OR target_id = ?")
            .bind(id)
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        // Delete from LanceDB (ignore not-found — may already be absent).
        let _ = self.lance.delete(id).await;

        // Delete from SQLite.
        sqlx::query("DELETE FROM memories WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(DbError::from)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Decay rate per hour for a given memory type.
fn decay_rate_for(memory_type: &MemoryType) -> f64 {
    match memory_type {
        MemoryType::Decision | MemoryType::Pattern => DECAY_SLOW,
        MemoryType::Preference | MemoryType::Fact => DECAY_MEDIUM,
        MemoryType::Todo => DECAY_FAST,
        MemoryType::Failure => 0.0, // Should never be called for Failure.
    }
}

/// Current time as Unix milliseconds.
#[allow(clippy::cast_possible_truncation)]
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
    use crate::config::Config;

    /// Create an isolated test environment.
    async fn test_deps() -> (tempfile::TempDir, MemoryDecayService) {
        let dir = tempfile::TempDir::new().unwrap();
        let config = Config::for_testing(dir.path());
        config.ensure_dirs().unwrap();

        let db = SqliteDb::connect(&config.db_path).await.unwrap();
        let lance = LanceDb::connect(&config.lance_dir).await.unwrap();
        let db = Arc::new(db);

        (dir, MemoryDecayService::from_parts(db, lance))
    }

    /// Insert a memory directly via SQL with controlled field values.
    async fn insert_memory(
        service: &MemoryDecayService,
        id: &str,
        memory_type: &str,
        importance: f64,
        access_count: i64,
        accessed_at: i64,
    ) {
        let now = now_millis();
        sqlx::query(
            "INSERT INTO memories (id, project_path, memory_type, content, \
             source_session_id, importance, access_count, created_at, accessed_at) \
             VALUES (?, '/test', ?, 'test content', NULL, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(memory_type)
        .bind(importance)
        .bind(access_count)
        .bind(now)
        .bind(accessed_at)
        .execute(service.db.pool())
        .await
        .unwrap();
    }

    /// Get a memory by ID from the test service's database.
    async fn get_memory(service: &MemoryDecayService, id: &str) -> Option<Memory> {
        let row = sqlx::query(
            "SELECT id, project_path, memory_type, content, source_session_id, \
             importance, access_count, created_at, accessed_at \
             FROM memories WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(service.db.pool())
        .await
        .unwrap();

        row.as_ref().map(row_to_memory)
    }

    /// Helper: simulate "now" as N hours after a given timestamp.
    fn hours_after(base_ms: i64, hours: f64) -> i64 {
        base_ms + (hours * MS_PER_HOUR) as i64
    }

    #[tokio::test]
    async fn failure_memories_are_never_decayed() {
        let (_dir, service) = test_deps().await;
        let now = now_millis();
        let old_access = hours_after(now, -1000.0); // 1000 hours ago

        insert_memory(&service, "fail-1", "failure", 0.9, 0, old_access).await;

        let report = service.run_cycle_at(now).await.unwrap();

        // Failure memory should be untouched.
        let m = get_memory(&service, "fail-1").await.unwrap();
        assert!(
            (m.importance - 0.9).abs() < f64::EPSILON,
            "failure importance should remain 0.9, got {}",
            m.importance
        );
        assert_eq!(report.decayed, 0);
        assert_eq!(report.pruned, 0);
    }

    #[tokio::test]
    async fn todo_memories_decay_fast() {
        let (_dir, service) = test_deps().await;
        let now = now_millis();
        let old_access = hours_after(now, -10.0); // 10 hours ago

        insert_memory(&service, "todo-1", "todo", 0.4, 0, old_access).await;

        let report = service.run_cycle_at(now).await.unwrap();

        let m = get_memory(&service, "todo-1").await.unwrap();
        // Expected: 0.4 - 0.01*10 + log2(1)*0.01 = 0.4 - 0.1 + 0 = 0.3
        assert!(
            (m.importance - 0.3).abs() < 0.001,
            "todo importance should be ~0.3, got {}",
            m.importance
        );
        assert_eq!(report.decayed, 1);
    }

    #[tokio::test]
    async fn decision_memories_decay_slowly() {
        let (_dir, service) = test_deps().await;
        let now = now_millis();
        let old_access = hours_after(now, -100.0); // 100 hours ago

        insert_memory(&service, "dec-1", "decision", 0.8, 0, old_access).await;

        let report = service.run_cycle_at(now).await.unwrap();

        let m = get_memory(&service, "dec-1").await.unwrap();
        // Expected: 0.8 - 0.001*100 + log2(1)*0.01 = 0.8 - 0.1 + 0 = 0.7
        assert!(
            (m.importance - 0.7).abs() < 0.001,
            "decision importance should be ~0.7, got {}",
            m.importance
        );
        assert_eq!(report.decayed, 1);
    }

    #[tokio::test]
    async fn high_access_count_provides_boost() {
        let (_dir, service) = test_deps().await;
        let now = now_millis();
        let old_access = hours_after(now, -10.0);

        // Two identical todos, one with many accesses.
        insert_memory(&service, "no-access", "todo", 0.4, 0, old_access).await;
        insert_memory(&service, "high-access", "todo", 0.4, 15, old_access).await;

        service.run_cycle_at(now).await.unwrap();

        let m_no = get_memory(&service, "no-access").await.unwrap();
        let m_hi = get_memory(&service, "high-access").await.unwrap();

        assert!(
            m_hi.importance > m_no.importance,
            "high-access ({}) should have higher importance than no-access ({})",
            m_hi.importance,
            m_no.importance
        );

        // Verify the boost: log2(1 + 15) * 0.01 = log2(16) * 0.01 = 4 * 0.01 = 0.04
        // high-access expected: 0.4 - 0.01*10 + 0.04 = 0.34
        assert!(
            (m_hi.importance - 0.34).abs() < 0.001,
            "high-access importance should be ~0.34, got {}",
            m_hi.importance
        );
    }

    #[tokio::test]
    async fn memories_below_threshold_are_pruned() {
        let (_dir, service) = test_deps().await;
        let now = now_millis();
        let very_old = hours_after(now, -500.0); // 500 hours ago

        // A todo accessed 500 hours ago with low importance should be pruned.
        // Expected: 0.1 - 0.01*500 + log2(1)*0.01 = 0.1 - 5.0 + 0 = max(0, -4.9) = 0
        insert_memory(&service, "prune-me", "todo", 0.1, 0, very_old).await;
        // A fact that will survive.
        insert_memory(&service, "keep-me", "fact", 0.9, 0, now).await;

        let report = service.run_cycle_at(now).await.unwrap();

        // The pruned memory should be gone.
        assert!(
            get_memory(&service, "prune-me").await.is_none(),
            "low-importance memory should be pruned"
        );
        // The surviving memory should still exist.
        assert!(
            get_memory(&service, "keep-me").await.is_some(),
            "high-importance memory should survive"
        );
        assert_eq!(report.pruned, 1);
    }

    #[tokio::test]
    async fn decay_report_counts_are_correct() {
        let (_dir, service) = test_deps().await;
        let now = now_millis();
        let old_access = hours_after(now, -10.0);
        let very_old = hours_after(now, -500.0);

        // 1 failure (skipped), 1 decayed, 1 pruned, 1 unchanged (fresh access).
        insert_memory(&service, "m-failure", "failure", 0.9, 0, old_access).await;
        insert_memory(&service, "m-decay", "fact", 0.5, 0, old_access).await;
        insert_memory(&service, "m-prune", "todo", 0.1, 0, very_old).await;
        insert_memory(&service, "m-fresh", "fact", 0.5, 0, now).await;

        let report = service.run_cycle_at(now).await.unwrap();

        assert_eq!(report.processed, 4, "should process all 4 memories");
        assert_eq!(report.decayed, 1, "one memory should be decayed");
        assert_eq!(report.pruned, 1, "one memory should be pruned");
    }

    #[tokio::test]
    async fn prune_also_removes_edges() {
        let (_dir, service) = test_deps().await;
        let now = now_millis();
        let very_old = hours_after(now, -500.0);

        insert_memory(&service, "m-will-prune", "todo", 0.1, 0, very_old).await;
        insert_memory(&service, "m-keeper", "failure", 0.9, 0, now).await;

        // Create an edge between them.
        sqlx::query(
            "INSERT INTO memory_edges (id, source_id, target_id, edge_type, weight, created_at) \
             VALUES ('e-1', 'm-will-prune', 'm-keeper', 'related_to', 1.0, ?)",
        )
        .bind(now)
        .execute(service.db.pool())
        .await
        .unwrap();

        service.run_cycle_at(now).await.unwrap();

        // Edge should also be gone.
        let edge_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memory_edges WHERE source_id = 'm-will-prune' OR target_id = 'm-will-prune'",
        )
        .fetch_one(service.db.pool())
        .await
        .unwrap();

        assert_eq!(edge_count.0, 0, "edges for pruned memory should be removed");
    }

    #[tokio::test]
    async fn empty_database_produces_zero_report() {
        let (_dir, service) = test_deps().await;

        let report = service.run_cycle().await.unwrap();

        assert_eq!(report.processed, 0);
        assert_eq!(report.decayed, 0);
        assert_eq!(report.pruned, 0);
    }
}
