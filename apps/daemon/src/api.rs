//! HTTP API layer.
//!
//! Defines the Axum router, health endpoint, authentication middleware,
//! and route modules.

pub mod health;
pub mod middleware;

use axum::Router;
use tower_http::cors::CorsLayer;

use crate::deps::AppDeps;

/// Build the full application router.
pub fn router(deps: AppDeps) -> Router {
    let api_routes = Router::new()
        .layer(axum::middleware::from_fn_with_state(
            deps.clone(),
            middleware::auth,
        ));

    Router::new()
        .route("/health", axum::routing::get(health::health))
        .nest("/api", api_routes)
        .layer(CorsLayer::permissive())
        .with_state(deps)
}
