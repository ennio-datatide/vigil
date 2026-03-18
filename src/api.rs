//! HTTP API layer — hook ingestion and health check only.

pub(crate) mod events;
pub mod health;
pub(crate) mod vigil;

use axum::Router;
use axum::routing::{get, post};

use crate::deps::AppDeps;

/// Build the application router (events + health + vigil chat).
pub fn router(deps: AppDeps) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/events", post(events::ingest_event))
        .route("/api/vigil/chat", post(vigil::chat))
        .with_state(deps)
}
