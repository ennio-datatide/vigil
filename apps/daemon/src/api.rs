//! HTTP API layer.
//!
//! Defines the Axum router, health endpoint, authentication middleware,
//! and route modules.

pub(crate) mod events;
pub(crate) mod filesystem;
pub mod health;
pub mod middleware;
pub(crate) mod notifications;
pub(crate) mod pipelines;
pub(crate) mod projects;
pub(crate) mod sessions;
pub(crate) mod settings;
pub(crate) mod skills;

use axum::routing::{delete, get, patch, post, put};
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
        .route("/skills", get(skills::list_skills))
        .route("/pipelines", get(pipelines::list_pipelines))
        .route("/pipelines", post(pipelines::create_pipeline))
        .route("/pipelines/{id}", get(pipelines::get_pipeline))
        .route("/pipelines/{id}", put(pipelines::update_pipeline))
        .route("/pipelines/{id}", delete(pipelines::delete_pipeline))
        .route(
            "/settings/telegram",
            get(settings::get_telegram).put(settings::put_telegram),
        )
        .route("/settings/telegram/test", post(settings::test_telegram))
        .route("/fs/dirs", get(filesystem::list_dirs))
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
