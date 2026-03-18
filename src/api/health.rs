//! Health check and `OpenAPI` spec endpoints.

use axum::Json;
use axum::response::IntoResponse;
use serde_json::{Value, json};

/// `GET /health` — returns `{"status": "ok"}`.
pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// `GET /openapi.json` — serve the `OpenAPI` 3.1 specification.
pub async fn openapi_spec() -> impl IntoResponse {
    let spec = include_str!("../../openapi.json");
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        spec,
    )
}
