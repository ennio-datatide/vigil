//! End-to-end integration tests.
//!
//! Each test exercises multiple API endpoints in sequence to verify
//! full lifecycle flows through the entire stack.

#![cfg(test)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt as _;
use serde_json::json;
use tower::ServiceExt as _;

use crate::api;
use crate::db::models::SessionStatus;
use crate::deps::AppDeps;
use crate::services::session_store::{CreateSessionInput, SessionStore};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a fully-wired test app with an isolated database.
async fn test_app() -> (axum::Router, AppDeps, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let config = crate::config::Config::for_testing(dir.path());
    let deps = AppDeps::new(config).await.expect("test deps");
    let router = api::router(deps.clone());
    (router, deps, dir)
}

/// Extract a JSON body from an Axum response.
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

fn put_json(uri: &str, body: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("PUT")
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

fn patch(uri: &str) -> Request<Body> {
    Request::builder()
        .method("PATCH")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// Create a running parent session directly via the store (bypasses agent
/// spawning which requires a real Claude binary).
async fn create_running_session(deps: &AppDeps, id: &str) {
    let store = SessionStore::new(Arc::clone(&deps.db));
    let input = CreateSessionInput {
        project_path: "/tmp/e2e-project".into(),
        prompt: "e2e parent task".into(),
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

// ---------------------------------------------------------------------------
// Test 1: Full Session Lifecycle
// ---------------------------------------------------------------------------

/// Exercises the core session lifecycle:
///   create session -> verify listing -> send hook event -> check notification
///   -> verify session status update.
#[tokio::test]
async fn full_session_lifecycle() {
    let (app, deps, _dir) = test_app().await;

    // -- Step 1: List sessions — should be empty.
    let resp = app.clone().oneshot(get("/api/sessions")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "list sessions should return 200");
    let sessions = json_body(resp).await;
    let arr = sessions.as_array().expect("sessions should be array");
    assert!(arr.is_empty(), "no sessions should exist initially");

    // -- Step 2: Create a session directly via the store (avoids agent spawn).
    let store = SessionStore::new(Arc::clone(&deps.db));
    let input = CreateSessionInput {
        project_path: "/tmp/e2e-project".into(),
        prompt: "run the e2e test suite".into(),
        skill: None,
        role: None,
        parent_id: None,
        spawn_type: None,
        skip_permissions: None,
        pipeline_id: None,
    };
    let session = store.create("e2e-session-1", &input).await.unwrap();
    assert_eq!(session.status, SessionStatus::Queued);

    // -- Step 3: Verify session appears in GET /api/sessions.
    let resp = app.clone().oneshot(get("/api/sessions")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let sessions = json_body(resp).await;
    let arr = sessions.as_array().unwrap();
    assert_eq!(arr.len(), 1, "one session should exist after creation");
    assert_eq!(arr[0]["id"], "e2e-session-1");
    assert_eq!(arr[0]["status"], "queued");

    // -- Step 4: Verify GET /api/sessions/:id returns the session.
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/e2e-session-1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["id"], "e2e-session-1");
    assert_eq!(body["prompt"], "run the e2e test suite");

    // -- Step 5: Send a hook event for this session.
    let event_body = json!({
        "session_id": "e2e-session-1",
        "data": {
            "hook_event_name": "Stop",
            "stop_reason": "auth_required",
            "message": "needs permission"
        }
    });
    let resp = app
        .clone()
        .oneshot(post_json("/events", &event_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "event ingestion should succeed");
    let body = json_body(resp).await;
    assert_eq!(body["ok"], true);

    // -- Step 6: Verify the event was persisted to the database.
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT session_id, event_type FROM events WHERE session_id = ?",
    )
    .bind("e2e-session-1")
    .fetch_one(deps.db.pool())
    .await
    .expect("event should be persisted");
    assert_eq!(row.0, "e2e-session-1");
    assert_eq!(row.1, "Stop");

    // -- Step 7: Update session status to completed (simulating lifecycle end).
    store
        .update_status(
            "e2e-session-1",
            SessionStatus::Completed,
            Some(crate::db::models::ExitReason::Completed),
            Some(1_700_000_000_000),
        )
        .await
        .unwrap();

    // -- Step 8: Verify status change via the API.
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/e2e-session-1"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    assert_eq!(body["status"], "completed", "session should be completed");
    assert_eq!(body["exitReason"], "completed");

    // -- Step 9: Create a notification and verify it appears.
    let resp = app
        .clone()
        .oneshot(post_json("/api/notifications/test", &json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .clone()
        .oneshot(get("/api/notifications"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let notifications = json_body(resp).await;
    let arr = notifications.as_array().unwrap();
    assert!(!arr.is_empty(), "notification should exist after test creation");

    // -- Step 10: Mark notification as read and verify.
    let notif_id = arr[0]["id"].as_i64().unwrap();
    let resp = app
        .clone()
        .oneshot(patch(&format!("/api/notifications/{notif_id}/read")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert!(body["readAt"].is_number(), "readAt should be set after marking read");

    // -- Step 11: Filter notifications by unread — should be empty now.
    let resp = app
        .oneshot(get("/api/notifications?unread=true"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    let arr = body.as_array().unwrap();
    assert!(arr.is_empty(), "no unread notifications should remain");
}

// ---------------------------------------------------------------------------
// Test 2: Memory Lifecycle
// ---------------------------------------------------------------------------

/// Exercises the memory CRUD lifecycle:
///   create memory -> search -> list -> delete -> verify gone.
#[tokio::test]
async fn memory_lifecycle() {
    let (app, _deps, _dir) = test_app().await;

    // -- Step 1: Create a memory.
    let body = json!({
        "content": "The system uses event sourcing for session state",
        "memoryType": "fact",
        "projectPath": "/tmp/e2e-memory",
        "sourceSessionId": "session-mem-1"
    });
    let resp = app
        .clone()
        .oneshot(post_json("/api/memory", &body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "memory creation should return 201");
    let created = json_body(resp).await;
    let memory_id = created["id"].as_str().expect("id should be a string").to_owned();
    assert_eq!(created["content"], "The system uses event sourcing for session state");
    assert_eq!(created["memoryType"], "fact");
    assert_eq!(created["projectPath"], "/tmp/e2e-memory");

    // -- Step 2: Create a second memory for search diversity.
    let body2 = json!({
        "content": "Database migrations run automatically on startup",
        "memoryType": "decision",
        "projectPath": "/tmp/e2e-memory"
    });
    let resp = app
        .clone()
        .oneshot(post_json("/api/memory", &body2))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // -- Step 3: List memories for the project.
    let resp = app
        .clone()
        .oneshot(get("/api/memory?projectPath=%2Ftmp%2Fe2e-memory"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list = json_body(resp).await;
    let arr = list.as_array().unwrap();
    assert_eq!(arr.len(), 2, "two memories should exist for the project");

    // -- Step 4: Search for a memory.
    let search_body = json!({
        "query": "event sourcing session state",
        "projectPath": "/tmp/e2e-memory",
        "limit": 5
    });
    let resp = app
        .clone()
        .oneshot(post_json("/api/memory/search", &search_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let results = json_body(resp).await;
    let arr = results.as_array().unwrap();
    assert!(!arr.is_empty(), "search should return at least one result");
    assert!(arr[0]["memory"].is_object(), "each result should have a memory object");
    assert!(arr[0]["score"].is_number(), "each result should have a score");

    // -- Step 5: Delete the first memory.
    let resp = app
        .clone()
        .oneshot(delete(&format!("/api/memory/{memory_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["ok"], true);

    // -- Step 6: Verify only one memory remains.
    let resp = app
        .oneshot(get("/api/memory?projectPath=%2Ftmp%2Fe2e-memory"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1, "only one memory should remain after deletion");
}

// ---------------------------------------------------------------------------
// Test 3: Sub-Session Lifecycle
// ---------------------------------------------------------------------------

/// Exercises the sub-session workflow:
///   create parent -> set running -> spawn child -> list children -> verify.
#[tokio::test]
async fn sub_session_lifecycle() {
    let (app, deps, _dir) = test_app().await;

    // -- Step 1: Create a running parent session.
    create_running_session(&deps, "parent-e2e").await;

    // -- Step 2: List children — should be empty initially.
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/parent-e2e/children"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let children = json_body(resp).await;
    let arr = children.as_array().unwrap();
    assert!(arr.is_empty(), "no children should exist initially");

    // -- Step 3: Spawn a branch child.
    let spawn_body = json!({
        "spawnType": "branch",
        "prompt": "research the architecture for e2e test"
    });
    let resp = app
        .clone()
        .oneshot(post_json("/api/sessions/parent-e2e/spawn", &spawn_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "spawn should return 201");
    let child = json_body(resp).await;
    assert_eq!(child["parentId"], "parent-e2e");
    assert_eq!(child["spawnType"], "branch");
    assert_eq!(child["prompt"], "research the architecture for e2e test");
    assert_eq!(child["status"], "queued");
    let child_id = child["id"].as_str().unwrap().to_owned();

    // -- Step 4: Spawn a worker child.
    let spawn_body = json!({
        "spawnType": "worker",
        "prompt": "implement feature X"
    });
    let resp = app
        .clone()
        .oneshot(post_json("/api/sessions/parent-e2e/spawn", &spawn_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let worker = json_body(resp).await;
    assert_eq!(worker["spawnType"], "worker");

    // -- Step 5: List children — should now have two.
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/parent-e2e/children"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let children = json_body(resp).await;
    let arr = children.as_array().unwrap();
    assert_eq!(arr.len(), 2, "two children should exist after spawning");

    // -- Step 6: Verify child appears in the main sessions list.
    let resp = app
        .clone()
        .oneshot(get(&format!("/api/sessions/{child_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["parentId"], "parent-e2e");

    // -- Step 7: Verify spawning from a non-running parent fails.
    let store = SessionStore::new(Arc::clone(&deps.db));
    store
        .update_status(
            "parent-e2e",
            SessionStatus::Completed,
            Some(crate::db::models::ExitReason::Completed),
            Some(1_700_000_000_000),
        )
        .await
        .unwrap();

    let spawn_body = json!({
        "spawnType": "branch",
        "prompt": "should fail because parent is completed"
    });
    let resp = app
        .oneshot(post_json("/api/sessions/parent-e2e/spawn", &spawn_body))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "spawn from completed parent should fail"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Vigil Lifecycle
// ---------------------------------------------------------------------------

/// Exercises the Vigil overseer lifecycle:
///   get status (empty) -> chat (activates) -> get status (active) -> get acta.
#[tokio::test]
async fn vigil_lifecycle() {
    let (app, _deps, _dir) = test_app().await;

    // -- Step 1: Get vigil status — should be empty.
    let resp = app
        .clone()
        .oneshot(get("/api/vigil/status"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    let projects = body["activeProjects"].as_array().unwrap();
    assert!(projects.is_empty(), "no vigils should be active initially");

    // -- Step 2: Get acta for unknown project — should be null.
    let resp = app
        .clone()
        .oneshot(get("/api/vigil/acta?projectPath=%2Ftmp%2Fvigil-e2e"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["projectPath"], "/tmp/vigil-e2e");
    assert!(body["acta"].is_null(), "acta should be null for unknown project");

    // -- Step 3: Chat with a vigil — should activate it.
    let chat_body = json!({
        "projectPath": "/tmp/vigil-e2e",
        "message": "What is the current project status?"
    });
    let resp = app
        .clone()
        .oneshot(post_json("/api/vigil/chat", &chat_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert!(
        body["response"].as_str().unwrap().contains("LLM integration pending"),
        "should return placeholder response"
    );

    // -- Step 4: Get vigil status — should show the project as active.
    let resp = app
        .clone()
        .oneshot(get("/api/vigil/status"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    let projects = body["activeProjects"].as_array().unwrap();
    assert_eq!(projects.len(), 1, "one vigil should be active");
    assert_eq!(projects[0], "/tmp/vigil-e2e");

    // -- Step 5: Chat with a second project to verify multi-project support.
    let chat_body = json!({
        "projectPath": "/tmp/vigil-e2e-2",
        "message": "hello second project"
    });
    let resp = app
        .clone()
        .oneshot(post_json("/api/vigil/chat", &chat_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(get("/api/vigil/status"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    let projects = body["activeProjects"].as_array().unwrap();
    assert_eq!(projects.len(), 2, "two vigils should be active");
}

// ---------------------------------------------------------------------------
// Test 5: Pipeline Lifecycle
// ---------------------------------------------------------------------------

/// Exercises the full pipeline CRUD lifecycle:
///   create -> get -> list -> update -> verify update -> delete -> verify gone.
#[tokio::test]
async fn pipeline_lifecycle() {
    let (app, _deps, _dir) = test_app().await;

    // -- Step 1: List pipelines — should be empty.
    let resp = app
        .clone()
        .oneshot(get("/api/pipelines"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    let arr = body.as_array().unwrap();
    assert!(arr.is_empty(), "no pipelines should exist initially");

    // -- Step 2: Create a pipeline.
    let create_body = json!({
        "name": "E2E Test Pipeline",
        "description": "A pipeline created during e2e testing",
        "steps": [
            {
                "id": "step-1",
                "label": "Research",
                "prompt": "Research the topic",
                "skill": null,
                "position": { "x": 0.0, "y": 0.0 }
            },
            {
                "id": "step-2",
                "label": "Implement",
                "prompt": "Implement the solution",
                "skill": "tdd",
                "position": { "x": 200.0, "y": 0.0 }
            }
        ],
        "edges": [
            { "id": "edge-1", "source": "step-1", "target": "step-2" }
        ],
        "isDefault": false
    });
    let resp = app
        .clone()
        .oneshot(post_json("/api/pipelines", &create_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "pipeline creation should return 201");
    let created = json_body(resp).await;
    let pipeline_id = created["id"].as_str().unwrap().to_owned();
    assert_eq!(created["name"], "E2E Test Pipeline");
    assert_eq!(created["description"], "A pipeline created during e2e testing");
    let steps = created["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    let edges = created["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1);

    // -- Step 3: Get the pipeline by ID.
    let resp = app
        .clone()
        .oneshot(get(&format!("/api/pipelines/{pipeline_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["id"], pipeline_id);
    assert_eq!(body["name"], "E2E Test Pipeline");

    // -- Step 4: List pipelines — should have one.
    let resp = app
        .clone()
        .oneshot(get("/api/pipelines"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1, "one pipeline should exist after creation");

    // -- Step 5: Update the pipeline.
    let update_body = json!({
        "name": "Updated E2E Pipeline",
        "description": "Updated description"
    });
    let resp = app
        .clone()
        .oneshot(put_json(&format!("/api/pipelines/{pipeline_id}"), &update_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = json_body(resp).await;
    assert_eq!(updated["name"], "Updated E2E Pipeline");
    assert_eq!(updated["description"], "Updated description");
    // Steps and edges should be unchanged.
    let steps = updated["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);

    // -- Step 6: Delete the pipeline.
    let resp = app
        .clone()
        .oneshot(delete(&format!("/api/pipelines/{pipeline_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["ok"], true);

    // -- Step 7: Verify it's gone.
    let resp = app
        .clone()
        .oneshot(get(&format!("/api/pipelines/{pipeline_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "deleted pipeline should return 404");

    // -- Step 8: List should be empty again.
    let resp = app
        .oneshot(get("/api/pipelines"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    let arr = body.as_array().unwrap();
    assert!(arr.is_empty(), "no pipelines should remain after deletion");
}

// ---------------------------------------------------------------------------
// Test 6: Cross-Cutting Concerns
// ---------------------------------------------------------------------------

/// Exercises cross-cutting behaviors: 404 for missing resources, health check,
/// and notification mark-all-read.
#[tokio::test]
async fn cross_cutting_concerns() {
    let (app, _deps, _dir) = test_app().await;

    // -- Health check.
    let resp = app.clone().oneshot(get("/health")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "health check should return 200");

    // -- 404 for non-existent session.
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/does-not-exist"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // -- 404 for non-existent notification.
    let resp = app
        .clone()
        .oneshot(patch("/api/notifications/99999/read"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // -- Create multiple test notifications and mark all read.
    let _ = app
        .clone()
        .oneshot(post_json("/api/notifications/test", &json!({})))
        .await
        .unwrap();
    let _ = app
        .clone()
        .oneshot(post_json("/api/notifications/test", &json!({})))
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(get("/api/notifications?unread=true"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 2, "two unread notifications should exist");

    let resp = app
        .clone()
        .oneshot(patch("/api/notifications/read-all"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["updated"], 2, "two notifications should be marked read");

    let resp = app
        .oneshot(get("/api/notifications?unread=true"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    let arr = body.as_array().unwrap();
    assert!(arr.is_empty(), "no unread notifications should remain after mark-all-read");
}
