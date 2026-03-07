//! Vigil API route handlers.
//!
//! Provides endpoints for querying Vigil status, chatting with the
//! project overseer, and retrieving the project acta (briefing).

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
#[allow(dead_code)] // `message` will be used when LLM integration is wired.
pub(crate) struct ChatInput {
    pub project_path: String,
    pub message: String,
}

/// `GET /api/vigil/status` — list all active Vigils and their status.
pub(crate) async fn get_status(
    State(deps): State<AppDeps>,
) -> Result<impl IntoResponse> {
    let active_projects = deps.vigil_service.active_projects().await;
    Ok(Json(StatusResponse { active_projects }))
}

/// `POST /api/vigil/chat` — chat with a project Vigil.
///
/// This is a placeholder. Actual LLM chat requires an API key and will
/// be wired when the Vigil agent is fully integrated.
pub(crate) async fn chat(
    State(deps): State<AppDeps>,
    Json(input): Json<ChatInput>,
) -> Result<impl IntoResponse> {
    // Ensure a vigil is active for this project.
    deps.vigil_service.ensure_vigil(&input.project_path).await;

    // Placeholder response — actual LLM integration deferred.
    Ok(Json(json!({
        "response": format!(
            "Vigil for {} received your message. LLM integration pending.",
            input.project_path,
        ),
        "projectPath": input.project_path,
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
    async fn chat_returns_placeholder() {
        let (app, _dir) = test_app().await;

        let body = serde_json::json!({
            "projectPath": "/tmp/chat-test",
            "message": "What is the project status?"
        });

        let resp = app.oneshot(post_json("/api/vigil/chat", &body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        assert_eq!(json["projectPath"], "/tmp/chat-test");
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
}
