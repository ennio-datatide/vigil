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
    let Some(expected) = &deps.config.api_token else {
        // No token configured — skip auth.
        return Ok(next.run(request).await);
    };

    let provided = extract_token(&request, &query);

    match provided {
        Some(token) if constant_time_eq(token.as_bytes(), expected.as_bytes()) => {
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
///
/// Uses the `subtle` crate for a proper constant-time equality check.
/// The early return on length mismatch is acceptable for fixed-length API tokens.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;

    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}
