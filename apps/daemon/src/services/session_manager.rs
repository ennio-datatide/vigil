//! Session event processing service.
//!
//! [`SessionManager`] subscribes to [`AppEvent`]s on the event bus and
//! processes hook events to update session state and create notifications.

#![allow(dead_code)] // Service is wired up in Task 1.16.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::config::Config;
use crate::db::models::{NotificationType, SessionStatus};
use crate::db::sqlite::SqliteDb;
use crate::deps::AppDeps;
use crate::events::{AppEvent, EventBus};
use crate::services::escalation::EscalationService;
use crate::services::notification_store::NotificationStore;
use crate::services::session_store::SessionStore;

/// Processes hook events and manages session lifecycle transitions.
pub(crate) struct SessionManager {
    db: Arc<SqliteDb>,
    event_bus: Arc<EventBus>,
    config: Arc<Config>,
    escalation_service: EscalationService,
}

impl SessionManager {
    /// Create a new session manager from the shared application dependencies.
    #[must_use]
    pub(crate) fn new(deps: &AppDeps) -> Self {
        Self {
            db: deps.db.clone(),
            event_bus: deps.event_bus.clone(),
            config: deps.config.clone(),
            escalation_service: deps.escalation_service.clone(),
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
                        old_status,
                        new_status,
                    }) => {
                        // Start/cancel escalation timers for blocker statuses.
                        if matches!(
                            new_status,
                            SessionStatus::NeedsInput | SessionStatus::AuthRequired
                        ) {
                            self.escalation_service.start_timer(&session_id).await;
                        } else if matches!(
                            old_status,
                            SessionStatus::NeedsInput | SessionStatus::AuthRequired
                        ) {
                            self.escalation_service.cancel_timer(&session_id).await;
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

        if payload_str.contains("\"auth_required\"") || payload_str.contains("\"permission_required\"") {
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

}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::db::sqlite::SqliteDb;
    use tempfile::TempDir;

    /// Create a test `SessionManager` with a short escalation timeout.
    fn test_manager(
        db: &Arc<SqliteDb>,
        event_bus: &Arc<EventBus>,
        dir: &TempDir,
    ) -> (SessionManager, EscalationService) {
        let escalation =
            EscalationService::new(Arc::clone(event_bus), Duration::from_millis(100));
        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(dir.path())),
            escalation_service: escalation.clone(),
        };
        (manager, escalation)
    }

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

        let (manager, _escalation) = test_manager(&db, &event_bus, &_dir);

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

        let (manager, _escalation) = test_manager(&db, &event_bus, &_dir);

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

        let (manager, _escalation) = test_manager(&db, &event_bus, &_dir);

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

        let (manager, _escalation) = test_manager(&db, &event_bus, &_dir);

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

        let (manager, _escalation) = test_manager(&db, &event_bus, &_dir);

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

        let (manager, _escalation) = test_manager(&db, &event_bus, &_dir);

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

        let (manager, _escalation) = test_manager(&db, &event_bus, &_dir);

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
    async fn needs_input_starts_escalation_timer() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));
        let escalation =
            EscalationService::new(Arc::clone(&event_bus), Duration::from_millis(50));
        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
            escalation_service: escalation.clone(),
        };

        let session_store = SessionStore::new(db.clone());
        session_store
            .create("sess-1", &sample_session_input())
            .await
            .unwrap();

        // Start the manager's event loop.
        let handle = manager.start();

        // Emit a StatusChanged event to NeedsInput.
        let _ = event_bus.emit(AppEvent::StatusChanged {
            session_id: "sess-1".to_string(),
            old_status: SessionStatus::Running,
            new_status: SessionStatus::NeedsInput,
        });

        // Wait for the escalation timer to fire.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(escalation.was_escalated("sess-1").await);

        handle.abort();
    }

    #[tokio::test]
    async fn resume_cancels_escalation_timer() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));
        let escalation =
            EscalationService::new(Arc::clone(&event_bus), Duration::from_millis(200));
        let manager = SessionManager {
            db: db.clone(),
            event_bus: event_bus.clone(),
            config: Arc::new(Config::for_testing(_dir.path())),
            escalation_service: escalation.clone(),
        };

        let session_store = SessionStore::new(db.clone());
        session_store
            .create("sess-1", &sample_session_input())
            .await
            .unwrap();

        let handle = manager.start();

        // Transition to NeedsInput — starts timer.
        let _ = event_bus.emit(AppEvent::StatusChanged {
            session_id: "sess-1".to_string(),
            old_status: SessionStatus::Running,
            new_status: SessionStatus::NeedsInput,
        });

        // Give the event loop time to process.
        tokio::time::sleep(Duration::from_millis(30)).await;

        // Resume — should cancel the timer.
        let _ = event_bus.emit(AppEvent::StatusChanged {
            session_id: "sess-1".to_string(),
            old_status: SessionStatus::NeedsInput,
            new_status: SessionStatus::Running,
        });

        // Wait past the original timeout.
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(!escalation.was_escalated("sess-1").await);

        handle.abort();
    }
}
