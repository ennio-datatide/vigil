//! Session event processing service.
//!
//! [`SessionManager`] subscribes to [`AppEvent`]s on the event bus and
//! processes hook events to update session state, create notifications,
//! and advance pipelines when sessions complete.

#![allow(dead_code)] // Service is wired up in Task 1.16.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::config::Config;
use crate::db::models::{NotificationType, SessionStatus};
use crate::db::sqlite::SqliteDb;
use crate::deps::AppDeps;
use crate::events::{AppEvent, EventBus};
use crate::services::notification_store::NotificationStore;
use crate::services::pipeline_store::PipelineStore;
use crate::services::session_store::{CreateSessionInput, SessionStore};

/// Processes hook events and manages session lifecycle transitions.
pub(crate) struct SessionManager {
    db: Arc<SqliteDb>,
    event_bus: Arc<EventBus>,
    config: Arc<Config>,
}

impl SessionManager {
    /// Create a new session manager from the shared application dependencies.
    #[must_use]
    pub(crate) fn new(deps: &AppDeps) -> Self {
        Self {
            db: deps.db.clone(),
            event_bus: deps.event_bus.clone(),
            config: deps.config.clone(),
        }
    }

    /// Start the event processing loop as a background task.
    ///
    /// Returns a [`JoinHandle`](tokio::task::JoinHandle) that the caller
    /// should store and abort on shutdown.
    pub(crate) fn start(self) -> tokio::task::JoinHandle<()> {
        let mut rx = self.event_bus.subscribe();

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(AppEvent::HookEvent {
                        session_id,
                        event_type,
                        payload,
                    }) => {
                        if let Err(e) = self
                            .handle_hook_event(&session_id, &event_type, payload.as_ref())
                            .await
                        {
                            tracing::error!(
                                session_id,
                                event_type,
                                error = %e,
                                "failed to process hook event"
                            );
                        }
                    }
                    Ok(AppEvent::StatusChanged {
                        session_id,
                        new_status,
                        ..
                    }) => {
                        if new_status == SessionStatus::Completed
                            && let Err(e) = self.advance_pipeline(&session_id).await
                        {
                            tracing::error!(
                                session_id,
                                error = %e,
                                "pipeline advancement failed"
                            );
                        }

                        // Detect child session completion and emit ChildCompleted.
                        // success = true only for Completed; both Failed and Cancelled
                        // map to success = false (cancelled is treated as unsuccessful).
                        if matches!(
                            new_status,
                            SessionStatus::Completed
                                | SessionStatus::Failed
                                | SessionStatus::Cancelled
                        ) && let Err(e) = self
                            .handle_child_completion(
                                &session_id,
                                new_status == SessionStatus::Completed,
                            )
                            .await
                        {
                            tracing::error!(
                                session_id,
                                error = %e,
                                "child completion handling failed"
                            );
                        }
                    }
                    Ok(_) => {} // Ignore other events.
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "session manager lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    /// Dispatch a hook event to the appropriate handler.
    async fn handle_hook_event(
        &self,
        session_id: &str,
        event_type: &str,
        payload: Option<&serde_json::Value>,
    ) -> anyhow::Result<()> {
        match event_type {
            "Stop" => self.handle_stop(session_id, payload).await,
            "Notification" => self.handle_notification(session_id, payload).await,
            _ => Ok(()), // Other hook events are informational only.
        }
    }

    /// Handle a `Stop` hook event.
    ///
    /// If the payload indicates an auth-related issue, transitions the session
    /// to `auth_required` and creates a notification. Normal stops are handled
    /// by the agent spawner's exit monitor.
    async fn handle_stop(
        &self,
        session_id: &str,
        payload: Option<&serde_json::Value>,
    ) -> anyhow::Result<()> {
        let payload_str = payload
            .map(std::string::ToString::to_string)
            .unwrap_or_default();

        if payload_str.contains("auth_required") || payload_str.contains("permission") {
            let store = SessionStore::new(self.db.clone());
            let session = store
                .update_status(session_id, SessionStatus::AuthRequired, None, None)
                .await?;
            let _ = self.event_bus.emit(AppEvent::SessionUpdate { session });

            let notif_store = NotificationStore::new(self.db.clone());
            let notif = notif_store
                .create(
                    session_id,
                    NotificationType::AuthRequired,
                    "Session requires authentication",
                )
                .await?;
            let _ = self.event_bus.emit(AppEvent::NotificationCreated {
                notification_id: notif.id,
            });
        }
        // Normal stops are handled by the agent spawner's exit monitor.

        Ok(())
    }

    /// Handle a `Notification` hook event.
    ///
    /// Transitions the session to `needs_input` and creates a notification
    /// with the message from the payload.
    async fn handle_notification(
        &self,
        session_id: &str,
        payload: Option<&serde_json::Value>,
    ) -> anyhow::Result<()> {
        let message = payload
            .and_then(|p| p.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Agent needs attention");

        let store = SessionStore::new(self.db.clone());
        let session = store
            .update_status(session_id, SessionStatus::NeedsInput, None, None)
            .await?;
        let _ = self.event_bus.emit(AppEvent::SessionUpdate { session });

        let notif_store = NotificationStore::new(self.db.clone());
        let notif = notif_store
            .create(session_id, NotificationType::NeedsInput, message)
            .await?;
        let _ = self.event_bus.emit(AppEvent::NotificationCreated {
            notification_id: notif.id,
        });

        Ok(())
    }

    /// Handle child session completion.
    ///
    /// When a session with a `parent_id` reaches a terminal status, emits a
    /// [`AppEvent::ChildCompleted`] event and creates a notification for the
    /// parent session.
    async fn handle_child_completion(
        &self,
        session_id: &str,
        success: bool,
    ) -> anyhow::Result<()> {
        let store = SessionStore::new(self.db.clone());
        let Some(session) = store.get(session_id).await? else {
            tracing::debug!(session_id, "session not found for child completion check");
            return Ok(());
        };

        let Some(parent_id) = &session.parent_id else {
            return Ok(()); // Not a child session.
        };

        let _ = self.event_bus.emit(AppEvent::ChildCompleted {
            parent_id: parent_id.clone(),
            child_id: session_id.to_string(),
            success,
        });

        // Create a notification for the parent.
        let status_label = if success { "completed" } else { "failed" };
        let message = format!("Child session {session_id} {status_label}");

        let notif_store = NotificationStore::new(self.db.clone());
        let notif = notif_store
            .create(parent_id, NotificationType::SessionDone, &message)
            .await?;
        let _ = self.event_bus.emit(AppEvent::NotificationCreated {
            notification_id: notif.id,
        });

        Ok(())
    }

    /// Advance a pipeline after a session completes.
    ///
    /// If the completed session belongs to a pipeline and there is a next step
    /// connected via an edge, a new session is created for that step.
    async fn advance_pipeline(&self, session_id: &str) -> anyhow::Result<()> {
        let session_store = SessionStore::new(self.db.clone());
        let Some(session) = session_store.get(session_id).await? else {
            return Ok(());
        };

        let Some(pipeline_id) = &session.pipeline_id else {
            return Ok(());
        };
        let Some(step_index) = session.pipeline_step_index else {
            return Ok(());
        };

        let pipeline_store = PipelineStore::new(self.db.clone());
        let Some(pipeline) = pipeline_store.get(pipeline_id).await? else {
            return Ok(());
        };

        // Find current step and its outgoing edge.
        let step_usize = usize::try_from(step_index)?;
        let Some(current) = pipeline.steps.get(step_usize) else {
            return Ok(());
        };

        // Find next step via edges.
        let next_step_id = pipeline
            .edges
            .iter()
            .find(|e| e.source == current.id)
            .map(|e| &e.target);

        let Some(next_id) = next_step_id else {
            return Ok(()); // No more steps -- pipeline is done.
        };

        let Some(next_idx) = pipeline.steps.iter().position(|s| s.id == *next_id) else {
            return Ok(());
        };
        let next = &pipeline.steps[next_idx];

        // Create a new session for the next step.
        let new_id = uuid::Uuid::new_v4().to_string();
        let input = CreateSessionInput {
            project_path: session.project_path,
            prompt: next.prompt.clone(),
            skill: next.skill.clone(),
            role: None,
            parent_id: Some(session_id.to_string()),
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: Some(pipeline_id.clone()),
        };

        let _created = session_store.create(&new_id, &input).await?;

        // Set the pipeline step index on the new session.
        let new_session = session_store
            .set_pipeline_step_index(&new_id, i32::try_from(next_idx)?)
            .await?;

        let _ = self.event_bus.emit(AppEvent::SessionUpdate {
            session: new_session,
        });

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{PipelineEdge, PipelineStep, Position};
    use crate::db::sqlite::SqliteDb;
    use crate::services::pipeline_store::CreatePipelineInput;
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

    fn sample_session_input() -> CreateSessionInput {
        CreateSessionInput {
            project_path: "/tmp/proj".into(),
            prompt: "do something".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        }
    }

    #[tokio::test]
    async fn handle_stop_with_auth_required() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));
        let mut rx = event_bus.subscribe();

        let session_store = SessionStore::new(db.clone());
        session_store
            .create("sess-1", &sample_session_input())
            .await
            .unwrap();

        // Transition to running first so we can test auth_required transition.
        session_store
            .update_status("sess-1", SessionStatus::Running, None, None)
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        let payload = serde_json::json!({ "reason": "auth_required" });
        manager
            .handle_stop("sess-1", Some(&payload))
            .await
            .unwrap();

        // Verify session status was updated.
        let session = session_store.get("sess-1").await.unwrap().unwrap();
        assert_eq!(session.status, SessionStatus::AuthRequired);

        // Verify notification was created.
        let notif_store = NotificationStore::new(db.clone());
        let notifications = notif_store.list(false).await.unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].notification_type,
            NotificationType::AuthRequired
        );
        assert_eq!(notifications[0].session_id, "sess-1");

        // Verify events were emitted (SessionUpdate + NotificationCreated).
        let event1 = rx.try_recv().unwrap();
        assert!(matches!(event1, AppEvent::SessionUpdate { .. }));
        let event2 = rx.try_recv().unwrap();
        assert!(matches!(event2, AppEvent::NotificationCreated { .. }));
    }

    #[tokio::test]
    async fn handle_stop_normal_is_noop() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));

        let session_store = SessionStore::new(db.clone());
        session_store
            .create("sess-1", &sample_session_input())
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        // Normal stop (no auth_required in payload).
        let payload = serde_json::json!({ "reason": "completed" });
        manager
            .handle_stop("sess-1", Some(&payload))
            .await
            .unwrap();

        // Session should still be queued (unchanged).
        let session = session_store.get("sess-1").await.unwrap().unwrap();
        assert_eq!(session.status, SessionStatus::Queued);
    }

    #[tokio::test]
    async fn handle_notification_event() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));

        let session_store = SessionStore::new(db.clone());
        session_store
            .create("sess-1", &sample_session_input())
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        let payload = serde_json::json!({ "message": "Please review the changes" });
        manager
            .handle_notification("sess-1", Some(&payload))
            .await
            .unwrap();

        // Verify session status.
        let session = session_store.get("sess-1").await.unwrap().unwrap();
        assert_eq!(session.status, SessionStatus::NeedsInput);

        // Verify notification.
        let notif_store = NotificationStore::new(db.clone());
        let notifications = notif_store.list(false).await.unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].notification_type,
            NotificationType::NeedsInput
        );
        assert_eq!(notifications[0].message, "Please review the changes");
    }

    #[tokio::test]
    async fn handle_notification_default_message() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));

        let session_store = SessionStore::new(db.clone());
        session_store
            .create("sess-1", &sample_session_input())
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        // No message in payload -- should use default.
        manager
            .handle_notification("sess-1", None)
            .await
            .unwrap();

        let notif_store = NotificationStore::new(db.clone());
        let notifications = notif_store.list(false).await.unwrap();
        assert_eq!(notifications[0].message, "Agent needs attention");
    }

    #[tokio::test]
    async fn pipeline_advancement_creates_next_session() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));

        // Create a two-step pipeline.
        let pipeline_store = PipelineStore::new(db.clone());
        let pipeline = pipeline_store
            .create(CreatePipelineInput {
                name: "Test Pipeline".to_string(),
                description: String::new(),
                steps: vec![
                    PipelineStep {
                        id: "step-1".to_string(),
                        label: "Implement".to_string(),
                        prompt: "Write the code".to_string(),
                        skill: Some("coding".to_string()),
                        position: Position { x: 0.0, y: 0.0 },
                    },
                    PipelineStep {
                        id: "step-2".to_string(),
                        label: "Test".to_string(),
                        prompt: "Write tests".to_string(),
                        skill: Some("testing".to_string()),
                        position: Position { x: 100.0, y: 0.0 },
                    },
                ],
                edges: vec![PipelineEdge {
                    id: "edge-1".to_string(),
                    source: "step-1".to_string(),
                    target: "step-2".to_string(),
                }],
                is_default: false,
            })
            .await
            .unwrap();

        // Create a session for step-1.
        let session_store = SessionStore::new(db.clone());
        let input = CreateSessionInput {
            project_path: "/tmp/proj".into(),
            prompt: "Write the code".into(),
            skill: Some("coding".into()),
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: Some(pipeline.id.clone()),
        };
        session_store.create("sess-1", &input).await.unwrap();
        session_store
            .set_pipeline_step_index("sess-1", 0)
            .await
            .unwrap();

        // Complete the session.
        session_store
            .update_status(
                "sess-1",
                SessionStatus::Completed,
                None,
                Some(1_700_000_000_000),
            )
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        manager.advance_pipeline("sess-1").await.unwrap();

        // Verify a new session was created.
        let all_sessions = session_store.list().await.unwrap();
        assert_eq!(all_sessions.len(), 2);

        // Find the new session (not sess-1).
        let new_session = all_sessions.iter().find(|s| s.id != "sess-1").unwrap();
        assert_eq!(new_session.prompt, "Write tests");
        assert_eq!(new_session.pipeline_id, Some(pipeline.id.clone()));
        assert_eq!(new_session.pipeline_step_index, Some(1));
        assert_eq!(new_session.parent_id, Some("sess-1".to_string()));
        assert_eq!(new_session.status, SessionStatus::Queued);
    }

    #[tokio::test]
    async fn pipeline_advancement_no_next_step() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));

        // Single-step pipeline with no edges.
        let pipeline_store = PipelineStore::new(db.clone());
        let pipeline = pipeline_store
            .create(CreatePipelineInput {
                name: "Single Step".to_string(),
                description: String::new(),
                steps: vec![PipelineStep {
                    id: "step-1".to_string(),
                    label: "Only step".to_string(),
                    prompt: "Do it".to_string(),
                    skill: None,
                    position: Position { x: 0.0, y: 0.0 },
                }],
                edges: vec![],
                is_default: false,
            })
            .await
            .unwrap();

        let session_store = SessionStore::new(db.clone());
        let input = CreateSessionInput {
            project_path: "/tmp/proj".into(),
            prompt: "Do it".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: Some(pipeline.id.clone()),
        };
        session_store.create("sess-1", &input).await.unwrap();
        session_store
            .set_pipeline_step_index("sess-1", 0)
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        // Should be a no-op -- no next step.
        manager.advance_pipeline("sess-1").await.unwrap();

        let all_sessions = session_store.list().await.unwrap();
        assert_eq!(all_sessions.len(), 1);
    }

    #[tokio::test]
    async fn child_completion_emits_event() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));
        let mut rx = event_bus.subscribe();

        let session_store = SessionStore::new(db.clone());

        // Create parent session.
        session_store
            .create("parent-1", &sample_session_input())
            .await
            .unwrap();

        // Create child session with parent_id.
        let child_input = CreateSessionInput {
            parent_id: Some("parent-1".to_string()),
            ..sample_session_input()
        };
        session_store
            .create("child-1", &child_input)
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        manager
            .handle_child_completion("child-1", true)
            .await
            .unwrap();

        // Verify ChildCompleted event was emitted.
        let event = rx.try_recv().unwrap();
        match event {
            AppEvent::ChildCompleted {
                parent_id,
                child_id,
                success,
            } => {
                assert_eq!(parent_id, "parent-1");
                assert_eq!(child_id, "child-1");
                assert!(success);
            }
            other => panic!("expected ChildCompleted, got {other:?}"),
        }

        // Verify notification was created for the parent.
        let notif_store = NotificationStore::new(db.clone());
        let notifications = notif_store.list(false).await.unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].session_id, "parent-1");
        assert_eq!(
            notifications[0].notification_type,
            NotificationType::SessionDone
        );
        assert!(notifications[0].message.contains("child-1"));
        assert!(notifications[0].message.contains("completed"));
    }

    #[tokio::test]
    async fn non_child_completion_no_event() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));
        let mut rx = event_bus.subscribe();

        let session_store = SessionStore::new(db.clone());

        // Create session without parent_id.
        session_store
            .create("sess-1", &sample_session_input())
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        manager
            .handle_child_completion("sess-1", true)
            .await
            .unwrap();

        // No events should be emitted.
        assert!(rx.try_recv().is_err());

        // No notifications should be created.
        let notif_store = NotificationStore::new(db.clone());
        let notifications = notif_store.list(false).await.unwrap();
        assert!(notifications.is_empty());
    }

    #[tokio::test]
    async fn failed_child_emits_event_with_success_false() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));
        let mut rx = event_bus.subscribe();

        let session_store = SessionStore::new(db.clone());

        // Create parent and child sessions.
        session_store
            .create("parent-1", &sample_session_input())
            .await
            .unwrap();

        let child_input = CreateSessionInput {
            parent_id: Some("parent-1".to_string()),
            ..sample_session_input()
        };
        session_store
            .create("child-1", &child_input)
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        // Handle as failed (success = false).
        manager
            .handle_child_completion("child-1", false)
            .await
            .unwrap();

        // Verify ChildCompleted event with success=false.
        let event = rx.try_recv().unwrap();
        match event {
            AppEvent::ChildCompleted {
                parent_id,
                child_id,
                success,
            } => {
                assert_eq!(parent_id, "parent-1");
                assert_eq!(child_id, "child-1");
                assert!(!success);
            }
            other => panic!("expected ChildCompleted, got {other:?}"),
        }

        // Verify notification says "failed".
        let notif_store = NotificationStore::new(db.clone());
        let notifications = notif_store.list(false).await.unwrap();
        assert_eq!(notifications.len(), 1);
        assert!(notifications[0].message.contains("failed"));
    }

    #[tokio::test]
    async fn pipeline_advancement_skipped_without_pipeline() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));

        let session_store = SessionStore::new(db.clone());
        session_store
            .create("sess-1", &sample_session_input())
            .await
            .unwrap();

        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
        };

        // Should be a no-op -- no pipeline_id.
        manager.advance_pipeline("sess-1").await.unwrap();

        let all_sessions = session_store.list().await.unwrap();
        assert_eq!(all_sessions.len(), 1);
    }
}
