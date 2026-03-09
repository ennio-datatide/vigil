//! Pipeline execution route handlers.
//!
//! Implements the REST endpoints for triggering and querying pipeline
//! executions: execute, get, and list.

use std::sync::Arc;

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::deps::AppDeps;
use crate::error::{Error, Result};
use crate::services::pipeline_execution_store::PipelineExecutionStore;
use crate::services::pipeline_runner::PipelineRunner;
use crate::services::pipeline_store::PipelineStore;

/// Request body for `POST /api/pipelines/:id/execute`.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecutePipelineBody {
    pub prompt: String,
    pub project_path: String,
}

/// `POST /api/pipelines/:id/execute` -- start a pipeline execution.
pub(crate) async fn execute_pipeline(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
    Json(body): Json<ExecutePipelineBody>,
) -> Result<impl IntoResponse> {
    // Look up the pipeline.
    let pipeline_store = PipelineStore::new(Arc::clone(&deps.db));
    let pipeline = pipeline_store
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("pipeline {id} not found")))?;

    // Create the execution record.
    let exec_store = PipelineExecutionStore::new(Arc::clone(&deps.db));
    let execution = exec_store
        .create(&id, &body.prompt, &body.project_path)
        .await?;

    // Start the runner in the background.
    let runner = PipelineRunner::new(&deps);
    runner
        .start(pipeline, body.prompt, body.project_path, execution.id.clone())
        .await
        .map_err(Error::Other)?;

    // Re-fetch to get the updated status (now running).
    let execution = exec_store
        .get(&execution.id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("pipeline execution {} not found", execution.id)))?;

    Ok((StatusCode::CREATED, Json(execution)))
}

/// `GET /api/executions/:id` -- get a single pipeline execution.
pub(crate) async fn get_execution(
    State(deps): State<AppDeps>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let store = PipelineExecutionStore::new(Arc::clone(&deps.db));
    let execution = store
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("pipeline execution {id} not found")))?;
    Ok(Json(execution))
}

/// `GET /api/executions` -- list all pipeline executions.
pub(crate) async fn list_executions(State(deps): State<AppDeps>) -> Result<impl IntoResponse> {
    let store = PipelineExecutionStore::new(Arc::clone(&deps.db));
    let executions = store.list().await?;
    Ok(Json(executions))
}
