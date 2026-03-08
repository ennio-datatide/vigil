//! Hierarchical error types for the daemon.
//!
//! Provides a top-level [`Error`] that wraps domain-specific error enums,
//! plus an [`IntoResponse`] implementation for HTTP status code mapping.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Top-level error type for the daemon.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(#[from] Box<ConfigError>),

    #[error("database error: {0}")]
    Db(#[from] Box<DbError>),

    #[error("session error: {0}")]
    Session(#[from] Box<SessionError>),

    #[error("memory error: {0}")]
    Memory(#[from] Box<MemoryError>),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Configuration-related errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    ReadFile(#[source] std::io::Error),

    #[error("invalid config: {0}")]
    Invalid(String),

    #[error("failed to create directory: {0}")]
    CreateDir(#[source] std::io::Error),
}

/// Database-related errors.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Session lifecycle errors.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session not found: {0}")]
    NotFound(String),

    #[error("invalid status transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },

    #[error("spawn failed: {0}")]
    SpawnFailed(String),
}

/// Memory subsystem errors.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("embedding failed: {0}")]
    Embedding(String),

    #[error("vector store error: {0}")]
    VectorStore(String),

    #[error("memory not found: {0}")]
    NotFound(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::Session(inner) => match inner.as_ref() {
                SessionError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
                _ => (StatusCode::BAD_REQUEST, self.to_string()),
            },
            Self::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            Self::Config(_) | Self::Db(_) | Self::Memory(_) | Self::Other(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = axum::Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Convenience result type using the daemon [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

// Convenience conversions so callers can use `?` without explicit Boxing.
impl From<ConfigError> for Error {
    fn from(error: ConfigError) -> Self {
        Self::Config(Box::new(error))
    }
}

impl From<DbError> for Error {
    fn from(error: DbError) -> Self {
        Self::Db(Box::new(error))
    }
}

impl From<SessionError> for Error {
    fn from(error: SessionError) -> Self {
        Self::Session(Box::new(error))
    }
}

impl From<MemoryError> for Error {
    fn from(error: MemoryError) -> Self {
        Self::Memory(Box::new(error))
    }
}
