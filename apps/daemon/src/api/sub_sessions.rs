//! Sub-session API route handlers.
//!
//! Provides endpoints for listing child sessions and spawning new
//! sub-sessions (branches or workers) from a running parent.

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::deps::AppDeps;
use crate::error::Result;
use crate::services::sub_session::SpawnInput;

/// `GET /api/sessions/:id/children` — list children of a session.
pub(crate) async fn list_children(
    State(deps): State<AppDeps>,
    Path(parent_id): Path<String>,
) -> Result<impl IntoResponse> {
    let children = deps.sub_session_service.list_children(&parent_id).await?;
    Ok(Json(children))
}

/// `POST /api/sessions/:id/spawn` — spawn a child session.
pub(crate) async fn spawn_child(
    State(deps): State<AppDeps>,
    Path(parent_id): Path<String>,
    Json(input): Json<SpawnInput>,
) -> Result<impl IntoResponse> {
    let child = deps
        .sub_session_service
        .spawn(&parent_id, &input)
        .await?;
    Ok((StatusCode::CREATED, Json(child)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    use crate::api;
    use crate::db::models::SessionStatus;
    use crate::deps::AppDeps;
    use crate::services::session_store::{CreateSessionInput, SessionStore};

    async fn test_app_with_deps() -> (axum::Router, AppDeps, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let config = crate::config::Config::for_testing(dir.path());
        let deps = AppDeps::new(config).await.expect("test deps");
        let router = api::router(deps.clone());
        (router, deps, dir)
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    fn post_json(uri: &str, body: &serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap()
    }

    /// Helper: create a running parent session directly via the store.
    async fn create_running_parent(deps: &AppDeps, id: &str) {
        let store = SessionStore::new(Arc::clone(&deps.db));
        let input = CreateSessionInput {
            project_path: "/tmp/test-project".into(),
            prompt: "parent task".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        };
        store.create(id, &input).await.unwrap();
        store
            .update_status(id, SessionStatus::Running, None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn list_children_empty() {
        let (app, deps, _dir) = test_app_with_deps().await;

        create_running_parent(&deps, "parent-1").await;

        let resp = app
            .oneshot(get("/api/sessions/parent-1/children"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        let arr = json.as_array().expect("response should be array");
        assert!(arr.is_empty());
    }

    #[tokio::test]
    async fn spawn_and_list_children() {
        let (app, deps, _dir) = test_app_with_deps().await;

        create_running_parent(&deps, "parent-1").await;

        // Spawn a branch child via the API.
        let spawn_body = serde_json::json!({
            "spawnType": "branch",
            "prompt": "research the architecture"
        });
        let resp = app
            .clone()
            .oneshot(post_json("/api/sessions/parent-1/spawn", &spawn_body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let child_json = json_body(resp).await;
        assert_eq!(child_json["parentId"], "parent-1");
        assert_eq!(child_json["spawnType"], "branch");
        assert_eq!(child_json["prompt"], "research the architecture");
        assert_eq!(child_json["status"], "queued");

        // List children and verify the spawned child appears.
        let resp = app
            .oneshot(get("/api/sessions/parent-1/children"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = json_body(resp).await;
        let arr = json.as_array().expect("response should be array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["parentId"], "parent-1");
    }

    #[tokio::test]
    async fn spawn_from_non_running_parent_fails() {
        let (app, deps, _dir) = test_app_with_deps().await;

        // Create a parent that stays queued (not running).
        let store = SessionStore::new(Arc::clone(&deps.db));
        let input = CreateSessionInput {
            project_path: "/tmp/test-project".into(),
            prompt: "queued parent".into(),
            skill: None,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        };
        store.create("queued-parent", &input).await.unwrap();

        let spawn_body = serde_json::json!({
            "spawnType": "branch",
            "prompt": "should fail"
        });
        let resp = app
            .oneshot(post_json("/api/sessions/queued-parent/spawn", &spawn_body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
