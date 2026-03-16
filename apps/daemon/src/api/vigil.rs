//! Vigil API route handlers.
//!
//! Provides endpoints for querying Vigil status, chatting with the
//! global overseer, retrieving a project acta (briefing), and browsing
//! chat history.

use axum::extract::{Json, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::deps::AppDeps;
use crate::error::Result;

/// Response for `GET /api/vigil/status`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    active_projects: Vec<String>,
}

/// Query parameters for `GET /api/vigil/acta`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActaQuery {
    pub project_path: String,
}

/// Request body for `POST /api/vigil/chat`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatInput {
    pub message: String,
    pub project_path: Option<String>,
}

/// Query parameters for `GET /api/vigil/history`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    100
}

/// `GET /api/vigil/status` — list all active Vigils and their status.
pub(crate) async fn get_status(
    State(deps): State<AppDeps>,
) -> Result<impl IntoResponse> {
    let active_projects = deps.vigil_service.active_projects().await;
    Ok(Json(StatusResponse { active_projects }))
}

/// Result of processing a Vigil chat message.
pub(crate) struct ChatResult {
    pub response: String,
}

/// Check if any worker session is in `needs_input` state. If so, send the
/// user's message to that worker's PTY and wait for it to complete or ask
/// another question. Returns `None` if no worker is waiting.
async fn try_reply_to_waiting_worker(
    deps: &AppDeps,
    message: &str,
) -> Option<anyhow::Result<String>> {
    use crate::db::models::SessionStatus;
    use crate::services::session_store::SessionStore;

    let store = SessionStore::new(std::sync::Arc::clone(&deps.db));

    // Find the most recent session that needs input (not the vigil session).
    let Ok(sessions) = store.list().await else {
        return None;
    };

    let waiting = sessions
        .iter()
        .filter(|s| {
            s.id != "vigil"
                && (s.status == SessionStatus::NeedsInput
                    || s.status == SessionStatus::AuthRequired)
        })
        .max_by_key(|s| s.started_at)?;

    let sid = waiting.id.clone();
    tracing::info!(
        session_id = %sid,
        "routing user message to waiting worker instead of Vigil"
    );

    // Send input to the worker's PTY.
    let data = format!("{message}\r");
    if let Err(e) = deps.pty_manager.write(&sid, data.into_bytes()).await {
        return Some(Err(anyhow::anyhow!("failed to write to worker: {e}")));
    }

    // Update status back to running.
    let _ = store
        .update_status(&sid, SessionStatus::Running, None, None)
        .await;

    // Wait for the worker's Stop hook event which contains the clean
    // response text in `last_assistant_message`. This is more reliable
    // than parsing raw PTY output.
    let mut rx = deps.event_bus.subscribe();
    let max_wait = std::time::Duration::from_secs(600);

    let result = tokio::time::timeout(max_wait, async {
        while let Ok(event) = rx.recv().await {
            if let crate::events::AppEvent::HookEvent {
                session_id: ev_sid,
                event_type,
                payload,
            } = &event
                && ev_sid == &sid
                && event_type == "Stop"
            {
                let response = payload
                    .as_ref()
                    .and_then(|p| p.get("last_assistant_message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return response;
            }
        }
        String::new()
    })
    .await;

    match result {
        Ok(response) if !response.is_empty() => Some(Ok(response)),
        Ok(_) => Some(Ok("Worker finished but produced no response.".to_string())),
        Err(_) => Some(Ok("Worker is still running. Check the session monitor.".to_string())),
    }
}

/// Core Vigil chat logic — shared between the HTTP handler and the Telegram poller.
///
/// Persists the user message, sends it to the persistent Vigil PTY via
/// [`VigilManager::send_message()`], and persists the response.
pub(crate) async fn process_chat(
    deps: &AppDeps,
    message: &str,
    project_path: Option<&str>,
) -> anyhow::Result<ChatResult> {
    tracing::info!(
        message_len = message.len(),
        project_path = ?project_path,
        "vigil chat request received"
    );

    // Save user message.
    deps.vigil_chat_store
        .save_message("user", message, None)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    tracing::debug!("user message persisted");

    // If project_path provided, ensure vigil is active for that project.
    if let Some(pp) = project_path {
        deps.vigil_service.ensure_vigil(pp).await;
        tracing::debug!(project_path = %pp, "vigil activated for project");
    }

    // Check if any worker session is waiting for input. If so, route the
    // user's message directly to that worker instead of going through Vigil.
    // This ensures multi-turn conversations stay on the same worker session.
    if let Some(result) = try_reply_to_waiting_worker(deps, message).await {
        let response = result?;
        deps.vigil_chat_store
            .save_message("vigil", &response, None)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(ChatResult { response });
    }

    // Send to Vigil via persistent PTY.
    let response = match deps.vigil_manager.send_message(message).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "vigil send_message failed");
            return Err(e);
        }
    };

    tracing::info!(response_len = response.len(), "vigil responded");

    // Save vigil response.
    deps.vigil_chat_store
        .save_message("vigil", &response, None)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    tracing::debug!("vigil response persisted");

    Ok(ChatResult { response })
}

/// `POST /api/vigil/chat` — chat with the Vigil overseer.
///
/// The `project_path` field is optional. When provided, the Vigil for that
/// project is activated. The conversation is always persisted to the global
/// chat store regardless.
pub(crate) async fn chat(
    State(deps): State<AppDeps>,
    Json(input): Json<ChatInput>,
) -> impl IntoResponse {
    match process_chat(&deps, &input.message, input.project_path.as_deref()).await {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({
                "response": result.response,
                "sessionId": serde_json::Value::Null,
                "hitMaxTurns": false,
            })),
        )
            .into_response(),
        Err(e) if e.to_string().contains("processing another message") => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "response": format!("Vigil encountered an error: {e}"),
                "error": true,
            })),
        )
            .into_response(),
    }
}

/// Request body for `PUT /api/vigil/acta`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateActaInput {
    pub project_path: String,
    pub content: String,
}

/// `PUT /api/vigil/acta` — update the acta for a project.
pub(crate) async fn update_acta(
    State(deps): State<AppDeps>,
    Json(input): Json<UpdateActaInput>,
) -> Result<impl IntoResponse> {
    deps.vigil_service
        .update_acta(&input.project_path, &input.content)
        .await?;

    Ok(Json(json!({ "ok": true })))
}

/// `GET /api/vigil/acta?projectPath=...` — get the acta for a project.
pub(crate) async fn get_acta(
    State(deps): State<AppDeps>,
    Query(params): Query<ActaQuery>,
) -> Result<impl IntoResponse> {
    let acta = deps.vigil_service.get_acta(&params.project_path).await;
    Ok(Json(json!({
        "projectPath": params.project_path,
        "acta": acta,
    })))
}

/// `GET /api/vigil/history` — retrieve paginated chat history.
pub(crate) async fn get_history(
    State(deps): State<AppDeps>,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse> {
    let messages = deps
        .vigil_chat_store
        .list_messages(query.limit, query.offset)
        .await?;
    Ok(Json(json!({ "messages": messages })))
}

/// `DELETE /api/vigil/history` — clear all chat history.
pub(crate) async fn clear_history(State(deps): State<AppDeps>) -> Result<impl IntoResponse> {
    deps.vigil_chat_store.clear().await?;
    Ok(Json(json!({ "ok": true })))
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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

    fn post_json(uri: &str, body: &serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap()
    }

    #[tokio::test]
    async fn get_status_returns_empty() {
        let (app, _dir) = test_app().await;

        let resp = app.oneshot(get("/api/vigil/status")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        let projects = json["activeProjects"].as_array().expect("should be array");
        assert!(projects.is_empty(), "no vigils should be active initially");
    }

    #[tokio::test]
    async fn update_and_get_acta() {
        let (app, _dir) = test_app().await;

        // Update acta.
        let body = serde_json::json!({
            "projectPath": "/tmp/acta-test",
            "content": "This is the project briefing."
        });
        let req = Request::builder()
            .method("PUT")
            .uri("/api/vigil/acta")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Get acta should return the content.
        let resp = app
            .oneshot(get("/api/vigil/acta?projectPath=%2Ftmp%2Facta-test"))
            .await
            .unwrap();
        let json = json_body(resp).await;
        assert_eq!(json["acta"], "This is the project briefing.");
    }

    #[tokio::test]
    async fn get_acta_returns_null_for_unknown() {
        let (app, _dir) = test_app().await;

        let resp = app
            .oneshot(get("/api/vigil/acta?projectPath=%2Funknown"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        assert_eq!(json["projectPath"], "/unknown");
        assert!(json["acta"].is_null(), "acta should be null for unknown project");
    }

    #[tokio::test]
    async fn chat_returns_error_when_vigil_pty_not_started() {
        let (app, _dir) = test_app().await;

        // Without a running Vigil PTY, chat should return 500.
        let body = serde_json::json!({
            "message": "Hello Vigil"
        });

        let resp = app.oneshot(post_json("/api/vigil/chat", &body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let json = json_body(resp).await;
        assert!(json["error"].as_bool().unwrap_or(false));
    }

    #[tokio::test]
    async fn chat_persists_user_message_even_on_error() {
        let (app, _dir) = test_app().await;

        // Chat will fail (no PTY), but user message should be persisted.
        let body = serde_json::json!({ "message": "Hello from test" });
        let _ = app
            .clone()
            .oneshot(post_json("/api/vigil/chat", &body))
            .await
            .unwrap();

        // Retrieve history — should have at least the user message.
        let resp = app
            .oneshot(get("/api/vigil/history"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        let messages = json["messages"].as_array().expect("should be array");
        assert!(
            !messages.is_empty(),
            "user message should be persisted even when Vigil PTY is unavailable"
        );
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello from test");
    }

    #[tokio::test]
    async fn history_pagination_works() {
        let (app, _dir) = test_app().await;

        // Directly save messages via the store to test pagination without PTY.
        let dir = tempfile::TempDir::new().unwrap();
        let config = crate::config::Config::for_testing(dir.path());
        let deps = AppDeps::new(config).await.unwrap();

        for i in 0..4 {
            deps.vigil_chat_store
                .save_message("user", &format!("msg {i}"), None)
                .await
                .unwrap();
        }

        let router = api::router(deps);

        // Get with limit=2, offset=0.
        let resp = router
            .clone()
            .oneshot(get("/api/vigil/history?limit=2&offset=0"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = json_body(resp).await;
        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);

        // Get with limit=2, offset=2.
        let resp = router
            .oneshot(get("/api/vigil/history?limit=2&offset=2"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = json_body(resp).await;
        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
    }
}
