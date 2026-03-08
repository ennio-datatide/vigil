//! Pipeline CRUD route handlers.
//!
//! Implements the REST endpoints for pipeline management:
//! list, get, create, update, and delete.

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;

use crate::deps::AppDeps;
use crate::error::Result;
use crate::services::pipeline_store::{CreatePipelineInput, PipelineStore, UpdatePipelineInput};

/// `GET /api/pipelines` -- list all pipelines.
pub(crate) async fn list_pipelines(State(deps): State<AppDeps>) -> Result<impl IntoResponse> {
    let store = PipelineStore::new(deps.db);
    let pipelines = store.list().await?;
    Ok(Json(pipelines))
}

/// `GET /api/pipelines/:id` -- get a pipeline by id.
pub(crate) async fn get_pipeline(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let store = PipelineStore::new(deps.db);
    let pipeline = store
        .get(&id)
        .await?
        .ok_or_else(|| crate::error::Error::NotFound(format!("pipeline {id} not found")))?;
    Ok(Json(pipeline))
}

/// `POST /api/pipelines` -- create a new pipeline.
pub(crate) async fn create_pipeline(
    State(deps): State<AppDeps>,
    Json(input): Json<CreatePipelineInput>,
) -> Result<impl IntoResponse> {
    let store = PipelineStore::new(deps.db);
    let pipeline = store.create(input).await?;
    Ok((StatusCode::CREATED, Json(pipeline)))
}

/// `PUT /api/pipelines/:id` -- update an existing pipeline.
pub(crate) async fn update_pipeline(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
    Json(input): Json<UpdatePipelineInput>,
) -> Result<impl IntoResponse> {
    let store = PipelineStore::new(deps.db);
    let pipeline = store.update(&id, input).await?;
    Ok(Json(pipeline))
}

/// `DELETE /api/pipelines/:id` -- delete a pipeline.
pub(crate) async fn delete_pipeline(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let store = PipelineStore::new(deps.db);
    store.delete(&id).await?;
    Ok(Json(json!({ "ok": true })))
}
