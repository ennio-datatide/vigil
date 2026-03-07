//! Telegram settings route handlers.
//!
//! Implements GET, PUT, and POST (test) endpoints for managing
//! Telegram notification configuration.

use axum::extract::{Json, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::deps::AppDeps;
use crate::error::{Error, Result};
use crate::services::settings_store::SettingsStore;

/// Persisted Telegram configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
    pub dashboard_url: String,
    pub enabled: bool,
    pub events: Vec<String>,
}

/// Mask a bot token for safe display: first 4 + "..." + last 4.
fn mask_token(token: &str) -> String {
    if token.len() <= 8 {
        return "****".to_string();
    }
    format!("{}...{}", &token[..4], &token[token.len() - 4..])
}

/// `GET /api/settings/telegram` — retrieve Telegram configuration.
pub(crate) async fn get_telegram(State(deps): State<AppDeps>) -> Result<impl IntoResponse> {
    let store = SettingsStore::new(deps.db);

    let Some(raw) = store.get("telegram").await? else {
        return Ok(Json(json!({ "configured": false })));
    };

    let config: TelegramConfig =
        serde_json::from_str(&raw).map_err(|e| Error::Other(anyhow::anyhow!(e)))?;

    Ok(Json(json!({
        "configured": true,
        "botToken": mask_token(&config.bot_token),
        "chatId": config.chat_id,
        "dashboardUrl": config.dashboard_url,
        "enabled": config.enabled,
        "events": config.events,
    })))
}

/// `PUT /api/settings/telegram` — save Telegram configuration.
pub(crate) async fn put_telegram(
    State(deps): State<AppDeps>,
    Json(mut input): Json<TelegramConfig>,
) -> Result<impl IntoResponse> {
    let store = SettingsStore::new(deps.db);

    // If the incoming token looks masked, preserve the existing one from DB.
    if input.bot_token.contains("...")
        && let Some(existing_raw) = store.get("telegram").await?
    {
        let existing: TelegramConfig = serde_json::from_str(&existing_raw)
            .map_err(|e| Error::Other(anyhow::anyhow!(e)))?;
        input.bot_token = existing.bot_token;
    }

    let value = serde_json::to_string(&input).map_err(|e| Error::Other(anyhow::anyhow!(e)))?;
    store.set("telegram", &value).await?;

    Ok(Json(json!({ "ok": true })))
}

/// `POST /api/settings/telegram/test` — validate that Telegram is configured.
///
/// Actual message sending will be wired in Task 1.14.
pub(crate) async fn test_telegram(State(deps): State<AppDeps>) -> Result<impl IntoResponse> {
    let store = SettingsStore::new(deps.db);

    let Some(raw) = store.get("telegram").await? else {
        return Err(Error::BadRequest(
            "Telegram is not configured".to_string(),
        ));
    };

    let config: TelegramConfig =
        serde_json::from_str(&raw).map_err(|e| Error::Other(anyhow::anyhow!(e)))?;

    if !config.enabled {
        return Err(Error::BadRequest(
            "Telegram notifications are disabled".to_string(),
        ));
    }

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_token_short() {
        assert_eq!(mask_token("abcd"), "****");
        assert_eq!(mask_token("12345678"), "****");
    }

    #[test]
    fn mask_token_long() {
        assert_eq!(mask_token("123456789"), "1234...6789");
        assert_eq!(
            mask_token("bot000000000:XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"),
            "bot1...Dsaw"
        );
    }

    mod integration {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use http_body_util::BodyExt as _;
        use tower::ServiceExt as _;

        use crate::api;
        use crate::deps::AppDeps;

        async fn test_app() -> (axum::Router, tempfile::TempDir) {
            let dir = tempfile::TempDir::new().expect("temp dir");
            let config = crate::config::Config::for_testing(dir.path());
            let deps = AppDeps::new(config).await.expect("test deps");
            (api::router(deps), dir)
        }

        async fn json_body(resp: axum::response::Response) -> serde_json::Value {
            let bytes = resp.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice(&bytes).unwrap()
        }

        fn get(uri: &str) -> Request<Body> {
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .unwrap()
        }

        fn put_json(uri: &str, body: &serde_json::Value) -> Request<Body> {
            Request::builder()
                .method("PUT")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap()
        }

        fn post_empty(uri: &str) -> Request<Body> {
            Request::builder()
                .method("POST")
                .uri(uri)
                .body(Body::empty())
                .unwrap()
        }

        #[tokio::test]
        async fn get_telegram_not_configured() {
            let (app, _dir) = test_app().await;
            let resp = app.oneshot(get("/api/settings/telegram")).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = json_body(resp).await;
            assert_eq!(body["configured"], false);
        }

        #[tokio::test]
        async fn put_and_get_telegram_with_masking() {
            let (app, _dir) = test_app().await;

            let config = serde_json::json!({
                "botToken": "bot000000000:XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
                "chatId": "12345",
                "dashboardUrl": "http://localhost:3000",
                "enabled": true,
                "events": ["session_done", "error"]
            });

            // Save config.
            let resp = app
                .clone()
                .oneshot(put_json("/api/settings/telegram", &config))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = json_body(resp).await;
            assert_eq!(body["ok"], true);

            // Retrieve — token should be masked.
            let resp = app
                .oneshot(get("/api/settings/telegram"))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = json_body(resp).await;
            assert_eq!(body["configured"], true);
            assert_eq!(body["botToken"], "bot1...Dsaw");
            assert_eq!(body["chatId"], "12345");
            assert_eq!(body["enabled"], true);
        }

        #[tokio::test]
        async fn put_with_masked_token_preserves_original() {
            let (app, _dir) = test_app().await;

            // Save original config.
            let config = serde_json::json!({
                "botToken": "bot000000000:XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
                "chatId": "12345",
                "dashboardUrl": "http://localhost:3000",
                "enabled": true,
                "events": ["session_done"]
            });
            let resp = app
                .clone()
                .oneshot(put_json("/api/settings/telegram", &config))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);

            // Update with masked token — should preserve original.
            let update = serde_json::json!({
                "botToken": "bot1...Dsaw",
                "chatId": "99999",
                "dashboardUrl": "http://localhost:4000",
                "enabled": false,
                "events": ["error"]
            });
            let resp = app
                .clone()
                .oneshot(put_json("/api/settings/telegram", &update))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);

            // Verify the token was preserved (still masks the same way).
            let resp = app
                .oneshot(get("/api/settings/telegram"))
                .await
                .unwrap();
            let body = json_body(resp).await;
            assert_eq!(body["botToken"], "bot1...Dsaw");
            assert_eq!(body["chatId"], "99999");
            assert_eq!(body["enabled"], false);
        }

        #[tokio::test]
        async fn test_telegram_not_configured() {
            let (app, _dir) = test_app().await;
            let resp = app
                .oneshot(post_empty("/api/settings/telegram/test"))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn test_telegram_disabled() {
            let (app, _dir) = test_app().await;

            let config = serde_json::json!({
                "botToken": "bot123456:realtoken",
                "chatId": "12345",
                "dashboardUrl": "http://localhost:3000",
                "enabled": false,
                "events": []
            });
            let _ = app
                .clone()
                .oneshot(put_json("/api/settings/telegram", &config))
                .await
                .unwrap();

            let resp = app
                .oneshot(post_empty("/api/settings/telegram/test"))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn test_telegram_enabled_ok() {
            let (app, _dir) = test_app().await;

            let config = serde_json::json!({
                "botToken": "bot123456:realtoken",
                "chatId": "12345",
                "dashboardUrl": "http://localhost:3000",
                "enabled": true,
                "events": ["session_done"]
            });
            let _ = app
                .clone()
                .oneshot(put_json("/api/settings/telegram", &config))
                .await
                .unwrap();

            let resp = app
                .oneshot(post_empty("/api/settings/telegram/test"))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = json_body(resp).await;
            assert_eq!(body["ok"], true);
        }
    }
}
