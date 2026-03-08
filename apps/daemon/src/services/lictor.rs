//! Context size monitor (Lictor).
//!
//! [`LictorService`] is a programmatic monitor that subscribes to hook events,
//! tracks per-session token count estimates, and evaluates compaction thresholds.
//! It never interrupts running sessions — it only observes and reports.
//!
//! Tiered thresholds:
//! - **Background** (>80%): summarize oldest 30% via branch (Task 4.2).
//! - **Aggressive** (>85%): summarize oldest 50% via branch (Task 4.2).
//! - **Emergency** (>95%): hard truncation (Task 4.2).

#![allow(dead_code)] // Ahead of consumers (Task 4.2, 4.3).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::db::models::SessionStatus;
use crate::events::{AppEvent, EventBus};

/// Default maximum context window size for Claude (tokens).
const DEFAULT_MAX_TOKENS: usize = 200_000;

/// Maximum number of overflow recovery retries per session.
const MAX_OVERFLOW_RETRIES: u32 = 2;

/// Rough heuristic: 1 token ≈ 4 characters.
const CHARS_PER_TOKEN: usize = 4;

/// Estimated context state for a session.
#[derive(Debug, Clone)]
pub(crate) struct ContextState {
    /// Estimated token count.
    pub token_count: usize,
    /// Maximum context window size.
    pub max_tokens: usize,
    /// Number of compaction retries attempted.
    pub retry_count: u32,
}

impl ContextState {
    fn new() -> Self {
        Self {
            token_count: 0,
            max_tokens: DEFAULT_MAX_TOKENS,
            retry_count: 0,
        }
    }

    /// Context usage as a fraction (0.0 to 1.0+).
    #[allow(clippy::cast_precision_loss)] // Token counts fit well within f64 mantissa.
    pub(crate) fn usage(&self) -> f64 {
        self.token_count as f64 / self.max_tokens as f64
    }
}

/// Compaction level based on context usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactionLevel {
    /// Below 80% — no action needed.
    None,
    /// 80–85% — summarize oldest 30% via branch.
    Background,
    /// 85–95% — summarize oldest 50% via branch.
    Aggressive,
    /// >95% — hard truncation.
    Emergency,
}

impl CompactionLevel {
    /// Determine the compaction level from a usage fraction.
    pub(crate) fn from_usage(usage: f64) -> Self {
        if usage > 0.95 {
            Self::Emergency
        } else if usage > 0.85 {
            Self::Aggressive
        } else if usage > 0.80 {
            Self::Background
        } else {
            Self::None
        }
    }

    /// String representation for event serialization.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Background => "background",
            Self::Aggressive => "aggressive",
            Self::Emergency => "emergency",
        }
    }
}

impl std::fmt::Display for CompactionLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A compaction action to be taken on a session.
#[derive(Debug, Clone)]
pub(crate) struct CompactionAction {
    pub session_id: String,
    pub level: CompactionLevel,
    /// Fraction of oldest content to summarize (0.3 for Background, 0.5 for Aggressive, 0.0 for Emergency).
    pub summarize_fraction: f64,
}

/// Programmatic context size monitor.
///
/// Subscribes to the event bus, tracks per-session estimated token counts,
/// and evaluates compaction thresholds. Does not take any action itself —
/// that is left to the compaction service (Task 4.2).
pub(crate) struct LictorService {
    event_bus: Arc<EventBus>,
    /// Per-session estimated token counts.
    context_sizes: Arc<Mutex<HashMap<String, ContextState>>>,
}

impl LictorService {
    /// Create a new Lictor service.
    #[must_use]
    pub(crate) fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            event_bus,
            context_sizes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Update the estimated token count for a session.
    pub(crate) fn update_token_count(&self, session_id: &str, token_count: usize) {
        let mut map = self.context_sizes.lock().expect("lock poisoned");
        let state = map
            .entry(session_id.to_owned())
            .or_insert_with(ContextState::new);
        state.token_count = token_count;
    }

    /// Evaluate the compaction level for a session.
    pub(crate) fn evaluate(&self, session_id: &str) -> CompactionLevel {
        let map = self.context_sizes.lock().expect("lock poisoned");
        match map.get(session_id) {
            Some(state) => CompactionLevel::from_usage(state.usage()),
            None => CompactionLevel::None,
        }
    }

    /// Get the current context state for a session, if tracked.
    pub(crate) fn get_state(&self, session_id: &str) -> Option<ContextState> {
        let map = self.context_sizes.lock().expect("lock poisoned");
        map.get(session_id).cloned()
    }

    /// Remove a session from tracking (e.g., when it ends).
    pub(crate) fn remove_session(&self, session_id: &str) {
        let mut map = self.context_sizes.lock().expect("lock poisoned");
        map.remove(session_id);
    }

    /// Determine if a compaction action is needed for a session.
    ///
    /// Returns `Some(CompactionAction)` if the session's context usage exceeds
    /// a threshold, `None` otherwise.
    pub(crate) fn determine_action(&self, session_id: &str) -> Option<CompactionAction> {
        let map = self.context_sizes.lock().expect("lock poisoned");
        let state = map.get(session_id)?;
        let level = CompactionLevel::from_usage(state.usage());

        match level {
            CompactionLevel::None => None,
            CompactionLevel::Background => Some(CompactionAction {
                session_id: session_id.to_owned(),
                level,
                summarize_fraction: 0.3,
            }),
            CompactionLevel::Aggressive => Some(CompactionAction {
                session_id: session_id.to_owned(),
                level,
                summarize_fraction: 0.5,
            }),
            CompactionLevel::Emergency => Some(CompactionAction {
                session_id: session_id.to_owned(),
                level,
                summarize_fraction: 0.0,
            }),
        }
    }

    /// Handle a context overflow for a session.
    ///
    /// Returns `true` if the overflow was handled (emergency compaction + retry),
    /// `false` if max retries have been exceeded.
    pub(crate) fn handle_overflow(&self, session_id: &str) -> bool {
        let mut map = self.context_sizes.lock().expect("lock poisoned");
        let state = map
            .entry(session_id.to_owned())
            .or_insert_with(ContextState::new);

        if state.retry_count >= MAX_OVERFLOW_RETRIES {
            tracing::error!(
                %session_id,
                retry_count = state.retry_count,
                "lictor: max overflow retries exceeded, giving up"
            );
            return false;
        }

        state.retry_count += 1;
        let old_count = state.token_count;
        let new_count = state.max_tokens / 2;
        state.token_count = new_count;
        let retry = state.retry_count;

        tracing::warn!(
            %session_id,
            old_token_count = old_count,
            new_token_count = new_count,
            retry_count = retry,
            "lictor: overflow recovery — emergency compaction, dropped content logged for post-session review"
        );

        true
    }

    /// Execute a compaction action.
    ///
    /// - **Background / Aggressive**: emits a `CompactionRequested` event so that
    ///   a subscriber (e.g., session manager) can spawn a summarization branch.
    /// - **Emergency**: directly truncates by resetting the token count to 50%
    ///   of max tokens and logs the dropped content for post-session review.
    pub(crate) fn execute_compaction(&self, action: &CompactionAction) {
        match action.level {
            CompactionLevel::Background | CompactionLevel::Aggressive => {
                tracing::info!(
                    session_id = %action.session_id,
                    level = %action.level,
                    summarize_fraction = action.summarize_fraction,
                    "lictor: emitting compaction request"
                );

                let _ = self.event_bus.emit(AppEvent::CompactionRequested {
                    session_id: action.session_id.clone(),
                    level: action.level.as_str().to_owned(),
                    summarize_fraction: action.summarize_fraction,
                });
            }
            CompactionLevel::Emergency => {
                let mut map = self.context_sizes.lock().expect("lock poisoned");
                if let Some(state) = map.get_mut(&action.session_id) {
                    let old_count = state.token_count;
                    let new_count = state.max_tokens / 2;
                    state.token_count = new_count;
                    state.retry_count += 1;

                    tracing::warn!(
                        session_id = %action.session_id,
                        old_token_count = old_count,
                        new_token_count = new_count,
                        "lictor: emergency truncation — dropped content logged for post-session review"
                    );
                }

                let _ = self.event_bus.emit(AppEvent::CompactionRequested {
                    session_id: action.session_id.clone(),
                    level: action.level.as_str().to_owned(),
                    summarize_fraction: 0.0,
                });
            }
            CompactionLevel::None => {}
        }
    }

    /// Start the event-driven monitoring loop as a background task.
    ///
    /// Subscribes to the event bus and processes:
    /// - `HookEvent`: estimates token count from payload.
    /// - `StatusChanged` to terminal states: removes the session from tracking.
    ///
    /// Returns a [`JoinHandle`](tokio::task::JoinHandle) that the caller
    /// should store and abort on shutdown.
    pub(crate) fn start(self) -> tokio::task::JoinHandle<()> {
        let mut rx = self.event_bus.subscribe();
        let context_sizes = Arc::clone(&self.context_sizes);
        let event_bus = Arc::clone(&self.event_bus);

        // Keep `self` alive is not needed — we moved the Arcs out.
        // Build a helper that shares the same map.
        let monitor = LictorMonitor {
            event_bus,
            context_sizes,
        };

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => monitor.handle_event(&event),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "lictor event bus lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("lictor event bus closed, shutting down");
                        break;
                    }
                }
            }
        })
    }
}

/// Internal monitor that processes events from the bus.
struct LictorMonitor {
    event_bus: Arc<EventBus>,
    context_sizes: Arc<Mutex<HashMap<String, ContextState>>>,
}

impl LictorMonitor {
    /// Handle a single event from the bus.
    fn handle_event(&self, event: &AppEvent) {
        match event {
            AppEvent::HookEvent {
                session_id,
                event_type,
                payload,
            } => {
                self.process_hook_event(session_id, event_type, payload.as_ref());
            }
            AppEvent::StatusChanged {
                session_id,
                new_status,
                ..
            } => {
                if is_terminal(new_status) {
                    let mut map = self.context_sizes.lock().expect("lock poisoned");
                    map.remove(session_id);
                    tracing::debug!(%session_id, "lictor: removed terminal session");
                }
            }
            _ => {}
        }
    }

    /// Keywords that indicate a context overflow error in a hook event payload.
    const OVERFLOW_KEYWORDS: &[&str] = &[
        "context_overflow",
        "context_window",
        "token_limit",
        "maximum context length",
    ];

    /// Detect if a hook event indicates a context overflow error.
    fn detect_overflow(
        event_type: &str,
        payload: Option<&serde_json::Value>,
    ) -> bool {
        // Only consider error/stop events.
        let is_error_event = event_type.eq_ignore_ascii_case("error")
            || event_type.eq_ignore_ascii_case("stop");
        if !is_error_event {
            return false;
        }

        // Search the payload for overflow-related keywords.
        let payload_str = match payload {
            Some(v) => v.to_string().to_lowercase(),
            None => return false,
        };

        Self::OVERFLOW_KEYWORDS
            .iter()
            .any(|kw| payload_str.contains(kw))
    }

    /// Handle a context overflow for a session.
    ///
    /// Returns `true` if the overflow was handled (emergency compaction + retry),
    /// `false` if max retries have been exceeded.
    fn handle_overflow(&self, session_id: &str) -> bool {
        let mut map = self.context_sizes.lock().expect("lock poisoned");
        let state = map
            .entry(session_id.to_owned())
            .or_insert_with(ContextState::new);

        if state.retry_count >= MAX_OVERFLOW_RETRIES {
            tracing::error!(
                %session_id,
                retry_count = state.retry_count,
                "lictor: max overflow retries exceeded, giving up"
            );
            return false;
        }

        state.retry_count += 1;
        let old_count = state.token_count;
        let new_count = state.max_tokens / 2;
        state.token_count = new_count;
        let retry = state.retry_count;

        tracing::warn!(
            %session_id,
            old_token_count = old_count,
            new_token_count = new_count,
            retry_count = retry,
            "lictor: overflow recovery — emergency compaction, dropped content logged for post-session review"
        );

        true
    }

    /// Extract token count from a hook event payload, update tracking,
    /// and trigger compaction if thresholds are crossed. Also detects overflow
    /// errors and performs emergency compaction with retry.
    fn process_hook_event(
        &self,
        session_id: &str,
        event_type: &str,
        payload: Option<&serde_json::Value>,
    ) {
        // Check for overflow errors first.
        if Self::detect_overflow(event_type, payload) {
            let retrying = self.handle_overflow(session_id);
            if retrying {
                let _ = self.event_bus.emit(AppEvent::CompactionRequested {
                    session_id: session_id.to_owned(),
                    level: CompactionLevel::Emergency.as_str().to_owned(),
                    summarize_fraction: 0.0,
                });
            }
            return;
        }

        let token_count = payload.and_then(extract_token_count);

        if let Some(count) = token_count {
            let action = {
                let mut map = self.context_sizes.lock().expect("lock poisoned");
                let state = map
                    .entry(session_id.to_owned())
                    .or_insert_with(ContextState::new);
                state.token_count = count;

                let usage = state.usage();
                let level = CompactionLevel::from_usage(usage);

                if level != CompactionLevel::None {
                    tracing::info!(
                        %session_id,
                        token_count = count,
                        usage_pct = format!("{:.1}%", usage * 100.0),
                        ?level,
                        "lictor: context threshold reached"
                    );
                }

                // Build a compaction action if needed.
                match level {
                    CompactionLevel::None => None,
                    CompactionLevel::Background => Some(CompactionAction {
                        session_id: session_id.to_owned(),
                        level,
                        summarize_fraction: 0.3,
                    }),
                    CompactionLevel::Aggressive => Some(CompactionAction {
                        session_id: session_id.to_owned(),
                        level,
                        summarize_fraction: 0.5,
                    }),
                    CompactionLevel::Emergency => Some(CompactionAction {
                        session_id: session_id.to_owned(),
                        level,
                        summarize_fraction: 0.0,
                    }),
                }
            }; // Lock released here.

            // Execute the compaction action outside the lock.
            if let Some(action) = action {
                self.execute_compaction(&action);
            }
        }
    }

    /// Execute a compaction action from the monitor context.
    fn execute_compaction(&self, action: &CompactionAction) {
        match action.level {
            CompactionLevel::Background | CompactionLevel::Aggressive => {
                tracing::info!(
                    session_id = %action.session_id,
                    level = %action.level,
                    summarize_fraction = action.summarize_fraction,
                    "lictor: emitting compaction request"
                );

                let _ = self.event_bus.emit(AppEvent::CompactionRequested {
                    session_id: action.session_id.clone(),
                    level: action.level.as_str().to_owned(),
                    summarize_fraction: action.summarize_fraction,
                });
            }
            CompactionLevel::Emergency => {
                {
                    let mut map = self.context_sizes.lock().expect("lock poisoned");
                    if let Some(state) = map.get_mut(&action.session_id) {
                        let old_count = state.token_count;
                        let new_count = state.max_tokens / 2;
                        state.token_count = new_count;
                        state.retry_count += 1;

                        tracing::warn!(
                            session_id = %action.session_id,
                            old_token_count = old_count,
                            new_token_count = new_count,
                            "lictor: emergency truncation — dropped content logged for post-session review"
                        );
                    }
                }

                let _ = self.event_bus.emit(AppEvent::CompactionRequested {
                    session_id: action.session_id.clone(),
                    level: action.level.as_str().to_owned(),
                    summarize_fraction: 0.0,
                });
            }
            CompactionLevel::None => {}
        }
    }
}

/// Check if a session status is terminal.
fn is_terminal(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Completed
            | SessionStatus::Failed
            | SessionStatus::Cancelled
            | SessionStatus::Interrupted
    )
}

/// Extract a token count from a hook event payload.
///
/// Looks for a `tokenCount` field first. Falls back to estimating from the
/// `output` field length using the rough heuristic of 1 token ≈ 4 chars.
fn extract_token_count(payload: &serde_json::Value) -> Option<usize> {
    // Direct token count field.
    if let Some(count) = payload
        .get("tokenCount")
        .and_then(serde_json::Value::as_u64)
    {
        #[allow(clippy::cast_possible_truncation)] // Token counts never exceed usize.
        return Some(count as usize);
    }

    // Estimate from output length.
    if let Some(output) = payload.get("output").and_then(serde_json::Value::as_str) {
        let estimated = output.len() / CHARS_PER_TOKEN;
        return Some(estimated);
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_level_thresholds() {
        assert_eq!(CompactionLevel::from_usage(0.0), CompactionLevel::None);
        assert_eq!(CompactionLevel::from_usage(0.5), CompactionLevel::None);
        assert_eq!(CompactionLevel::from_usage(0.79), CompactionLevel::None);
        assert_eq!(CompactionLevel::from_usage(0.80), CompactionLevel::None);

        assert_eq!(CompactionLevel::from_usage(0.801), CompactionLevel::Background);
        assert_eq!(CompactionLevel::from_usage(0.85), CompactionLevel::Background);

        assert_eq!(CompactionLevel::from_usage(0.851), CompactionLevel::Aggressive);
        assert_eq!(CompactionLevel::from_usage(0.90), CompactionLevel::Aggressive);
        assert_eq!(CompactionLevel::from_usage(0.95), CompactionLevel::Aggressive);

        assert_eq!(CompactionLevel::from_usage(0.951), CompactionLevel::Emergency);
        assert_eq!(CompactionLevel::from_usage(1.0), CompactionLevel::Emergency);
        assert_eq!(CompactionLevel::from_usage(1.5), CompactionLevel::Emergency);
    }

    #[test]
    fn context_state_usage_calculation() {
        let state = ContextState {
            token_count: 100_000,
            max_tokens: 200_000,
            retry_count: 0,
        };
        let usage = state.usage();
        assert!((usage - 0.5).abs() < f64::EPSILON);

        let full = ContextState {
            token_count: 200_000,
            max_tokens: 200_000,
            retry_count: 0,
        };
        assert!((full.usage() - 1.0).abs() < f64::EPSILON);

        let empty = ContextState::new();
        assert!((empty.usage()).abs() < f64::EPSILON);
    }

    #[test]
    fn update_and_evaluate() {
        let event_bus = Arc::new(EventBus::new(16));
        let service = LictorService::new(event_bus);

        // No state yet — should be None level.
        assert_eq!(service.evaluate("s-1"), CompactionLevel::None);

        // Update to 50% usage.
        service.update_token_count("s-1", 100_000);
        assert_eq!(service.evaluate("s-1"), CompactionLevel::None);

        // Update to 82% usage — Background.
        service.update_token_count("s-1", 164_000);
        assert_eq!(service.evaluate("s-1"), CompactionLevel::Background);

        // Update to 90% usage — Aggressive.
        service.update_token_count("s-1", 180_000);
        assert_eq!(service.evaluate("s-1"), CompactionLevel::Aggressive);

        // Update to 96% usage — Emergency.
        service.update_token_count("s-1", 192_000);
        assert_eq!(service.evaluate("s-1"), CompactionLevel::Emergency);
    }

    #[test]
    fn remove_session_cleans_up() {
        let event_bus = Arc::new(EventBus::new(16));
        let service = LictorService::new(event_bus);

        service.update_token_count("s-1", 100_000);
        assert!(service.get_state("s-1").is_some());

        service.remove_session("s-1");
        assert!(service.get_state("s-1").is_none());
        assert_eq!(service.evaluate("s-1"), CompactionLevel::None);
    }

    #[test]
    fn get_state_returns_none_for_unknown() {
        let event_bus = Arc::new(EventBus::new(16));
        let service = LictorService::new(event_bus);
        assert!(service.get_state("nonexistent").is_none());
    }

    #[test]
    fn extract_token_count_from_direct_field() {
        let payload = serde_json::json!({ "tokenCount": 150_000 });
        assert_eq!(extract_token_count(&payload), Some(150_000));
    }

    #[test]
    fn extract_token_count_from_output_length() {
        // 400 chars / 4 chars per token = 100 tokens.
        let output = "a".repeat(400);
        let payload = serde_json::json!({ "output": output });
        assert_eq!(extract_token_count(&payload), Some(100));
    }

    #[test]
    fn extract_token_count_prefers_direct_over_estimate() {
        let payload = serde_json::json!({
            "tokenCount": 42,
            "output": "a".repeat(1000),
        });
        assert_eq!(extract_token_count(&payload), Some(42));
    }

    #[test]
    fn extract_token_count_returns_none_for_empty() {
        let payload = serde_json::json!({});
        assert_eq!(extract_token_count(&payload), None);
    }

    #[test]
    fn background_compaction_action() {
        let event_bus = Arc::new(EventBus::new(16));
        let service = LictorService::new(Arc::clone(&event_bus));

        // 82% usage → Background with 0.3 fraction.
        service.update_token_count("s-1", 164_000);

        let action = service.determine_action("s-1");
        assert!(action.is_some());

        let action = action.unwrap();
        assert_eq!(action.level, CompactionLevel::Background);
        assert!((action.summarize_fraction - 0.3).abs() < f64::EPSILON);
        assert_eq!(action.session_id, "s-1");
    }

    #[test]
    fn aggressive_compaction_action() {
        let event_bus = Arc::new(EventBus::new(16));
        let service = LictorService::new(Arc::clone(&event_bus));

        // 90% usage → Aggressive with 0.5 fraction.
        service.update_token_count("s-1", 180_000);

        let action = service.determine_action("s-1");
        assert!(action.is_some());

        let action = action.unwrap();
        assert_eq!(action.level, CompactionLevel::Aggressive);
        assert!((action.summarize_fraction - 0.5).abs() < f64::EPSILON);
        assert_eq!(action.session_id, "s-1");
    }

    #[test]
    fn emergency_compaction_truncates() {
        let event_bus = Arc::new(EventBus::new(16));
        let mut rx = event_bus.subscribe();
        let service = LictorService::new(Arc::clone(&event_bus));

        // 96% usage → Emergency.
        service.update_token_count("s-1", 192_000);

        let action = service.determine_action("s-1");
        assert!(action.is_some());
        let action = action.unwrap();
        assert_eq!(action.level, CompactionLevel::Emergency);
        assert!((action.summarize_fraction - 0.0).abs() < f64::EPSILON);

        // Execute emergency compaction — should truncate tokens to 50%.
        service.execute_compaction(&action);

        let state = service.get_state("s-1").unwrap();
        assert_eq!(state.token_count, 100_000); // 200_000 / 2
        assert_eq!(state.retry_count, 1);

        // Should have emitted a CompactionRequested event.
        let event = rx.try_recv().unwrap();
        match event {
            AppEvent::CompactionRequested {
                session_id,
                level,
                summarize_fraction,
            } => {
                assert_eq!(session_id, "s-1");
                assert_eq!(level, "emergency");
                assert!((summarize_fraction - 0.0).abs() < f64::EPSILON);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn no_action_below_threshold() {
        let event_bus = Arc::new(EventBus::new(16));
        let service = LictorService::new(Arc::clone(&event_bus));

        // 50% usage → no action.
        service.update_token_count("s-1", 100_000);

        let action = service.determine_action("s-1");
        assert!(action.is_none());
    }

    #[test]
    fn compaction_level_display() {
        assert_eq!(CompactionLevel::None.as_str(), "none");
        assert_eq!(CompactionLevel::Background.as_str(), "background");
        assert_eq!(CompactionLevel::Aggressive.as_str(), "aggressive");
        assert_eq!(CompactionLevel::Emergency.as_str(), "emergency");
        assert_eq!(format!("{}", CompactionLevel::Background), "background");
    }

    // -- Overflow recovery tests (Task 4.3) --

    #[test]
    fn overflow_detected_from_error_event() {
        let event_bus = Arc::new(EventBus::new(16));
        let mut rx = event_bus.subscribe();
        let monitor = LictorMonitor {
            event_bus: Arc::clone(&event_bus),
            context_sizes: Arc::new(Mutex::new(HashMap::new())),
        };

        // Seed session with high token count.
        {
            let mut map = monitor.context_sizes.lock().unwrap();
            map.insert(
                "s-1".to_owned(),
                ContextState {
                    token_count: 190_000,
                    max_tokens: DEFAULT_MAX_TOKENS,
                    retry_count: 0,
                },
            );
        }

        // An error event with "context_overflow" should trigger overflow handling.
        let payload = serde_json::json!({ "error": "context_overflow detected" });
        monitor.process_hook_event("s-1", "Error", Some(&payload));

        // Token count should be reduced to 50%.
        let map = monitor.context_sizes.lock().unwrap();
        let state = map.get("s-1").unwrap();
        assert_eq!(state.token_count, DEFAULT_MAX_TOKENS / 2);
        assert_eq!(state.retry_count, 1);

        // A CompactionRequested event should have been emitted.
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AppEvent::CompactionRequested { .. }));
    }

    #[test]
    fn overflow_retries_capped_at_max() {
        let event_bus = Arc::new(EventBus::new(16));
        let monitor = LictorMonitor {
            event_bus: Arc::clone(&event_bus),
            context_sizes: Arc::new(Mutex::new(HashMap::new())),
        };

        // Seed session with retry_count at MAX_OVERFLOW_RETRIES.
        {
            let mut map = monitor.context_sizes.lock().unwrap();
            map.insert(
                "s-1".to_owned(),
                ContextState {
                    token_count: 190_000,
                    max_tokens: DEFAULT_MAX_TOKENS,
                    retry_count: MAX_OVERFLOW_RETRIES,
                },
            );
        }

        // Should return false — retries exhausted.
        assert!(!monitor.handle_overflow("s-1"));

        // Token count should be unchanged.
        let map = monitor.context_sizes.lock().unwrap();
        let state = map.get("s-1").unwrap();
        assert_eq!(state.token_count, 190_000);
        assert_eq!(state.retry_count, MAX_OVERFLOW_RETRIES);
    }

    #[test]
    fn overflow_triggers_emergency_compaction() {
        let event_bus = Arc::new(EventBus::new(16));
        let monitor = LictorMonitor {
            event_bus: Arc::clone(&event_bus),
            context_sizes: Arc::new(Mutex::new(HashMap::new())),
        };

        // Seed session with high token count, no prior retries.
        {
            let mut map = monitor.context_sizes.lock().unwrap();
            map.insert(
                "s-1".to_owned(),
                ContextState {
                    token_count: 195_000,
                    max_tokens: DEFAULT_MAX_TOKENS,
                    retry_count: 0,
                },
            );
        }

        // First overflow — should succeed.
        assert!(monitor.handle_overflow("s-1"));
        {
            let map = monitor.context_sizes.lock().unwrap();
            let state = map.get("s-1").unwrap();
            assert_eq!(state.token_count, DEFAULT_MAX_TOKENS / 2);
            assert_eq!(state.retry_count, 1);
        }

        // Simulate tokens growing back up.
        {
            let mut map = monitor.context_sizes.lock().unwrap();
            map.get_mut("s-1").unwrap().token_count = 195_000;
        }

        // Second overflow — should still succeed (retry_count becomes 2).
        assert!(monitor.handle_overflow("s-1"));
        {
            let map = monitor.context_sizes.lock().unwrap();
            let state = map.get("s-1").unwrap();
            assert_eq!(state.token_count, DEFAULT_MAX_TOKENS / 2);
            assert_eq!(state.retry_count, 2);
        }

        // Third overflow — should fail (max retries reached).
        assert!(!monitor.handle_overflow("s-1"));
    }

    #[test]
    fn non_overflow_error_ignored() {
        let event_bus = Arc::new(EventBus::new(16));
        let monitor = LictorMonitor {
            event_bus: Arc::clone(&event_bus),
            context_sizes: Arc::new(Mutex::new(HashMap::new())),
        };

        // Seed session.
        {
            let mut map = monitor.context_sizes.lock().unwrap();
            map.insert(
                "s-1".to_owned(),
                ContextState {
                    token_count: 190_000,
                    max_tokens: DEFAULT_MAX_TOKENS,
                    retry_count: 0,
                },
            );
        }

        // An error event that is NOT about context overflow should not trigger overflow handling.
        let payload = serde_json::json!({ "error": "file not found" });
        monitor.process_hook_event("s-1", "Error", Some(&payload));

        // Token count should be unchanged (no token info to extract either).
        let map = monitor.context_sizes.lock().unwrap();
        let state = map.get("s-1").unwrap();
        assert_eq!(state.token_count, 190_000);
        assert_eq!(state.retry_count, 0);
    }
}
