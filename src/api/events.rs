//! Events ingestion route handler.
//!
//! Receives hook payloads from Claude Code agents, persists them to the
//! `events` table, and emits them on the internal event bus.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Json, State};
use axum::response::IntoResponse;
use serde_json::json;

use crate::deps::AppDeps;
use crate::error::{DbError, Result};
use crate::events::AppEvent;

/// Incoming hook payload from a Claude Code agent.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct HookPayload {
    pub session_id: String,
    pub data: serde_json::Value,
}

/// `POST /events` — ingest a hook event from a Claude Code agent.
///
/// This route is registered at the root level (not under `/api`) and is
/// not protected by authentication middleware.
pub(crate) async fn ingest_event(
    State(deps): State<AppDeps>,
    Json(payload): Json<HookPayload>,
) -> Result<impl IntoResponse> {
    let event_type = payload
        .data
        .get("hook_event_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_owned();

    let tool_name = payload
        .data
        .get("tool_name")
        .and_then(serde_json::Value::as_str)
        .map(String::from);

    let payload_text = serde_json::to_string(&payload.data).map_err(DbError::from)?;
    let timestamp = unix_ms();

    sqlx::query(
        "INSERT INTO events (session_id, event_type, tool_name, payload, timestamp) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&payload.session_id)
    .bind(&event_type)
    .bind(&tool_name)
    .bind(&payload_text)
    .bind(timestamp)
    .execute(deps.db.pool())
    .await
    .map_err(DbError::from)?;

    let _ = deps.event_bus.emit(AppEvent::HookEvent {
        session_id: payload.session_id,
        event_type,
        payload: Some(payload.data),
    });

    Ok(Json(json!({ "ok": true })))
}

/// Current time as Unix milliseconds.
fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use http_body_util::BodyExt;
    use serde_json::json;
    use tempfile::TempDir;
    use tower::ServiceExt;

    use crate::db::sqlite::SqliteDb;
    use crate::deps::AppDeps;

    use super::*;

    /// Build a minimal test app with only the `/events` route.
    async fn test_app() -> (Router, Arc<SqliteDb>, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = crate::config::Config::for_testing(dir.path());
        let deps = AppDeps::new(config).await.expect("test deps");
        let db = Arc::clone(&deps.db);

        let app = Router::new()
            .route("/events", post(ingest_event))
            .with_state(deps);

        (app, db, dir)
    }

    /// Send a POST /events request with the given JSON body.
    async fn post_event(app: Router, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method("POST")
            .uri("/events")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn ingest_event_persists_to_db() {
        let (app, db, _dir) = test_app().await;

        let body = json!({
            "session_id": "sess-123",
            "data": {
                "hook_event_name": "Stop",
                "reason": "completed"
            }
        });

        let (status, json) = post_event(app, body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json, json!({ "ok": true }));

        // Verify the event was persisted.
        let row = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, i64)>(
            "SELECT session_id, event_type, tool_name, payload, timestamp FROM events WHERE session_id = ?",
        )
        .bind("sess-123")
        .fetch_one(db.pool())
        .await
        .expect("event row not found");

        assert_eq!(row.0, "sess-123");
        assert_eq!(row.1, "Stop");
        assert!(row.2.is_none());
        assert!(row.3.is_some());
        assert!(row.4 > 0);

        // Verify the payload JSON is correct.
        let stored: serde_json::Value =
            serde_json::from_str(row.3.as_ref().unwrap()).expect("invalid JSON in payload");
        assert_eq!(stored["hook_event_name"], "Stop");
        assert_eq!(stored["reason"], "completed");
    }

    #[tokio::test]
    async fn ingest_event_extracts_event_type_and_tool_name() {
        let (app, db, _dir) = test_app().await;

        let body = json!({
            "session_id": "sess-456",
            "data": {
                "hook_event_name": "Notification",
                "tool_name": "Read",
                "message": "something happened"
            }
        });

        let (status, _) = post_event(app, body).await;
        assert_eq!(status, StatusCode::OK);

        let row = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT session_id, event_type, tool_name FROM events WHERE session_id = ?",
        )
        .bind("sess-456")
        .fetch_one(db.pool())
        .await
        .expect("event row not found");

        assert_eq!(row.0, "sess-456");
        assert_eq!(row.1, "Notification");
        assert_eq!(row.2, Some("Read".to_owned()));
    }

    #[tokio::test]
    async fn ingest_event_defaults_unknown_event_type() {
        let (app, db, _dir) = test_app().await;

        // No hook_event_name in data.
        let body = json!({
            "session_id": "sess-789",
            "data": {
                "some_field": "value"
            }
        });

        let (status, _) = post_event(app, body).await;
        assert_eq!(status, StatusCode::OK);

        let row =
            sqlx::query_as::<_, (String,)>("SELECT event_type FROM events WHERE session_id = ?")
                .bind("sess-789")
                .fetch_one(db.pool())
                .await
                .expect("event row not found");

        assert_eq!(row.0, "unknown");
    }
}
