//! Authentication and request processing middleware.
//!
//! Provides bearer token authentication with timing-safe comparison.
//! Skips auth entirely when no token is configured.

use axum::extract::{Query, State};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::deps::AppDeps;

/// Query parameters that may carry an authentication token.
#[derive(Debug, serde::Deserialize)]
pub struct AuthQuery {
    pub token: Option<String>,
}

/// Authentication middleware.
///
/// Checks for a bearer token in the `Authorization` header or `?token=` query
/// parameter. If no token is configured in [`AppDeps::config`], all requests
/// are allowed through.
///
/// # Errors
///
/// Returns `UNAUTHORIZED` if the token is missing or invalid.
pub async fn auth(
    State(deps): State<AppDeps>,
    Query(query): Query<AuthQuery>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = &deps.config.auth_token else {
        // No token configured — skip auth.
        return Ok(next.run(request).await);
    };

    let provided = extract_token(&request, &query);

    match provided {
        Some(token) if timing_safe_eq(token.as_bytes(), expected.as_bytes()) => {
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Extract the bearer token from the `Authorization` header or query parameter.
fn extract_token<'a>(
    request: &'a Request<axum::body::Body>,
    query: &'a AuthQuery,
) -> Option<&'a str> {
    // Try Authorization header first.
    if let Some(value) = request.headers().get("authorization")
        && let Ok(header) = value.to_str()
        && let Some(token) = header.strip_prefix("Bearer ")
    {
        return Some(token);
    }

    // Fall back to query parameter.
    query.token.as_deref()
}

/// Constant-time byte comparison to prevent timing attacks.
fn timing_safe_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}
