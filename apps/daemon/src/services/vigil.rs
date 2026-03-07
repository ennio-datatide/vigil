//! Vigil service — per-project overseer lifecycle management.
//!
//! Manages one Vigil agent per project. Each Vigil observes session events,
//! extracts memories from completed sessions, and maintains the project acta.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::db::kv::KvStore;
use crate::db::models::SessionStatus;
use crate::db::sqlite::SqliteDb;
use crate::events::{AppEvent, EventBus};
use crate::llm::vigil::VigilDeps;
use crate::services::memory_search::MemorySearch;
use crate::services::memory_store::MemoryStore;
use crate::services::session_store::SessionStore;
use crate::services::sub_session::SubSessionService;

/// State for a single project's Vigil instance.
struct ProjectVigil {
    /// Project path this Vigil oversees.
    project_path: String,
    /// Whether the Vigil has been initialized.
    active: bool,
}

/// Manages Vigil instances across all projects.
pub(crate) struct VigilService {
    event_bus: Arc<EventBus>,
    db: Arc<SqliteDb>,
    memory_store: MemoryStore,
    memory_search: MemorySearch,
    kv: KvStore,
    sub_session: SubSessionService,
    /// Active Vigils keyed by `project_path`.
    vigils: Arc<Mutex<HashMap<String, ProjectVigil>>>,
}

impl VigilService {
    pub(crate) fn new(
        event_bus: Arc<EventBus>,
        db: Arc<SqliteDb>,
        memory_store: MemoryStore,
        memory_search: MemorySearch,
        kv: KvStore,
        sub_session: SubSessionService,
    ) -> Self {
        Self {
            event_bus,
            db,
            memory_store,
            memory_search,
            kv,
            sub_session,
            vigils: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start the Vigil event processing loop.
    pub(crate) fn start(self) -> tokio::task::JoinHandle<()> {
        let mut rx = self.event_bus.subscribe();

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Err(e) = self.handle_event(&event).await {
                            tracing::error!(error = %e, "vigil event handling failed");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "vigil service lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    /// Handle a single event.
    async fn handle_event(&self, event: &AppEvent) -> anyhow::Result<()> {
        match event {
            // When a session is spawned, ensure its project has a Vigil.
            AppEvent::SessionSpawned { session } => {
                self.ensure_vigil(&session.project_path).await;
                Ok(())
            }
            // When a session completes, extract memories.
            AppEvent::StatusChanged {
                session_id,
                new_status,
                ..
            } if *new_status == SessionStatus::Completed => {
                self.on_session_completed(session_id).await
            }
            _ => Ok(()),
        }
    }

    /// Ensure a Vigil exists for the given project.
    async fn ensure_vigil(&self, project_path: &str) {
        let mut vigils = self.vigils.lock().await;
        if !vigils.contains_key(project_path) {
            tracing::info!(project_path, "starting vigil for project");
            vigils.insert(
                project_path.to_owned(),
                ProjectVigil {
                    project_path: project_path.to_owned(),
                    active: true,
                },
            );
        }
    }

    /// Check if a Vigil is active for the given project.
    pub(crate) async fn is_active(&self, project_path: &str) -> bool {
        let vigils = self.vigils.lock().await;
        vigils.get(project_path).is_some_and(|v| v.active)
    }

    /// List all active project paths.
    pub(crate) async fn active_projects(&self) -> Vec<String> {
        let vigils = self.vigils.lock().await;
        vigils
            .values()
            .filter(|v| v.active)
            .map(|v| v.project_path.clone())
            .collect()
    }

    /// Called when a session completes — extract key information as memories.
    ///
    /// For now, this is a placeholder. The actual memory extraction would involve:
    /// 1. Reading the session's output/conversation
    /// 2. Using the Vigil agent to identify key facts, decisions, and patterns
    /// 3. Saving them as memories
    ///
    /// Since we don't have access to conversation transcripts in the current
    /// architecture (they live in the claude CLI), we store a completion record.
    async fn on_session_completed(&self, session_id: &str) -> anyhow::Result<()> {
        let store = SessionStore::new(Arc::clone(&self.db));
        let Some(session) = store.get(session_id).await? else {
            return Ok(());
        };

        // Ensure vigil is active for this project.
        self.ensure_vigil(&session.project_path).await;

        // Create a memory recording the session completion.
        let content = format!(
            "Session completed: {} (prompt: \"{}\")",
            session_id,
            truncate(&session.prompt, 100),
        );

        let input = crate::services::memory_store::CreateMemoryInput {
            content,
            memory_type: crate::db::models::MemoryType::Fact,
            project_path: session.project_path.clone(),
            source_session_id: Some(session_id.to_owned()),
            importance: Some(0.3), // Low importance — just a record
        };

        self.memory_store.create(&input).await?;

        tracing::debug!(
            session_id,
            project = %session.project_path,
            "vigil: recorded session completion"
        );
        Ok(())
    }

    /// Get the [`VigilDeps`] for building an agent (used by API routes).
    pub(crate) fn vigil_deps(&self) -> VigilDeps {
        VigilDeps {
            memory_search: self.memory_search.clone(),
            memory_store: self.memory_store.clone(),
            db: Arc::clone(&self.db),
            kv: self.kv.clone(),
            sub_session: self.sub_session.clone(),
        }
    }
}

/// Truncate a string to a maximum length, appending "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_owned()
    } else {
        let end = s
            .char_indices()
            .nth(max_len.saturating_sub(3))
            .map_or(s.len(), |(i, _)| i);
        format!("{}...", &s[..end])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::lance::LanceDb;
    use crate::db::models::SessionStatus;
    use crate::events::AppEvent;
    use crate::services::session_store::{CreateSessionInput, SessionStore};

    /// Build an isolated test environment with all Vigil dependencies.
    async fn test_deps() -> (VigilService, Arc<SqliteDb>, Arc<EventBus>, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().unwrap();
        let config = crate::config::Config::for_testing(dir.path());
        config.ensure_dirs().unwrap();

        let db = Arc::new(
            crate::db::sqlite::SqliteDb::connect(&config.db_path)
                .await
                .unwrap(),
        );
        let lance = LanceDb::connect(&config.lance_dir).await.unwrap();
        let kv = KvStore::open(&config.kv_path).unwrap();
        let event_bus = Arc::new(EventBus::new(64));
        let memory_store = MemoryStore::new(Arc::clone(&db), lance.clone());
        let memory_search = MemorySearch::new(Arc::clone(&db), lance);
        let sub_session = SubSessionService::new(Arc::clone(&db), Arc::clone(&event_bus));

        let service = VigilService::new(
            Arc::clone(&event_bus),
            Arc::clone(&db),
            memory_store,
            memory_search,
            kv,
            sub_session,
        );
        (service, db, event_bus, dir)
    }

    #[tokio::test]
    async fn ensure_vigil_creates_entry() {
        let (service, _db, _bus, _dir) = test_deps().await;

        assert!(!service.is_active("/tmp/project-a").await);

        service.ensure_vigil("/tmp/project-a").await;

        assert!(service.is_active("/tmp/project-a").await);
    }

    #[tokio::test]
    async fn ensure_vigil_idempotent() {
        let (service, _db, _bus, _dir) = test_deps().await;

        service.ensure_vigil("/tmp/project-a").await;
        service.ensure_vigil("/tmp/project-a").await;

        let projects = service.active_projects().await;
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0], "/tmp/project-a");
    }

    #[tokio::test]
    async fn active_projects_returns_all() {
        let (service, _db, _bus, _dir) = test_deps().await;

        service.ensure_vigil("/tmp/project-a").await;
        service.ensure_vigil("/tmp/project-b").await;
        service.ensure_vigil("/tmp/project-c").await;

        let mut projects = service.active_projects().await;
        projects.sort();

        assert_eq!(projects.len(), 3);
        assert_eq!(
            projects,
            vec!["/tmp/project-a", "/tmp/project-b", "/tmp/project-c"]
        );
    }

    #[tokio::test]
    async fn session_completed_creates_memory() {
        let (service, db, _bus, _dir) = test_deps().await;

        // Create a session in the database.
        let store = SessionStore::new(Arc::clone(&db));
        let input = CreateSessionInput {
            project_path: "/tmp/test-project".into(),
            prompt: "fix the bug in auth module".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        };
        store.create("sess-1", &input).await.unwrap();

        // Mark it completed.
        store
            .update_status(
                "sess-1",
                SessionStatus::Completed,
                Some(crate::db::models::ExitReason::Completed),
                Some(1_700_000_000_000),
            )
            .await
            .unwrap();

        // Trigger the completion handler.
        service.on_session_completed("sess-1").await.unwrap();

        // Verify a memory was created.
        let memories = service
            .memory_store
            .list("/tmp/test-project")
            .await
            .unwrap();
        assert_eq!(memories.len(), 1);
        assert!(memories[0].content.contains("Session completed: sess-1"));
        assert!(memories[0].content.contains("fix the bug in auth module"));
        assert_eq!(
            memories[0].source_session_id,
            Some("sess-1".to_string())
        );
        assert!(
            (memories[0].importance - 0.3).abs() < f64::EPSILON,
            "importance should be 0.3 for completion records"
        );

        // The vigil should now be active for this project.
        assert!(service.is_active("/tmp/test-project").await);
    }

    #[tokio::test]
    async fn non_completed_status_ignored() {
        let (service, db, _bus, _dir) = test_deps().await;

        // Create a session.
        let store = SessionStore::new(Arc::clone(&db));
        let input = CreateSessionInput {
            project_path: "/tmp/test-project".into(),
            prompt: "do something".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        };
        store.create("sess-2", &input).await.unwrap();

        // Send a Failed status change event — should be ignored by handle_event.
        let event = AppEvent::StatusChanged {
            session_id: "sess-2".to_string(),
            old_status: SessionStatus::Running,
            new_status: SessionStatus::Failed,
        };

        service.handle_event(&event).await.unwrap();

        // No memories should be created.
        let memories = service
            .memory_store
            .list("/tmp/test-project")
            .await
            .unwrap();
        assert!(
            memories.is_empty(),
            "no memory should be created for non-completed status changes"
        );
    }

    #[tokio::test]
    async fn handle_session_spawned_starts_vigil() {
        let (service, _db, _bus, _dir) = test_deps().await;

        let session = crate::db::models::Session {
            id: "sess-3".to_string(),
            project_path: "/tmp/spawned-project".to_string(),
            worktree_path: None,
            tmux_session: None,
            prompt: "test prompt".to_string(),
            skills_used: None,
            status: SessionStatus::Running,
            agent_type: crate::db::models::AgentType::Claude,
            role: None,
            parent_id: None,
            spawn_type: None,
            spawn_result: None,
            retry_count: 0,
            started_at: Some(1_700_000_000_000),
            ended_at: None,
            exit_reason: None,
            git_metadata: None,
            pipeline_id: None,
            pipeline_step_index: None,
        };

        let event = AppEvent::SessionSpawned { session };
        service.handle_event(&event).await.unwrap();

        assert!(service.is_active("/tmp/spawned-project").await);
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let result = truncate("this is a very long string that should be truncated", 20);
        assert!(result.len() <= 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_exact_boundary() {
        let s = "abcde";
        assert_eq!(truncate(s, 5), "abcde");
        let result = truncate(s, 4);
        assert!(result.ends_with("..."));
    }
}
