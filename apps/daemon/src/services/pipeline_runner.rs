//! DAG executor for multi-step pipeline execution with context chaining.
//!
//! [`PipelineRunner`] orchestrates pipeline steps sequentially, spawning an
//! agent session for each step, waiting for completion, extracting output,
//! and feeding it as context into subsequent steps.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use crate::db::models::{Pipeline, SessionStatus};
use crate::deps::AppDeps;
use crate::events::AppEvent;
use crate::process::agent_spawner::AgentSpawner;
use crate::process::output_extract::{build_context_chain, extract_response_text};
use crate::services::pipeline_execution_store::PipelineExecutionStore;
use crate::services::session_store::{CreateSessionInput, SessionStore};

/// Maximum time to wait for a single pipeline step to complete.
const STEP_TIMEOUT: Duration = Duration::from_secs(600);

/// Orchestrates pipeline execution by running steps in topological order.
#[allow(dead_code)] // Constructed by pipeline execution endpoints (Task 6+).
pub(crate) struct PipelineRunner {
    deps: AppDeps,
}

#[allow(dead_code)] // Methods called by pipeline execution endpoints.
impl PipelineRunner {
    /// Create a new runner from the shared application dependencies.
    #[must_use]
    pub(crate) fn new(deps: &AppDeps) -> Self {
        Self {
            deps: deps.clone(),
        }
    }

    /// Start executing a pipeline in the background.
    ///
    /// Creates the execution record, marks it as running, spawns a background
    /// task for the core loop, and returns the execution ID immediately.
    ///
    /// # Errors
    ///
    /// Returns an error if the execution record cannot be created.
    pub(crate) async fn start(
        &self,
        pipeline: Pipeline,
        prompt: String,
        project_path: String,
        execution_id: String,
    ) -> anyhow::Result<String> {
        let exec_store = PipelineExecutionStore::new(Arc::clone(&self.deps.db));

        // Mark execution as running.
        exec_store
            .update_status(&execution_id, crate::db::models::PipelineExecutionStatus::Running)
            .await?;

        let step_order = topological_sort(&pipeline);
        let deps = self.deps.clone();
        let eid = execution_id.clone();

        tokio::spawn(async move {
            if let Err(e) = run_pipeline(
                deps.clone(),
                pipeline,
                step_order,
                prompt,
                project_path,
                eid.clone(),
            )
            .await
            {
                tracing::error!(execution_id = %eid, error = %e, "pipeline execution failed");
                // Best-effort: mark execution as failed if not already.
                let store = PipelineExecutionStore::new(Arc::clone(&deps.db));
                let _ = store.mark_failed(&eid).await;
            }
        });

        Ok(execution_id)
    }
}

/// Compute a topological ordering of pipeline step indices based on edges.
///
/// Uses Kahn's algorithm (BFS). If edges are empty or a cycle is detected,
/// falls back to the natural order `[0, 1, 2, ...]`.
pub(crate) fn topological_sort(pipeline: &Pipeline) -> Vec<usize> {
    let steps = &pipeline.steps;
    let edges = &pipeline.edges;

    if steps.is_empty() {
        return vec![];
    }

    if edges.is_empty() {
        return (0..steps.len()).collect();
    }

    // Build an index lookup: step.id → index.
    let id_to_idx: HashMap<&str, usize> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), i))
        .collect();

    // Build adjacency list and in-degree count.
    let n = steps.len();
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    let mut in_degree: Vec<usize> = vec![0; n];

    for edge in edges {
        let Some(&src) = id_to_idx.get(edge.source.as_str()) else {
            continue;
        };
        let Some(&tgt) = id_to_idx.get(edge.target.as_str()) else {
            continue;
        };
        adj[src].push(tgt);
        in_degree[tgt] += 1;
    }

    // Kahn's algorithm.
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut order = Vec::with_capacity(n);
    let mut visited: HashSet<usize> = HashSet::new();

    while let Some(node) = queue.pop_front() {
        if !visited.insert(node) {
            continue;
        }
        order.push(node);
        for &neighbor in &adj[node] {
            in_degree[neighbor] = in_degree[neighbor].saturating_sub(1);
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    // If we didn't visit all nodes, there's a cycle — fall back to natural order.
    if order.len() != n {
        tracing::warn!(
            pipeline_id = %pipeline.id,
            "cycle detected in pipeline edges, falling back to natural order"
        );
        return (0..n).collect();
    }

    order
}

/// Core pipeline execution loop.
///
/// Iterates through steps in the given order, spawning an agent session for
/// each step, waiting for it to complete, and chaining its output as context
/// into subsequent steps.
async fn run_pipeline(
    deps: AppDeps,
    pipeline: Pipeline,
    step_order: Vec<usize>,
    initial_prompt: String,
    project_path: String,
    execution_id: String,
) -> anyhow::Result<()> {
    let exec_store = PipelineExecutionStore::new(Arc::clone(&deps.db));
    let session_store = SessionStore::new(Arc::clone(&deps.db));
    let spawner = AgentSpawner::new(&deps);

    let mut completed_labels: Vec<String> = Vec::new();
    let mut completed_outputs: Vec<String> = Vec::new();

    for (order_idx, &step_idx) in step_order.iter().enumerate() {
        let step = &pipeline.steps[step_idx];
        let step_idx_i32 = i32::try_from(step_idx).unwrap_or(i32::MAX);

        tracing::info!(
            execution_id = %execution_id,
            step_index = step_idx,
            step_label = %step.label,
            "starting pipeline step {}/{}",
            order_idx + 1,
            step_order.len(),
        );

        let result = run_single_step(
            &deps,
            &exec_store,
            &session_store,
            &spawner,
            &pipeline,
            step_idx,
            step_idx_i32,
            &execution_id,
            &initial_prompt,
            &project_path,
            &completed_labels,
            &completed_outputs,
        )
        .await?;

        match result {
            StepResult::Completed { label, output } => {
                completed_labels.push(label);
                completed_outputs.push(output);
            }
            StepResult::Failed => return Ok(()),
        }
    }

    // All steps completed successfully.
    exec_store.mark_completed(&execution_id).await?;
    tracing::info!(execution_id = %execution_id, "pipeline execution completed successfully");

    Ok(())
}

/// Outcome of a single pipeline step execution.
enum StepResult {
    Completed { label: String, output: String },
    Failed,
}

/// Execute a single pipeline step: create session, spawn agent, wait, extract output.
#[allow(clippy::too_many_arguments)]
async fn run_single_step(
    deps: &AppDeps,
    exec_store: &PipelineExecutionStore,
    session_store: &SessionStore,
    spawner: &AgentSpawner,
    pipeline: &Pipeline,
    step_idx: usize,
    step_idx_i32: i32,
    execution_id: &str,
    initial_prompt: &str,
    project_path: &str,
    completed_labels: &[String],
    completed_outputs: &[String],
) -> anyhow::Result<StepResult> {
    let step = &pipeline.steps[step_idx];

    // Update current step index in execution record.
    exec_store
        .set_current_step(execution_id, step_idx_i32)
        .await?;

    // Build the composite prompt with context from previous steps.
    let composite_prompt = build_context_chain(
        initial_prompt,
        completed_labels,
        completed_outputs,
        &step.prompt,
    );

    // Create a session for this step.
    let session_id = uuid::Uuid::new_v4().to_string();
    let input = CreateSessionInput {
        project_path: project_path.to_string(),
        prompt: composite_prompt.clone(),
        skill: step.skill.clone(),
        role: None,
        parent_id: None,
        spawn_type: None,
        skip_permissions: Some(true),
        pipeline_id: Some(pipeline.id.clone()),
    };

    let session = session_store.create(&session_id, &input).await?;
    session_store
        .set_pipeline_step_index(&session_id, step_idx_i32)
        .await?;
    exec_store
        .record_step_session(execution_id, &step.id, &session_id)
        .await?;

    // Spawn and wait.
    spawner
        .spawn_interactive_pipeline_step(&session, &composite_prompt)
        .await?;

    let terminal_status = wait_for_session_terminal(deps, &session_id).await;

    match terminal_status {
        Some(SessionStatus::Completed) => {
            tracing::info!(
                execution_id = %execution_id,
                session_id = %session_id,
                step_label = %step.label,
                "pipeline step completed successfully",
            );

            let raw_output = deps
                .output_manager
                .get_full_output(&session_id)
                .unwrap_or_default();

            let response_text = extract_response_text(&raw_output);
            exec_store
                .record_step_output(execution_id, &step.id, &response_text)
                .await?;

            Ok(StepResult::Completed {
                label: step.label.clone(),
                output: response_text,
            })
        }
        Some(status) => {
            tracing::error!(
                execution_id = %execution_id,
                session_id = %session_id,
                step_label = %step.label,
                ?status,
                "pipeline step ended with non-success status",
            );
            exec_store.mark_failed(execution_id).await?;
            Ok(StepResult::Failed)
        }
        None => {
            tracing::error!(
                execution_id = %execution_id,
                session_id = %session_id,
                step_label = %step.label,
                "pipeline step timed out after {} seconds",
                STEP_TIMEOUT.as_secs(),
            );
            let _ = session_store
                .update_status(
                    &session_id,
                    SessionStatus::Failed,
                    Some(crate::db::models::ExitReason::Error),
                    Some(chrono::Utc::now().timestamp_millis()),
                )
                .await;
            exec_store.mark_failed(execution_id).await?;
            Ok(StepResult::Failed)
        }
    }
}

/// Wait for a session to reach a terminal status by subscribing to the event bus.
///
/// Returns `Some(status)` if the session reaches a terminal state, or `None`
/// if the timeout expires.
async fn wait_for_session_terminal(
    deps: &AppDeps,
    session_id: &str,
) -> Option<SessionStatus> {
    let mut rx = deps.event_bus.subscribe();

    let result = tokio::time::timeout(STEP_TIMEOUT, async {
        loop {
            match rx.recv().await {
                Ok(AppEvent::SessionUpdate { session }) => {
                    if session.id == session_id && is_terminal(&session.status) {
                        return session.status;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    // Event bus shut down — treat as failure.
                    return SessionStatus::Failed;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        session_id,
                        skipped = n,
                        "event bus receiver lagged, checking current status",
                    );
                    // Check current status in case we missed the terminal event.
                    let store = SessionStore::new(Arc::clone(&deps.db));
                    if let Ok(Some(session)) = store.get(session_id).await
                        && is_terminal(&session.status)
                    {
                        return session.status;
                    }
                }
                _ => {
                    // Other event types — continue waiting.
                }
            }
        }
    })
    .await;

    result.ok()
}

/// Check whether a session status is terminal.
fn is_terminal(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Completed
            | SessionStatus::Failed
            | SessionStatus::Cancelled
            | SessionStatus::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{PipelineEdge, PipelineStep, Position};

    fn make_pipeline(steps: Vec<(&str, &str)>, edges: Vec<(&str, &str)>) -> Pipeline {
        Pipeline {
            id: "test-pipe".into(),
            name: "Test".into(),
            description: "test pipeline".into(),
            steps: steps
                .into_iter()
                .map(|(id, label)| PipelineStep {
                    id: id.into(),
                    label: label.into(),
                    prompt: format!("Do {label}"),
                    skill: None,
                    position: Position { x: 0.0, y: 0.0 },
                })
                .collect(),
            edges: edges
                .into_iter()
                .enumerate()
                .map(|(i, (src, tgt))| PipelineEdge {
                    id: format!("e{i}"),
                    source: src.into(),
                    target: tgt.into(),
                })
                .collect(),
            is_default: false,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn topological_sort_empty_edges_returns_natural_order() {
        let pipeline = make_pipeline(
            vec![("a", "A"), ("b", "B"), ("c", "C")],
            vec![],
        );
        assert_eq!(topological_sort(&pipeline), vec![0, 1, 2]);
    }

    #[test]
    fn topological_sort_linear_chain() {
        let pipeline = make_pipeline(
            vec![("a", "A"), ("b", "B"), ("c", "C")],
            vec![("a", "b"), ("b", "c")],
        );
        assert_eq!(topological_sort(&pipeline), vec![0, 1, 2]);
    }

    #[test]
    fn topological_sort_reverse_edges() {
        let pipeline = make_pipeline(
            vec![("a", "A"), ("b", "B"), ("c", "C")],
            vec![("c", "b"), ("b", "a")],
        );
        assert_eq!(topological_sort(&pipeline), vec![2, 1, 0]);
    }

    #[test]
    fn topological_sort_diamond() {
        // a → b, a → c, b → d, c → d
        let pipeline = make_pipeline(
            vec![("a", "A"), ("b", "B"), ("c", "C"), ("d", "D")],
            vec![("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")],
        );
        let order = topological_sort(&pipeline);
        assert_eq!(order[0], 0); // a must be first
        assert_eq!(order[3], 3); // d must be last
        // b and c can be in either order
        assert!(order[1] == 1 || order[1] == 2);
        assert!(order[2] == 1 || order[2] == 2);
    }

    #[test]
    fn topological_sort_cycle_falls_back() {
        let pipeline = make_pipeline(
            vec![("a", "A"), ("b", "B")],
            vec![("a", "b"), ("b", "a")],
        );
        // Cycle detected → natural order.
        assert_eq!(topological_sort(&pipeline), vec![0, 1]);
    }

    #[test]
    fn topological_sort_empty_pipeline() {
        let pipeline = make_pipeline(vec![], vec![]);
        assert_eq!(topological_sort(&pipeline), Vec::<usize>::new());
    }
}
