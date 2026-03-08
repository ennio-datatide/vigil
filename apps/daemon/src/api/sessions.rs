//! Session CRUD route handlers.
//!
//! Implements the REST endpoints for session management:
//! list, get, create, cancel, hard-delete, restart, and resume.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;

use crate::db::models::{ExitReason, SessionStatus};
use crate::deps::AppDeps;
use crate::error::{Error, Result};
use crate::events::AppEvent;
use crate::process::agent_spawner::AgentSpawner;
use crate::services::session_store::{is_terminal_status, CreateSessionInput, SessionStore};

/// `GET /api/sessions` — list all sessions, most recent first.
pub(crate) async fn list_sessions(State(deps): State<AppDeps>) -> Result<impl IntoResponse> {
    let store = SessionStore::new(deps.db);
    let sessions = store.list().await?;
    Ok(Json(sessions))
}

/// `GET /api/sessions/:id` — get a single session by ID.
pub(crate) async fn get_session(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let store = SessionStore::new(deps.db);
    let session = store
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("session {id}")))?;
    Ok(Json(session))
}

/// `POST /api/sessions` — create a new session and spawn the agent.
pub(crate) async fn create_session(
    State(deps): State<AppDeps>,
    Json(input): Json<CreateSessionInput>,
) -> Result<impl IntoResponse> {
    let id = uuid::Uuid::new_v4().to_string();
    let store = SessionStore::new(deps.db.clone());
    let session = store.create(&id, &input).await?;

    let _ = deps.event_bus.emit(AppEvent::SessionUpdate {
        session: session.clone(),
    });

    // Spawn agent in background (don't block the response).
    let deps_clone = deps.clone();
    let session_clone = session.clone();
    tokio::spawn(async move {
        let spawner = AgentSpawner::new(&deps_clone);
        if let Err(e) = spawner.spawn_interactive(&session_clone, false).await {
            tracing::error!(session_id = session_clone.id, error = %e, "agent spawn failed");
            let store = SessionStore::new(deps_clone.db.clone());
            let _ = store
                .update_status(
                    &session_clone.id,
                    SessionStatus::Failed,
                    Some(ExitReason::Error),
                    Some(unix_ms()),
                )
                .await;
            let _ = deps_clone.event_bus.emit(AppEvent::SessionSpawnFailed {
                session_id: session_clone.id.clone(),
                reason: e.to_string(),
            });
        }
    });

    Ok((StatusCode::CREATED, Json(session)))
}

/// `DELETE /api/sessions/:id` — cancel (soft-delete) a session.
pub(crate) async fn cancel_session(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    // Kill the running agent before updating status.
    deps.pty_manager.kill(&id).await;

    let store = SessionStore::new(deps.db.clone());
    let now_ms = unix_ms();

    let session = store
        .update_status(
            &id,
            SessionStatus::Cancelled,
            Some(crate::db::models::ExitReason::UserCancelled),
            Some(now_ms),
        )
        .await?;

    let _ = deps.event_bus.emit(AppEvent::SessionUpdate {
        session: session.clone(),
    });

    Ok(Json(session))
}

/// `DELETE /api/sessions/:id/remove` — hard-delete a session from the database.
pub(crate) async fn remove_session(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    // Kill the running agent (if any) before deleting.
    deps.pty_manager.kill(&id).await;

    let store = SessionStore::new(deps.db.clone());
    store.delete(&id).await?;

    let _ = deps.event_bus.emit(AppEvent::SessionRemoved {
        session_id: id,
    });

    Ok(Json(json!({ "ok": true })))
}

/// `POST /api/sessions/:id/restart` — reset a terminal session to queued.
pub(crate) async fn restart_session(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let store = SessionStore::new(deps.db.clone());

    // Validate session exists and is in a terminal state.
    let existing = store
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("session {id}")))?;

    if !is_terminal_status(&existing.status) {
        return Err(Error::BadRequest(format!(
            "cannot restart session in '{}' state",
            status_display(&existing.status),
        )));
    }

    if existing.worktree_path.is_none() {
        return Err(Error::BadRequest(
            "No worktree path — cannot restart".into(),
        ));
    }

    let session = store.reset_to_queued(&id).await?;

    let _ = deps.event_bus.emit(AppEvent::SessionUpdate {
        session: session.clone(),
    });

    // Spawn agent with --continue in background.
    let deps_clone = deps.clone();
    let session_clone = session.clone();
    tokio::spawn(async move {
        let spawner = AgentSpawner::new(&deps_clone);
        if let Err(e) = spawner.spawn_interactive(&session_clone, true).await {
            tracing::error!(session_id = session_clone.id, error = %e, "agent restart failed");
            let store = SessionStore::new(deps_clone.db.clone());
            let _ = store
                .update_status(
                    &session_clone.id,
                    SessionStatus::Failed,
                    Some(ExitReason::Error),
                    Some(unix_ms()),
                )
                .await;
            let _ = deps_clone.event_bus.emit(AppEvent::SessionSpawnFailed {
                session_id: session_clone.id.clone(),
                reason: e.to_string(),
            });
        }
    });

    Ok(Json(session))
}

/// `POST /api/sessions/:id/resume` — create a new session based on an existing one.
pub(crate) async fn resume_session(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let store = SessionStore::new(deps.db.clone());

    // Validate source session exists and is terminal.
    let original = store
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("session {id}")))?;

    if !is_terminal_status(&original.status) {
        return Err(Error::BadRequest(format!(
            "cannot resume session in '{}' state",
            status_display(&original.status),
        )));
    }

    if original.worktree_path.is_none() {
        return Err(Error::BadRequest(
            "No worktree path found for session — cannot resume".into(),
        ));
    }

    let new_id = uuid::Uuid::new_v4().to_string();
    let input = CreateSessionInput {
        project_path: original.project_path,
        prompt: "Resumed conversation".into(),
        skill: None,
        role: None,
        parent_id: Some(id.clone()),
        spawn_type: None,
        skip_permissions: None,
        pipeline_id: None,
    };

    let session = store.create(&new_id, &input).await?;

    let _ = deps.event_bus.emit(AppEvent::SessionUpdate {
        session: session.clone(),
    });

    // Spawn agent with --continue in background.
    let deps_clone = deps.clone();
    let session_clone = session.clone();
    tokio::spawn(async move {
        let spawner = AgentSpawner::new(&deps_clone);
        if let Err(e) = spawner.spawn_interactive(&session_clone, true).await {
            tracing::error!(session_id = session_clone.id, error = %e, "agent resume failed");
            let store = SessionStore::new(deps_clone.db.clone());
            let _ = store
                .update_status(
                    &session_clone.id,
                    SessionStatus::Failed,
                    Some(ExitReason::Error),
                    Some(unix_ms()),
                )
                .await;
            let _ = deps_clone.event_bus.emit(AppEvent::SessionSpawnFailed {
                session_id: session_clone.id.clone(),
                reason: e.to_string(),
            });
        }
    });

    Ok((StatusCode::CREATED, Json(session)))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Current time as Unix milliseconds.
fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

/// Human-readable display string for a session status.
fn status_display(s: &SessionStatus) -> &'static str {
    match s {
        SessionStatus::Queued => "queued",
        SessionStatus::Running => "running",
        SessionStatus::NeedsInput => "needs_input",
        SessionStatus::AuthRequired => "auth_required",
        SessionStatus::Completed => "completed",
        SessionStatus::Failed => "failed",
        SessionStatus::Cancelled => "cancelled",
        SessionStatus::Interrupted => "interrupted",
    }
}
