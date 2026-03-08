//! Project CRUD route handlers.
//!
//! Implements the REST endpoints for project management:
//! list, create (register), and delete (unregister).

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;

use crate::deps::AppDeps;
use crate::error::Result;
use crate::services::project_store::ProjectStore;

/// Input for registering a new project.
#[derive(Debug, Deserialize)]
pub(crate) struct CreateProjectInput {
    pub path: String,
    pub name: String,
}

/// `GET /api/projects` -- list all registered projects.
pub(crate) async fn list_projects(State(deps): State<AppDeps>) -> Result<impl IntoResponse> {
    let store = ProjectStore::new(deps.db);
    let projects = store.list().await?;
    Ok(Json(projects))
}

/// `POST /api/projects` -- register (or re-register) a project.
pub(crate) async fn create_project(
    State(deps): State<AppDeps>,
    Json(input): Json<CreateProjectInput>,
) -> Result<impl IntoResponse> {
    let store = ProjectStore::new(deps.db);
    let project = store.create(&input.path, &input.name).await?;
    Ok((StatusCode::CREATED, Json(project)))
}

/// `DELETE /api/projects/:path` -- unregister a project.
///
/// The `:path` parameter is URL-encoded (e.g., `%2FUsers%2Ffoo%2Fbar`).
pub(crate) async fn delete_project(
    State(deps): State<AppDeps>,
    Path(path): Path<String>,
) -> Result<impl IntoResponse> {
    let store = ProjectStore::new(deps.db);
    store.delete(&path).await?;
    Ok(Json(json!({ "ok": true })))
}
