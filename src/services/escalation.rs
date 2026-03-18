//! Blocker escalation timer service.
//!
//! When a session enters `needs_input` or `auth_required` and the user
//! doesn't respond within a configurable timeout (default 2 minutes),
//! an `EscalationTriggered` event is emitted so the notifier can send
//! a Telegram notification.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::events::{AppEvent, EventBus};

/// Default escalation timeout: 2 minutes.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone)]
#[allow(dead_code)] // Will be wired in Task 1.4.
pub(crate) struct EscalationService {
    event_bus: Arc<EventBus>,
    timeout: Duration,
    timers: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    #[cfg(test)]
    escalated: Arc<Mutex<Vec<String>>>,
}

#[allow(dead_code)] // Will be wired in Task 1.4.
impl EscalationService {
    pub(crate) fn new(event_bus: Arc<EventBus>, timeout: Duration) -> Self {
        Self {
            event_bus,
            timeout,
            timers: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            escalated: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[allow(dead_code)] // Will be used when wiring the service.
    pub(crate) fn with_default_timeout(event_bus: Arc<EventBus>) -> Self {
        Self::new(event_bus, DEFAULT_TIMEOUT)
    }

    /// Start an escalation timer for a session.
    /// If a timer already exists for this session, it is replaced.
    pub(crate) async fn start_timer(&self, session_id: &str) {
        // Cancel existing timer if any
        self.cancel_timer(session_id).await;

        let sid = session_id.to_owned();
        let timeout = self.timeout;
        let event_bus = Arc::clone(&self.event_bus);
        #[cfg(test)]
        let escalated = Arc::clone(&self.escalated);

        let handle = tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            #[cfg(test)]
            escalated.lock().await.push(sid.clone());
            let _ = event_bus.emit(AppEvent::EscalationTriggered { session_id: sid });
        });

        self.timers
            .lock()
            .await
            .insert(session_id.to_owned(), handle);
    }

    /// Cancel an escalation timer (user responded in time).
    pub(crate) async fn cancel_timer(&self, session_id: &str) {
        if let Some(handle) = self.timers.lock().await.remove(session_id) {
            handle.abort();
        }
    }

    /// Check if a session was escalated (test-only).
    #[cfg(test)]
    pub(crate) async fn was_escalated(&self, session_id: &str) -> bool {
        self.escalated.lock().await.contains(&session_id.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn escalation_fires_after_timeout() {
        let event_bus = Arc::new(EventBus::new(64));
        let service = EscalationService::new(event_bus, Duration::from_millis(50));

        service.start_timer("session-1").await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(service.was_escalated("session-1").await);
    }

    #[tokio::test]
    async fn escalation_cancelled_before_timeout() {
        let event_bus = Arc::new(EventBus::new(64));
        let service = EscalationService::new(event_bus, Duration::from_millis(200));

        service.start_timer("session-1").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        service.cancel_timer("session-1").await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(!service.was_escalated("session-1").await);
    }

    #[tokio::test]
    async fn replacing_timer_cancels_previous() {
        let event_bus = Arc::new(EventBus::new(64));
        let service = EscalationService::new(event_bus, Duration::from_millis(100));

        service.start_timer("session-1").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Replace with new timer — old one should be cancelled
        service.start_timer("session-1").await;
        tokio::time::sleep(Duration::from_millis(60)).await;
        // First timer would have fired by now if not cancelled
        // But we replaced it, so it shouldn't have escalated yet
        // The new timer has ~40ms left
        assert!(!service.was_escalated("session-1").await);

        // Wait for new timer to fire
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(service.was_escalated("session-1").await);
    }

    #[tokio::test]
    async fn emits_event_on_escalation() {
        let event_bus = Arc::new(EventBus::new(64));
        let mut rx = event_bus.subscribe();
        let service = EscalationService::new(Arc::clone(&event_bus), Duration::from_millis(50));

        service.start_timer("session-1").await;

        // Should receive the event
        let event = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("timed out")
            .expect("recv error");

        match event {
            AppEvent::EscalationTriggered { session_id } => {
                assert_eq!(session_id, "session-1");
            }
            other => panic!("expected EscalationTriggered, got {other:?}"),
        }
    }
}
