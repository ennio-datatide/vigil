//! Telegram notifier service.
//!
//! Subscribes to [`AppEvent::StatusChanged`] events on the event bus and sends
//! formatted Telegram messages when the new status matches the user's configured
//! event list.

use std::fmt::Write as _;
use std::sync::Arc;

use reqwest::Client;

use crate::api::settings::TelegramConfig;
use crate::db::models::SessionStatus;
use crate::db::sqlite::SqliteDb;
use crate::deps::AppDeps;
use crate::events::{AppEvent, EventBus};
use crate::services::session_store::SessionStore;
use crate::services::settings_store::SettingsStore;

/// Sends Telegram notifications on session status changes.
#[allow(dead_code)] // Will be instantiated in Task 1.16 (wiring).
pub(crate) struct TelegramNotifier {
    db: Arc<SqliteDb>,
    event_bus: Arc<EventBus>,
    http: Client,
}

#[allow(dead_code)] // Will be instantiated in Task 1.16 (wiring).
impl TelegramNotifier {
    /// Create a new notifier wired to the shared dependencies.
    pub(crate) fn new(deps: &AppDeps) -> Self {
        Self {
            db: deps.db.clone(),
            event_bus: deps.event_bus.clone(),
            http: Client::new(),
        }
    }

    /// Start listening for status-change events in a background task.
    pub(crate) fn start(self) -> tokio::task::JoinHandle<()> {
        let mut rx = self.event_bus.subscribe();

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(AppEvent::StatusChanged {
                        session_id,
                        old_status,
                        new_status,
                    }) => {
                        if let Err(e) = self
                            .handle_status_change(&session_id, &old_status, &new_status)
                            .await
                        {
                            tracing::error!(session_id, error = %e, "telegram notification failed");
                        }
                    }
                    Ok(AppEvent::EscalationTriggered { session_id }) => {
                        if let Err(e) = self.handle_escalation(&session_id).await {
                            tracing::error!(session_id, error = %e, "escalation notification failed");
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "telegram notifier lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    /// Process a single status change: load config, check filters, send message.
    async fn handle_status_change(
        &self,
        session_id: &str,
        _old_status: &SessionStatus,
        new_status: &SessionStatus,
    ) -> anyhow::Result<()> {
        let store = SettingsStore::new(self.db.clone());
        let Some(raw) = store.get("telegram").await? else {
            return Ok(());
        };
        let config: TelegramConfig = serde_json::from_str(&raw)?;

        if !config.enabled {
            return Ok(());
        }

        let Some(event_name) = status_to_event_name(new_status) else {
            return Ok(());
        };

        if !config.events.iter().any(|e| e == event_name) {
            return Ok(());
        }

        let emoji = status_emoji(new_status);

        let mut message = format!("{emoji} Session `{session_id}`\nStatus: *{event_name}*");

        if !config.dashboard_url.is_empty() {
            let _ = write!(message, "\n[Open Dashboard]({})", config.dashboard_url);
        }

        self.send_telegram(&config.bot_token, &config.chat_id, &message)
            .await
    }

    /// Handle an escalation event: send a Telegram message about a blocker
    /// that went unanswered past the timeout.
    async fn handle_escalation(&self, session_id: &str) -> anyhow::Result<()> {
        let settings_store = SettingsStore::new(self.db.clone());
        let Some(raw) = settings_store.get("telegram").await? else {
            return Ok(());
        };
        let config: TelegramConfig = serde_json::from_str(&raw)?;

        if !config.enabled {
            return Ok(());
        }

        let session_store = SessionStore::new(self.db.clone());
        let (prompt, project) = match session_store.get(session_id).await? {
            Some(s) => {
                let truncated = if s.prompt.len() > 80 {
                    format!("{}...", &s.prompt[..80])
                } else {
                    s.prompt.clone()
                };
                (truncated, s.project_path)
            }
            None => (session_id.to_string(), "unknown".to_string()),
        };

        let mut message = format!(
            "\u{23f0} *Escalation* \u{2014} Session needs attention!\n\n\
             *Session:* {prompt}\n\
             *Project:* {project}"
        );

        if !config.dashboard_url.is_empty() {
            let _ = write!(
                message,
                "\n\n[Open Dashboard]({}/dashboard/sessions/{session_id})",
                config.dashboard_url
            );
        }

        self.send_telegram(&config.bot_token, &config.chat_id, &message)
            .await
    }

    /// Send a message via the Telegram Bot API.
    async fn send_telegram(
        &self,
        bot_token: &str,
        chat_id: &str,
        message: &str,
    ) -> anyhow::Result<()> {
        let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");

        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": message,
                "parse_mode": "Markdown",
                "disable_web_page_preview": true,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Telegram API error: {body}");
        }

        Ok(())
    }

    /// Send a test notification (called from the settings route).
    #[allow(dead_code)] // Will be wired from the settings test route.
    pub(crate) async fn send_test(&self, config: &TelegramConfig) -> anyhow::Result<()> {
        self.send_telegram(
            &config.bot_token,
            &config.chat_id,
            "\u{1f514} *Test notification from Vigil*\nYour Telegram integration is working!",
        )
        .await
    }
}

/// Map a [`SessionStatus`] to the event-name string used in [`TelegramConfig::events`].
///
/// Returns `None` for statuses that should never trigger a notification.
pub(crate) fn status_to_event_name(status: &SessionStatus) -> Option<&'static str> {
    match status {
        SessionStatus::Completed => Some("session_done"),
        SessionStatus::Failed => Some("error"),
        SessionStatus::NeedsInput => Some("needs_input"),
        SessionStatus::AuthRequired => Some("auth_required"),
        SessionStatus::Cancelled => Some("cancelled"),
        SessionStatus::Queued | SessionStatus::Running | SessionStatus::Interrupted => None,
    }
}

/// Emoji prefix for a given session status.
pub(crate) fn status_emoji(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Completed => "\u{2705}",    // check mark
        SessionStatus::Failed => "\u{274c}",        // cross mark
        SessionStatus::NeedsInput => "\u{23f3}",    // hourglass
        SessionStatus::AuthRequired => "\u{1f510}", // locked with key
        SessionStatus::Cancelled => "\u{1f6ab}",    // prohibited
        _ => "\u{2139}\u{fe0f}",                    // info
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_to_event_name_maps_correctly() {
        assert_eq!(
            status_to_event_name(&SessionStatus::Completed),
            Some("session_done")
        );
        assert_eq!(
            status_to_event_name(&SessionStatus::Failed),
            Some("error")
        );
        assert_eq!(
            status_to_event_name(&SessionStatus::NeedsInput),
            Some("needs_input")
        );
        assert_eq!(
            status_to_event_name(&SessionStatus::AuthRequired),
            Some("auth_required")
        );
        assert_eq!(
            status_to_event_name(&SessionStatus::Cancelled),
            Some("cancelled")
        );
    }

    #[test]
    fn status_to_event_name_returns_none_for_non_notifiable() {
        assert_eq!(status_to_event_name(&SessionStatus::Queued), None);
        assert_eq!(status_to_event_name(&SessionStatus::Running), None);
        assert_eq!(status_to_event_name(&SessionStatus::Interrupted), None);
    }

    #[test]
    fn event_filtering_respects_configured_events() {
        let configured_events = vec!["session_done".to_string(), "error".to_string()];

        // Completed -> "session_done" is in the list
        let event_name = status_to_event_name(&SessionStatus::Completed).unwrap();
        assert!(configured_events.iter().any(|e| e == event_name));

        // Failed -> "error" is in the list
        let event_name = status_to_event_name(&SessionStatus::Failed).unwrap();
        assert!(configured_events.iter().any(|e| e == event_name));

        // NeedsInput -> "needs_input" is NOT in the list
        let event_name = status_to_event_name(&SessionStatus::NeedsInput).unwrap();
        assert!(!configured_events.iter().any(|e| e == event_name));

        // AuthRequired -> "auth_required" is NOT in the list
        let event_name = status_to_event_name(&SessionStatus::AuthRequired).unwrap();
        assert!(!configured_events.iter().any(|e| e == event_name));
    }

    #[test]
    fn status_emoji_returns_distinct_values() {
        // Just ensure each notifiable status has a non-empty emoji.
        let statuses = [
            SessionStatus::Completed,
            SessionStatus::Failed,
            SessionStatus::NeedsInput,
            SessionStatus::AuthRequired,
            SessionStatus::Cancelled,
        ];
        for status in &statuses {
            let emoji = status_emoji(status);
            assert!(!emoji.is_empty(), "emoji for {status:?} should not be empty");
        }
    }
}
