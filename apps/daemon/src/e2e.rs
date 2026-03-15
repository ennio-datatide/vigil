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
///   get status (empty) -> acta (null) -> update acta -> get acta (updated).
///
/// NOTE: Chat tests require a running Vigil PTY which is only available when
/// the daemon is running with a real `claude` binary. Chat-specific tests
/// live in `api::vigil::tests`.
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

    // -- Step 3: Update acta.
    let acta_body = json!({
        "projectPath": "/tmp/vigil-e2e",
        "content": "Project briefing for E2E test."
    });
    let req = Request::builder()
        .method("PUT")
        .uri("/api/vigil/acta")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&acta_body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // -- Step 4: Get acta — should be updated.
    let resp = app
        .clone()
        .oneshot(get("/api/vigil/acta?projectPath=%2Ftmp%2Fvigil-e2e"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["acta"], "Project briefing for E2E test.");

    // -- Step 5: Chat without PTY should return 500 (no Vigil running).
    let chat_body = json!({
        "message": "Hello Vigil"
    });
    let resp = app
        .oneshot(post_json("/api/vigil/chat", &chat_body))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "chat should fail without a running Vigil PTY"
    );
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
// Test 6: Session Output in API Response
// ---------------------------------------------------------------------------

/// Exercises the session output feature:
///   create session -> append output -> verify GET includes output
///   -> verify output persists to disk log -> remove buffer -> fallback to disk.
#[tokio::test]
async fn session_output_in_api() {
    let (app, deps, _dir) = test_app().await;

    // -- Step 1: Create a session directly via the store.
    create_running_session(&deps, "output-session-1").await;

    // -- Step 2: GET without any output — should not have output field.
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/output-session-1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["id"], "output-session-1");
    assert!(
        body.get("output").is_none() || body["output"].is_null(),
        "output should be absent or null when no data exists"
    );

    // -- Step 3: Append output to the session's buffer.
    deps.output_manager.ensure_buffer("output-session-1").await;
    deps.output_manager
        .append("output-session-1", b"Hello from worker!\nTask completed successfully.\n")
        .await;

    // -- Step 4: GET should now include the output.
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/output-session-1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    let output = body["output"].as_str().expect("output should be a string");
    assert!(
        output.contains("Hello from worker!"),
        "output should contain the appended data"
    );
    assert!(
        output.contains("Task completed successfully."),
        "output should contain all appended data"
    );

    // -- Step 5: Remove in-memory buffer; should fall back to disk log.
    deps.output_manager.remove("output-session-1").await;
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/output-session-1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    let output = body["output"]
        .as_str()
        .expect("output should still be available from disk log");
    assert!(
        output.contains("Hello from worker!"),
        "disk log fallback should contain the data"
    );
}

// ---------------------------------------------------------------------------
// Test 7: Vigil Chat History Persistence
// ---------------------------------------------------------------------------

/// Exercises chat history:
///   send message -> verify history -> send another -> verify ordering
///   -> verify pagination.
#[tokio::test]
async fn vigil_chat_history() {
    let (app, deps, _dir) = test_app().await;

    // -- Step 1: History should be empty initially.
    let resp = app
        .clone()
        .oneshot(get("/api/vigil/history"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    let messages = body["messages"].as_array().unwrap();
    assert!(messages.is_empty(), "history should be empty initially");

    // -- Step 2: Save a user message directly via the store.
    deps.vigil_chat_store
        .save_message("user", "first message", None)
        .await
        .unwrap();
    deps.vigil_chat_store
        .save_message("vigil", "first response", None)
        .await
        .unwrap();

    // -- Step 3: Verify history contains both messages in order.
    let resp = app
        .clone()
        .oneshot(get("/api/vigil/history"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "should have two messages");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "first message");
    assert_eq!(messages[1]["role"], "vigil");
    assert_eq!(messages[1]["content"], "first response");

    // -- Step 4: Add more messages and test pagination.
    deps.vigil_chat_store
        .save_message("user", "second message", None)
        .await
        .unwrap();
    deps.vigil_chat_store
        .save_message("vigil", "second response", None)
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(get("/api/vigil/history?limit=2&offset=0"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "limit=2 should return 2 messages");

    let resp = app
        .clone()
        .oneshot(get("/api/vigil/history?limit=2&offset=2"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "offset=2 limit=2 should return 2 messages");
}

// ---------------------------------------------------------------------------
// Test 8: Session Hard Delete and Cleanup
// ---------------------------------------------------------------------------

/// Exercises session cleanup:
///   create sessions -> hard-delete one -> verify removed -> list reflects it.
#[tokio::test]
async fn session_hard_delete() {
    let (app, deps, _dir) = test_app().await;

    // -- Step 1: Create two sessions.
    create_running_session(&deps, "del-session-1").await;
    create_running_session(&deps, "del-session-2").await;

    // -- Step 2: Verify both exist.
    let resp = app.clone().oneshot(get("/api/sessions")).await.unwrap();
    let body = json_body(resp).await;
    assert_eq!(body.as_array().unwrap().len(), 2);

    // -- Step 3: Hard-delete the first session.
    let resp = app
        .clone()
        .oneshot(delete("/api/sessions/del-session-1/remove"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["ok"], true);

    // -- Step 4: Verify it's gone.
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/del-session-1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // -- Step 5: Other session still exists.
    let resp = app
        .clone()
        .oneshot(get("/api/sessions/del-session-2"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // -- Step 6: List should have only one.
    let resp = app.oneshot(get("/api/sessions")).await.unwrap();
    let body = json_body(resp).await;
    assert_eq!(body.as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// Test 9: Vigil Acta Update and Retrieve
// ---------------------------------------------------------------------------

/// Exercises acta lifecycle:
///   update acta -> verify -> update again -> verify overwrite.
#[tokio::test]
async fn vigil_acta_lifecycle() {
    let (app, _deps, _dir) = test_app().await;

    // -- Step 1: Set acta for a project.
    let body = json!({
        "projectPath": "/tmp/acta-e2e",
        "content": "Project uses Rust + Axum. Main entry: src/lib.rs."
    });
    let resp = app
        .clone()
        .oneshot(put_json("/api/vigil/acta", &body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // -- Step 2: Retrieve and verify.
    let resp = app
        .clone()
        .oneshot(get("/api/vigil/acta?projectPath=%2Ftmp%2Facta-e2e"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["acta"], "Project uses Rust + Axum. Main entry: src/lib.rs.");

    // -- Step 3: Update with new content.
    let body = json!({
        "projectPath": "/tmp/acta-e2e",
        "content": "Updated: now includes WebSocket support."
    });
    let resp = app
        .clone()
        .oneshot(put_json("/api/vigil/acta", &body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // -- Step 4: Verify overwrite.
    let resp = app
        .oneshot(get("/api/vigil/acta?projectPath=%2Ftmp%2Facta-e2e"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    assert_eq!(body["acta"], "Updated: now includes WebSocket support.");
}

// ---------------------------------------------------------------------------
// Test 10: Settings Store (Telegram Config)
// ---------------------------------------------------------------------------

/// Exercises the settings store lifecycle:
///   get (unconfigured) -> save -> get (configured) -> update -> verify.
#[tokio::test]
async fn telegram_settings_lifecycle() {
    let (app, _deps, _dir) = test_app().await;

    // -- Step 1: Get telegram settings — should be unconfigured.
    let resp = app
        .clone()
        .oneshot(get("/api/settings/telegram"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["configured"], false);

    // -- Step 2: Save telegram settings.
    let settings_body = json!({
        "botToken": "123456:ABC",
        "chatId": "999",
        "dashboardUrl": "http://localhost:3000",
        "enabled": true,
        "events": ["session_complete", "blocker"]
    });
    let resp = app
        .clone()
        .oneshot(put_json("/api/settings/telegram", &settings_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // -- Step 3: Verify settings are saved.
    let resp = app
        .clone()
        .oneshot(get("/api/settings/telegram"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["configured"], true);
    assert_eq!(body["chatId"], "999");
    assert_eq!(body["enabled"], true);

    // -- Step 4: Update settings.
    let settings_body = json!({
        "botToken": "123456:ABC",
        "chatId": "888",
        "dashboardUrl": "http://localhost:3000",
        "enabled": false,
        "events": ["session_complete"]
    });
    let resp = app
        .clone()
        .oneshot(put_json("/api/settings/telegram", &settings_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // -- Step 5: Verify update.
    let resp = app
        .oneshot(get("/api/settings/telegram"))
        .await
        .unwrap();
    let body = json_body(resp).await;
    assert_eq!(body["chatId"], "888");
    assert_eq!(body["enabled"], false);
}

// ---------------------------------------------------------------------------
// Test 11: Cross-Cutting Concerns
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

// ---------------------------------------------------------------------------
// Test 12: Pipeline Execution Lifecycle
// ---------------------------------------------------------------------------

/// Exercises the pipeline execution REST API:
///   create pipeline -> list executions (empty) -> execute pipeline
///   -> get execution -> list executions (1).
#[tokio::test]
async fn pipeline_execution_lifecycle() {
    let (app, _deps, _dir) = test_app().await;

    // -- Step 1: Create a 2-step pipeline.
    let pipeline_body = json!({
        "name": "Exec Test Pipeline",
        "description": "E2E test pipeline for execution",
        "steps": [
            { "id": "s1", "label": "Step 1", "prompt": "Do step 1", "position": { "x": 0.0, "y": 0.0 } },
            { "id": "s2", "label": "Step 2", "prompt": "Do step 2", "position": { "x": 100.0, "y": 0.0 } }
        ],
        "edges": [
            { "id": "e1", "source": "s1", "target": "s2" }
        ],
        "isDefault": true
    });

    let resp = app
        .clone()
        .oneshot(post_json("/api/pipelines", &pipeline_body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let pipeline = json_body(resp).await;
    let pipeline_id = pipeline["id"].as_str().unwrap();

    // -- Step 2: List executions — should be empty.
    let resp = app
        .clone()
        .oneshot(get("/api/executions"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let executions = json_body(resp).await;
    let arr = executions.as_array().unwrap();
    assert!(arr.is_empty(), "no executions should exist initially");

    // -- Step 3: Execute the pipeline.
    //
    // The background runner will fail (no `claude` binary in test environment),
    // but the execution record should still be created and returned.
    let exec_body = json!({
        "projectPath": "/tmp/test-project",
        "prompt": "Add dark mode"
    });

    let resp = app
        .clone()
        .oneshot(post_json(
            &format!("/api/pipelines/{pipeline_id}/execute"),
            &exec_body,
        ))
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "execute should succeed, got {}",
        resp.status()
    );
    let execution = json_body(resp).await;
    let exec_id = execution["id"].as_str().unwrap();
    assert_eq!(execution["initialPrompt"].as_str(), Some("Add dark mode"));
    assert_eq!(execution["pipelineId"].as_str(), Some(pipeline_id));

    // -- Step 4: Get the execution by ID.
    let resp = app
        .clone()
        .oneshot(get(&format!("/api/executions/{exec_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let fetched = json_body(resp).await;
    assert_eq!(fetched["id"].as_str(), Some(exec_id));

    // -- Step 5: List executions — should have 1.
    let resp = app
        .clone()
        .oneshot(get("/api/executions"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let executions = json_body(resp).await;
    let arr = executions.as_array().unwrap();
    assert_eq!(arr.len(), 1, "one execution should exist after triggering");
}

// ---------------------------------------------------------------------------
// Test 13: Output Extraction Large Data
// ---------------------------------------------------------------------------

/// Exercises the output extraction utilities with large inputs:
///   strip ANSI from large data -> truncation -> plain text passthrough
///   -> empty input -> ANSI-only content -> context chain with truncation.
#[tokio::test]
async fn output_extraction_large_data() {
    use crate::process::output_extract::{build_context_chain, extract_response_text, strip_ansi};

    // -- Large output with ANSI codes.
    let mut large = Vec::new();
    for i in 0..10_000 {
        large.extend_from_slice(format!("\x1b[32mLine {i}\x1b[0m\n").as_bytes());
    }
    let extracted = extract_response_text(&large);
    assert!(
        !extracted.contains("\x1b["),
        "should strip all ANSI sequences"
    );
    assert!(
        extracted.len() <= 4000,
        "should truncate to 4000 chars, got {}",
        extracted.len()
    );

    // -- Plain text — no ANSI.
    let plain = b"Hello, this is a simple response.";
    let result = extract_response_text(plain);
    assert_eq!(result, "Hello, this is a simple response.");

    // -- Empty input.
    assert!(extract_response_text(b"").is_empty());

    // -- ANSI-only content.
    let ansi_only = b"\x1b[31m\x1b[0m\x1b[32m\x1b[0m";
    let result = strip_ansi(ansi_only);
    assert!(result.trim().is_empty(), "ANSI-only content should be empty after stripping");

    // -- Context chain with truncation.
    let labels: Vec<String> = vec!["A", "B", "C", "D", "E"]
        .into_iter()
        .map(String::from)
        .collect();
    let long_output = "X".repeat(3000);
    let outputs: Vec<String> = vec![&long_output; 5]
        .into_iter()
        .map(|s| s.clone())
        .collect();
    let chain = build_context_chain("user prompt", &labels, &outputs, "current step");
    assert!(chain.contains("<user_request>"), "should contain user request tag");
    assert!(chain.contains("<current_step>"), "should contain current step tag");
    // Oldest steps should have been truncated; most recent step (E) must be present.
    assert!(
        chain.contains("Step 5: E"),
        "most recent step should be kept after truncation"
    );
}
