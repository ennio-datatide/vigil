//! Telegram long-polling service for bidirectional Vigil chat.
//!
//! Polls `getUpdates` from the Telegram Bot API and routes incoming
//! messages through the Vigil chat pipeline. Responses are sent back
//! to the same Telegram chat.

use reqwest::Client;

use crate::api::settings::TelegramConfig;
use crate::deps::AppDeps;
use crate::services::settings_store::SettingsStore;

/// Polls Telegram for incoming messages and routes them through Vigil.
pub(crate) struct TelegramPoller {
    deps: AppDeps,
    http: Client,
}

impl TelegramPoller {
    /// Create a new poller wired to the shared dependencies.
    pub(crate) fn new(deps: &AppDeps) -> Self {
        Self {
            deps: deps.clone(),
            http: Client::new(),
        }
    }

    /// Start the polling loop in a background task.
    pub(crate) fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut offset: i64 = 0;

            loop {
                // Load config each iteration so changes are picked up live.
                let config = match self.load_config().await {
                    Some(c) if c.enabled => c,
                    _ => {
                        // Not configured or disabled — sleep and retry.
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                        continue;
                    }
                };

                // Long-poll for updates (30s timeout on the Telegram side).
                match self.get_updates(&config.bot_token, offset, 30).await {
                    Ok(updates) => {
                        for update in updates {
                            // Always advance offset to avoid reprocessing.
                            if update.update_id >= offset {
                                offset = update.update_id + 1;
                            }

                            // Only process text messages from the configured chat.
                            if let Some(ref msg) = update.message {
                                let chat_id = msg.chat.id.to_string();
                                if chat_id != config.chat_id {
                                    tracing::debug!(
                                        chat_id,
                                        expected = config.chat_id,
                                        "ignoring message from unconfigured chat"
                                    );
                                    continue;
                                }

                                if let Some(ref text) = msg.text {
                                    tracing::info!(
                                        chat_id,
                                        text_len = text.len(),
                                        "telegram message received"
                                    );
                                    self.handle_message(&config, text).await;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "telegram getUpdates failed");
                        // Back off on error.
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        })
    }

    /// Handle an incoming Telegram message by routing it through Vigil chat.
    async fn handle_message(&self, config: &TelegramConfig, text: &str) {
        // Send "typing" indicator so user knows we're processing.
        self.send_chat_action(&config.bot_token, &config.chat_id, "typing")
            .await
            .ok();

        // Route through the Vigil chat pipeline.
        match crate::api::vigil::process_chat(&self.deps, text, None).await {
            Ok(result) => {
                let response = if result.error {
                    format!("⚠️ {}", result.response)
                } else {
                    result.response
                };

                if let Err(e) =
                    self.send_message(&config.bot_token, &config.chat_id, &response).await
                {
                    tracing::error!(error = %e, "failed to send telegram response");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "vigil chat processing failed");
                let _ = self
                    .send_message(
                        &config.bot_token,
                        &config.chat_id,
                        &format!("⚠️ Error: {e}"),
                    )
                    .await;
            }
        }
    }

    /// Load the Telegram config from settings store.
    async fn load_config(&self) -> Option<TelegramConfig> {
        let store = SettingsStore::new(self.deps.db.clone());
        let raw = store.get("telegram").await.ok()??;
        serde_json::from_str(&raw).ok()
    }

    /// Call Telegram `getUpdates` with long polling.
    async fn get_updates(
        &self,
        bot_token: &str,
        offset: i64,
        timeout: u32,
    ) -> anyhow::Result<Vec<TelegramUpdate>> {
        let url = format!("https://api.telegram.org/bot{bot_token}/getUpdates");

        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "offset": offset,
                "timeout": timeout,
                "allowed_updates": ["message"],
            }))
            // HTTP timeout must be longer than the Telegram long-poll timeout.
            .timeout(std::time::Duration::from_secs(u64::from(timeout) + 10))
            .send()
            .await?;

        let body: TelegramResponse<Vec<TelegramUpdate>> = resp.json().await?;

        if !body.ok {
            anyhow::bail!(
                "Telegram API error: {}",
                body.description.unwrap_or_default()
            );
        }

        Ok(body.result.unwrap_or_default())
    }

    /// Send a text message via the Telegram Bot API.
    async fn send_message(
        &self,
        bot_token: &str,
        chat_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");

        // Telegram has a 4096 char limit per message. Split if needed.
        let chunks = split_message(text, 4000);
        for chunk in &chunks {
            let resp = self
                .http
                .post(&url)
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": chunk,
                    "parse_mode": "Markdown",
                    "disable_web_page_preview": true,
                }))
                .send()
                .await?;

            if !resp.status().is_success() {
                // Retry without Markdown parse mode in case formatting broke it.
                let resp2 = self
                    .http
                    .post(&url)
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "text": chunk,
                        "disable_web_page_preview": true,
                    }))
                    .send()
                    .await?;

                if !resp2.status().is_success() {
                    let body = resp2.text().await.unwrap_or_default();
                    anyhow::bail!("Telegram sendMessage error: {body}");
                }
            }
        }

        Ok(())
    }

    /// Send a chat action (e.g., "typing") indicator.
    async fn send_chat_action(
        &self,
        bot_token: &str,
        chat_id: &str,
        action: &str,
    ) -> anyhow::Result<()> {
        let url = format!("https://api.telegram.org/bot{bot_token}/sendChatAction");
        self.http
            .post(&url)
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "action": action,
            }))
            .send()
            .await?;
        Ok(())
    }
}

/// Split a message into chunks that fit within Telegram's character limit.
fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining);
            break;
        }
        // Try to split at a newline boundary.
        let split_at = remaining[..max_len]
            .rfind('\n')
            .unwrap_or(max_len);
        chunks.push(&remaining[..split_at]);
        remaining = remaining[split_at..].trim_start_matches('\n');
    }
    chunks
}

// ---------------------------------------------------------------------------
// Telegram API types (minimal subset)
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct TelegramResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Debug, serde::Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    text: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct TelegramChat {
    id: i64,
}
