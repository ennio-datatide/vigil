//! Vigil API route handlers.
//!
//! Provides endpoints for querying Vigil status, chatting with the
//! global overseer, retrieving a project acta (briefing), and browsing
//! chat history.

use std::fmt::Write as _;

use axum::extract::{Json, Query, State};
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
    pub session_id: Option<String>,
    pub hit_max_turns: bool,
    pub error: bool,
}

/// Core Vigil chat logic — shared between the HTTP handler and the Telegram poller.
///
/// Persists messages, builds context, invokes Claude CLI, and returns the result.
#[allow(clippy::too_many_lines)]
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

    // Load recent chat history for context (last 6 non-error messages).
    let all_messages = deps
        .vigil_chat_store
        .list_messages(100, 0)
        .await
        .unwrap_or_default();
    let recent: Vec<_> = all_messages
        .iter()
        .rev()
        .filter(|m| !is_error_message(m))
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    tracing::debug!(
        total_messages = all_messages.len(),
        context_messages = recent.len(),
        "loaded chat history for context"
    );

    // Build the conversation context from recent history.
    let mut context = String::new();
    for msg in &recent {
        let role = if msg.role == "user" { "Human" } else { "Vigil" };
        let _ = write!(context, "{role}: {}\n\n", msg.content);
    }

    // Build the full prompt — just the user message with minimal context.
    let prompt = if context.is_empty() {
        message.to_owned()
    } else {
        format!(
            "<conversation_history>\n{context}</conversation_history>\n\n{message}",
        )
    };

    tracing::debug!(prompt_len = prompt.len(), "built prompt with context");

    // Ensure Vigil config files exist.
    let vigil_dir = deps.config.praefectus_home.join("vigil");
    std::fs::create_dir_all(&vigil_dir).ok();

    let mcp_config_path = vigil_dir.join("mcp-config.json");
    let daemon_url = format!("http://localhost:{}", deps.config.server_port);
    if let Err(e) = crate::process::claude_cli::write_mcp_config(&mcp_config_path, &daemon_url) {
        tracing::error!(error = %e, "failed to write MCP config");
    }

    // Always write the strategy prompt so the compiled-in version wins
    // over any stale copy on disk.
    let strategy_path = vigil_dir.join("strategy.md");
    let strategy_content = include_str!("../../prompts/vigil-strategy.md");
    std::fs::write(&strategy_path, strategy_content).ok();

    tracing::info!("invoking claude CLI for vigil response...");

    // Serialise CLI calls — only one `claude -p` at a time.
    let _guard = deps.vigil_cli_mutex.lock().await;

    // Invoke Claude CLI.
    let result = match crate::process::claude_cli::invoke_vigil(
        &prompt,
        &strategy_path,
        &mcp_config_path,
        10,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "vigil claude CLI call failed");
            return Ok(ChatResult {
                response: format!("Vigil encountered an error: {e}"),
                session_id: None,
                hit_max_turns: false,
                error: true,
            });
        }
    };

    tracing::info!(
        response_len = result.response.len(),
        session_id = ?result.session_id,
        hit_max_turns = result.hit_max_turns,
        "claude CLI responded"
    );

    // Build response text — append session link if max turns was hit.
    let response = if result.hit_max_turns {
        let session_note = if let Some(ref sid) = result.session_id {
            format!(
                "\n\n---\n*Reached max turns limit. Resume this session:*\n```\nclaude --resume {sid}\n```",
            )
        } else {
            "\n\n---\n*Reached max turns limit. Consider increasing max turns or breaking the task into smaller steps.*".to_string()
        };

        if result.response.is_empty() {
            format!("Vigil ran out of turns before completing the response.{session_note}")
        } else {
            format!("{}{session_note}", result.response)
        }
    } else {
        result.response
    };

    // Save vigil response.
    deps.vigil_chat_store
        .save_message("vigil", &response, None)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    tracing::debug!("vigil response persisted");

    Ok(ChatResult {
        session_id: result.session_id,
        hit_max_turns: result.hit_max_turns,
        error: false,
        response,
    })
}

/// `POST /api/vigil/chat` — chat with the Vigil overseer.
///
/// The `project_path` field is optional. When provided, the Vigil for that
/// project is activated. The conversation is always persisted to the global
/// chat store regardless.
pub(crate) async fn chat(
    State(deps): State<AppDeps>,
    Json(input): Json<ChatInput>,
) -> Result<impl IntoResponse> {
    let result = process_chat(&deps, &input.message, input.project_path.as_deref())
        .await
        .map_err(crate::error::Error::Other)?;

    if result.error {
        return Ok(Json(json!({
            "response": result.response,
            "sessionId": serde_json::Value::Null,
            "hitMaxTurns": false,
            "error": true,
        })));
    }

    Ok(Json(json!({
        "response": result.response,
        "sessionId": result.session_id,
        "hitMaxTurns": result.hit_max_turns,
    })))
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

/// Returns `true` when the message looks like a Vigil error that should be
/// excluded from conversation context sent to Claude.
fn is_error_message(msg: &crate::services::vigil_chat::VigilMessage) -> bool {
    if msg.role != "vigil" {
        return false;
    }
    let c = &msg.content;
    c.starts_with("Vigil encountered an error:")
        || c.starts_with("Error:")
        || c.starts_with("Failed to reach Vigil")
        || c.contains("timed out after")
        || c.contains("Reached max turns")
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
    async fn chat_with_project_path_returns_response() {
        let (app, _dir) = test_app().await;

        let body = serde_json::json!({
            "projectPath": "/tmp/chat-test",
            "message": "What is the project status?"
        });

        let resp = app.oneshot(post_json("/api/vigil/chat", &body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        assert!(
            json["response"].as_str().is_some(),
            "should have a response string"
        );
    }

    #[tokio::test]
    async fn chat_activates_vigil() {
        let (app, _dir) = test_app().await;

        // Chat with a vigil to activate it.
        let body = serde_json::json!({
            "projectPath": "/tmp/activate-test",
            "message": "hello"
        });
        let _ = app
            .clone()
            .oneshot(post_json("/api/vigil/chat", &body))
            .await
            .unwrap();

        // Now status should show the project as active.
        let resp = app.oneshot(get("/api/vigil/status")).await.unwrap();
        let json = json_body(resp).await;
        let projects = json["activeProjects"].as_array().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0], "/tmp/activate-test");
    }

    #[tokio::test]
    async fn chat_without_project_path() {
        let (app, _dir) = test_app().await;

        let body = serde_json::json!({
            "message": "Hello Vigil"
        });

        let resp = app.oneshot(post_json("/api/vigil/chat", &body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        assert!(
            json["response"].as_str().is_some(),
            "should have a response string"
        );
    }

    #[tokio::test]
    async fn chat_history_persists() {
        let (app, _dir) = test_app().await;

        // Send a chat message.
        let body = serde_json::json!({ "message": "Hello from test" });
        let resp = app
            .clone()
            .oneshot(post_json("/api/vigil/chat", &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Retrieve history.
        let resp = app
            .oneshot(get("/api/vigil/history"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        let messages = json["messages"].as_array().expect("should be array");
        assert_eq!(messages.len(), 2, "should have user + vigil messages");
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello from test");
        assert_eq!(messages[1]["role"], "vigil");
    }

    #[tokio::test]
    async fn history_with_pagination() {
        let (app, _dir) = test_app().await;

        // Send two chat messages to create 4 total messages (2 user + 2 vigil).
        for msg in &["first", "second"] {
            let body = serde_json::json!({ "message": msg });
            let _ = app
                .clone()
                .oneshot(post_json("/api/vigil/chat", &body))
                .await
                .unwrap();
        }

        // Get with limit=2, offset=0.
        let resp = app
            .clone()
            .oneshot(get("/api/vigil/history?limit=2&offset=0"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = json_body(resp).await;
        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);

        // Get with limit=2, offset=2.
        let resp = app
            .oneshot(get("/api/vigil/history?limit=2&offset=2"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = json_body(resp).await;
        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
    }
}
