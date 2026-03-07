//! Application-wide event bus built on [`tokio::sync::broadcast`].
//!
//! Provides pub/sub for internal domain events such as session state changes,
//! hook events, and memory updates.

use tokio::sync::broadcast;

use crate::db::models::{Session, SessionStatus};

/// Domain events emitted throughout the daemon.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)] // Variants are used by later tasks.
pub enum AppEvent {
    /// A session's data was updated.
    SessionUpdate { session: Session },

    /// A session transitioned to a new status.
    StatusChanged {
        session_id: String,
        old_status: SessionStatus,
        new_status: SessionStatus,
    },

    /// A hook event was received from an agent.
    HookEvent {
        session_id: String,
        event_type: String,
        payload: Option<serde_json::Value>,
    },

    /// A new session was spawned.
    SessionSpawned { session: Session },

    /// Spawning a session failed.
    SessionSpawnFailed {
        session_id: String,
        reason: String,
    },

    /// A session was removed.
    SessionRemoved { session_id: String },

    /// A notification was created.
    NotificationCreated { notification_id: i64 },

    /// A child session was spawned from a parent.
    ChildSpawned {
        parent_id: String,
        child_id: String,
    },

    /// A child session completed.
    ChildCompleted {
        parent_id: String,
        child_id: String,
        success: bool,
    },

    /// A memory entry was created or updated.
    MemoryUpdated { memory_id: String },

    /// The acta (session summaries) were refreshed.
    ActaRefreshed { project_path: String },
}

/// Broadcast-based event bus for internal pub/sub.
#[derive(Debug)]
#[allow(dead_code)] // Methods are used by later tasks.
pub struct EventBus {
    sender: broadcast::Sender<AppEvent>,
}

#[allow(dead_code)] // Methods are used by later tasks.
impl EventBus {
    /// Create a new event bus with the given channel capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Get a reference to the sender (for cloning into producers).
    #[must_use]
    pub fn sender(&self) -> &broadcast::Sender<AppEvent> {
        &self.sender
    }

    /// Subscribe to the event stream.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.sender.subscribe()
    }

    /// Emit an event to all subscribers.
    ///
    /// Returns the number of receivers that received the event.
    /// Returns `Ok(0)` if there are no active subscribers.
    #[must_use]
    pub fn emit(&self, event: AppEvent) -> usize {
        // `send` returns Err only when there are zero receivers, which is fine.
        self.sender.send(event).unwrap_or(0)
    }
}
