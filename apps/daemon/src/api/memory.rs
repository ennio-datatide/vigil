//! Memory API route handlers.
//!
//! Implements CRUD and search endpoints for the Memory service,
//! backed by [`MemoryStore`] and [`MemorySearch`].

use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;

use crate::deps::AppDeps;
use crate::error::{Error, Result};
use crate::services::memory_store::CreateMemoryInput;

/// Maximum number of search results a client can request.
const MAX_SEARCH_LIMIT: usize = 100;

/// Query parameters for the list endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListQuery {
    /// Project path to filter memories by.
    pub project_path: String,
}

/// Request body for the search endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchInput {
    /// The search query string.
    pub query: String,
    /// Optional project path to scope the search.
    pub project_path: Option<String>,
    /// Maximum number of results (defaults to 10).
    pub limit: Option<usize>,
}

/// `GET /api/memory?projectPath=...` — list memories for a project.
pub(crate) async fn list_memories(
    State(deps): State<AppDeps>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse> {
    let memories = deps.memory_store.list(&params.project_path).await?;
    Ok(Json(memories))
}

/// `POST /api/memory/search` — hybrid search across memories.
pub(crate) async fn search_memories(
    State(deps): State<AppDeps>,
    Json(input): Json<SearchInput>,
) -> Result<impl IntoResponse> {
    if input.query.trim().is_empty() {
        return Err(Error::BadRequest("query must not be empty".into()));
    }
    let limit = input.limit.unwrap_or(10).min(MAX_SEARCH_LIMIT);
    let results = deps
        .memory_search
        .search(&input.query, input.project_path.as_deref(), limit)
        .await?;
    Ok(Json(results))
}

/// `POST /api/memory` — create a new memory.
pub(crate) async fn create_memory(
    State(deps): State<AppDeps>,
    Json(input): Json<CreateMemoryInput>,
) -> Result<impl IntoResponse> {
    if input.content.trim().is_empty() {
        return Err(Error::BadRequest("content must not be empty".into()));
    }
    let memory = deps.memory_store.create(&input).await?;
    Ok((StatusCode::CREATED, Json(memory)))
}

/// `DELETE /api/memory/{id}` — delete a memory by ID.
pub(crate) async fn delete_memory(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    deps.memory_store.delete(&id).await?;
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

    fn delete(uri: &str) -> Request<Body> {
        Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn create_memory_returns_201() {
        let (app, _dir) = test_app().await;

        let body = serde_json::json!({
            "content": "Rust uses ownership for memory safety",
            "memoryType": "fact",
            "projectPath": "/tmp/test-project",
            "sourceSessionId": "session-1"
        });

        let resp = app.oneshot(post_json("/api/memory", &body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let json = json_body(resp).await;
        assert_eq!(json["content"], "Rust uses ownership for memory safety");
        assert_eq!(json["memoryType"], "fact");
        assert_eq!(json["projectPath"], "/tmp/test-project");
        assert_eq!(json["sourceSessionId"], "session-1");
        assert!(json["id"].is_string());
    }

    #[tokio::test]
    async fn list_memories_by_project() {
        let (app, _dir) = test_app().await;

        // Create two memories in the same project.
        let body1 = serde_json::json!({
            "content": "First memory",
            "memoryType": "fact",
            "projectPath": "/tmp/alpha"
        });
        let body2 = serde_json::json!({
            "content": "Second memory",
            "memoryType": "decision",
            "projectPath": "/tmp/alpha"
        });
        // Create a memory in a different project.
        let body3 = serde_json::json!({
            "content": "Other project memory",
            "memoryType": "fact",
            "projectPath": "/tmp/beta"
        });

        let _ = app
            .clone()
            .oneshot(post_json("/api/memory", &body1))
            .await
            .unwrap();
        let _ = app
            .clone()
            .oneshot(post_json("/api/memory", &body2))
            .await
            .unwrap();
        let _ = app
            .clone()
            .oneshot(post_json("/api/memory", &body3))
            .await
            .unwrap();

        // List memories for /tmp/alpha.
        let resp = app
            .clone()
            .oneshot(get("/api/memory?projectPath=%2Ftmp%2Falpha"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = json_body(resp).await;
        let arr = json.as_array().expect("response should be array");
        assert_eq!(arr.len(), 2);

        // List memories for /tmp/beta.
        let resp = app
            .oneshot(get("/api/memory?projectPath=%2Ftmp%2Fbeta"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = json_body(resp).await;
        let arr = json.as_array().expect("response should be array");
        assert_eq!(arr.len(), 1);
    }

    #[tokio::test]
    async fn search_memories_returns_results() {
        let (app, _dir) = test_app().await;

        // Create a memory.
        let body = serde_json::json!({
            "content": "Rust ownership model prevents data races",
            "memoryType": "fact",
            "projectPath": "/tmp/search-test"
        });
        let _ = app
            .clone()
            .oneshot(post_json("/api/memory", &body))
            .await
            .unwrap();

        // Search for it.
        let search_body = serde_json::json!({
            "query": "Rust ownership data races",
            "projectPath": "/tmp/search-test",
            "limit": 5
        });
        let resp = app
            .oneshot(post_json("/api/memory/search", &search_body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        let arr = json.as_array().expect("response should be array");
        assert!(!arr.is_empty(), "search should return at least one result");
        // Each result should have memory and score.
        assert!(arr[0]["memory"].is_object());
        assert!(arr[0]["score"].is_number());
    }

    #[tokio::test]
    async fn delete_memory_returns_ok() {
        let (app, _dir) = test_app().await;

        // Create a memory.
        let body = serde_json::json!({
            "content": "Memory to delete",
            "memoryType": "fact",
            "projectPath": "/tmp/delete-test"
        });
        let resp = app
            .clone()
            .oneshot(post_json("/api/memory", &body))
            .await
            .unwrap();
        let json = json_body(resp).await;
        let id = json["id"].as_str().unwrap();

        // Delete it.
        let resp = app
            .clone()
            .oneshot(delete(&format!("/api/memory/{id}")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = json_body(resp).await;
        assert_eq!(json["ok"], true);

        // Verify it's gone by listing.
        let resp = app
            .oneshot(get("/api/memory?projectPath=%2Ftmp%2Fdelete-test"))
            .await
            .unwrap();
        let json = json_body(resp).await;
        let arr = json.as_array().unwrap();
        assert!(arr.is_empty());
    }

    #[tokio::test]
    async fn delete_nonexistent_returns_404() {
        let (app, _dir) = test_app().await;

        let resp = app
            .oneshot(delete("/api/memory/nonexistent-id"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
