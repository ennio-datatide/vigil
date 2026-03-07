//! HTTP API layer.
//!
//! Defines the Axum router, health endpoint, authentication middleware,
//! and route modules.

pub mod health;
pub mod middleware;
pub(crate) mod sessions;
pub(crate) mod settings;

use axum::routing::{delete, get, post};
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
        .route(
            "/settings/telegram",
            get(settings::get_telegram).put(settings::put_telegram),
        )
        .route("/settings/telegram/test", post(settings::test_telegram))
        .layer(axum::middleware::from_fn_with_state(
            deps.clone(),
            middleware::auth,
        ));

    Router::new()
        .route("/health", get(health::health))
        .nest("/api", api_routes)
        .layer(CorsLayer::permissive())
        .with_state(deps)
}
