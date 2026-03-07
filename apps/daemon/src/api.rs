//! HTTP API layer.
//!
//! Defines the Axum router, health endpoint, authentication middleware,
//! and route modules.

pub(crate) mod events;
pub mod health;
pub mod middleware;
pub(crate) mod notifications;
pub(crate) mod projects;
pub(crate) mod sessions;

use axum::routing::{delete, get, patch, post};
use axum::Router;
use tower_http::cors::CorsLayer;

use crate::deps::AppDeps;

/// Build the full application router.
pub fn router(deps: AppDeps) -> Router {
    let api_routes = Router::new()
        .route("/sessions", get(sessions::list_sessions))
        .route("/sessions", post(sessions::create_session))
        .route("/sessions/{id}", get(sessions::get_session))
        .route("/sessions/{id}", delete(sessions::cancel_session))
        .route("/sessions/{id}/remove", delete(sessions::remove_session))
        .route("/sessions/{id}/restart", post(sessions::restart_session))
        .route("/sessions/{id}/resume", post(sessions::resume_session))
        .route("/projects", get(projects::list_projects))
        .route("/projects", post(projects::create_project))
        .route("/projects/{path}", delete(projects::delete_project))
        .route("/notifications", get(notifications::list_notifications))
        .route("/notifications/test", post(notifications::test_notification))
        .route("/notifications/read-all", patch(notifications::read_all))
        .route("/notifications/{id}/read", patch(notifications::mark_read))
        .layer(axum::middleware::from_fn_with_state(
            deps.clone(),
            middleware::auth,
        ));

    Router::new()
        .route("/health", get(health::health))
        .route("/events", post(events::ingest_event))
        .nest("/api", api_routes)
        .layer(CorsLayer::permissive())
        .with_state(deps)
}
