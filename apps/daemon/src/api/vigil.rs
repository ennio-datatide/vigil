//! Vigil API route handlers.
//!
//! Provides endpoints for querying Vigil status, chatting with the
//! global overseer, retrieving a project acta (briefing), and browsing
//! chat history.

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

/// `POST /api/vigil/chat` — chat with the Vigil overseer.
///
/// The `project_path` field is optional. When provided, the Vigil for that
/// project is activated. The conversation is always persisted to the global
/// chat store regardless.
pub(crate) async fn chat(
    State(deps): State<AppDeps>,
    Json(input): Json<ChatInput>,
) -> Result<impl IntoResponse> {
    // Save user message.
    deps.vigil_chat_store
        .save_message("user", &input.message, None)
        .await?;

    // If project_path provided, ensure vigil is active for that project.
    if let Some(ref pp) = input.project_path {
        deps.vigil_service.ensure_vigil(pp).await;
    }

    // Placeholder response — actual LLM integration deferred.
    let response = "I received your message. (Vigil LLM integration pending)".to_owned();

    // Save vigil response.
    deps.vigil_chat_store
        .save_message("vigil", &response, None)
        .await?;

    Ok(Json(json!({
        "response": response,
    })))
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
            json["response"]
                .as_str()
                .unwrap()
                .contains("LLM integration pending"),
            "should return placeholder response"
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
            json["response"].as_str().unwrap().contains("LLM integration pending"),
            "should return placeholder response without project_path"
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
