//! MCP server for Vigil tools.
//!
//! Exposes the 7 Vigil tools (`memory_recall`, `memory_save`, `memory_delete`,
//! `session_recall`, `acta_update`, `spawn_worker`, `execute_pipeline`) via the Model Context Protocol
//! stdio transport. Launched as a subprocess by the `claude` CLI.

use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::io::stdio,
    ErrorData as McpError,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Tool argument structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct MemoryRecallArgs {
    /// Natural-language query to search memories.
    pub query: String,
    /// Absolute path to the project directory.
    pub project_path: String,
    /// Maximum number of results to return (default: server-side default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct MemorySaveArgs {
    /// The memory content to persist.
    pub content: String,
    /// Type of memory (e.g. "lesson", "fact", "preference").
    pub memory_type: String,
    /// Absolute path to the project directory.
    pub project_path: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct MemoryDeleteArgs {
    /// ID of the memory to delete.
    pub memory_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct SessionRecallArgs {
    /// Specific session ID to retrieve. If omitted, lists sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Filter sessions by project path (client-side filtering).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct ActaUpdateArgs {
    /// Absolute path to the project directory.
    pub project_path: String,
    /// Updated acta (briefing) content.
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct SpawnWorkerArgs {
    /// Absolute path to the project directory.
    pub project_path: String,
    /// Prompt / instructions for the worker session.
    pub prompt: String,
    /// Optional skill to assign the worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    /// If true (default), block until the worker completes and return its output.
    /// Set to false for long-running tasks where you don't need the result immediately.
    #[serde(default = "default_wait")]
    pub wait: bool,
}

fn default_wait() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct ExecutePipelineArgs {
    /// Absolute path to the project directory.
    pub project_path: String,
    /// The user's request / instructions for the pipeline.
    pub prompt: String,
    /// Pipeline ID to execute. If omitted, uses the default pipeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline_id: Option<String>,
}

// ---------------------------------------------------------------------------
// MCP server
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct VigilMcpServer {
    daemon_url: String,
    client: reqwest::Client,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl VigilMcpServer {
    fn new(daemon_url: String) -> Self {
        Self {
            daemon_url,
            client: reqwest::Client::new(),
            tool_router: Self::tool_router(),
        }
    }

    /// Search project memories by semantic similarity.
    #[tool(name = "memory_recall", description = "Search project memories by semantic similarity. Returns the most relevant memories matching the query.")]
    async fn memory_recall(
        &self,
        Parameters(args): Parameters<MemoryRecallArgs>,
    ) -> Result<CallToolResult, McpError> {
        let url = format!("{}/api/memory/search", self.daemon_url);
        let body = serde_json::json!({
            "query": args.query,
            "projectPath": args.project_path,
            "limit": args.limit,
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| McpError::internal_error(format!("HTTP request failed: {e}"), None))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to read response: {e}"), None))?;

        if status.is_success() {
            Ok(CallToolResult::success(vec![Content::text(text)]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Error {status}: {text}"
            ))]))
        }
    }

    /// Save a new memory for a project.
    #[tool(name = "memory_save", description = "Save a new memory for a project. Memories persist across sessions and are searchable.")]
    async fn memory_save(
        &self,
        Parameters(args): Parameters<MemorySaveArgs>,
    ) -> Result<CallToolResult, McpError> {
        let url = format!("{}/api/memory", self.daemon_url);
        let body = serde_json::json!({
            "content": args.content,
            "memoryType": args.memory_type,
            "projectPath": args.project_path,
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| McpError::internal_error(format!("HTTP request failed: {e}"), None))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to read response: {e}"), None))?;

        if status.is_success() {
            Ok(CallToolResult::success(vec![Content::text(text)]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Error {status}: {text}"
            ))]))
        }
    }

    /// Delete a memory by ID.
    #[tool(name = "memory_delete", description = "Delete a specific memory by its ID.")]
    async fn memory_delete(
        &self,
        Parameters(args): Parameters<MemoryDeleteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let url = format!("{}/api/memory/{}", self.daemon_url, args.memory_id);

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| McpError::internal_error(format!("HTTP request failed: {e}"), None))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to read response: {e}"), None))?;

        if status.is_success() {
            Ok(CallToolResult::success(vec![Content::text(
                if text.is_empty() {
                    format!("Memory {} deleted", args.memory_id)
                } else {
                    text
                },
            )]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Error {status}: {text}"
            ))]))
        }
    }

    /// Recall session information — either a specific session or list sessions filtered by project.
    #[tool(name = "session_recall", description = "Recall session information. Provide session_id to get a specific session, or project_path to list sessions for a project.")]
    async fn session_recall(
        &self,
        Parameters(args): Parameters<SessionRecallArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (url, needs_filter) = if let Some(ref id) = args.session_id {
            (format!("{}/api/sessions/{id}", self.daemon_url), false)
        } else {
            (format!("{}/api/sessions", self.daemon_url), true)
        };

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| McpError::internal_error(format!("HTTP request failed: {e}"), None))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to read response: {e}"), None))?;

        if !status.is_success() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Error {status}: {text}"
            ))]));
        }

        // Client-side filter by project_path when listing all sessions.
        if needs_filter
            && let Some(ref project_path) = args.project_path
        {
            let filtered = filter_sessions_by_project(&text, project_path);
            return Ok(CallToolResult::success(vec![Content::text(filtered)]));
        }

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    /// Update the project acta (briefing document).
    #[tool(name = "acta_update", description = "Update the project acta (briefing document) with new content. The acta summarizes project context for future sessions.")]
    async fn acta_update(
        &self,
        Parameters(args): Parameters<ActaUpdateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let response = self
            .client
            .put(format!("{}/api/vigil/acta", self.daemon_url))
            .json(&serde_json::json!({
                "projectPath": args.project_path,
                "content": args.content,
            }))
            .send()
            .await
            .map_err(|e| McpError::internal_error(format!("HTTP request failed: {e}"), None))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to read response: {e}"), None))?;

        if status.is_success() {
            Ok(CallToolResult::success(vec![Content::text(text)]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Error {status}: {text}"
            ))]))
        }
    }

    /// Spawn a new worker session with a specific prompt.
    #[tool(name = "spawn_worker", description = "Spawn a new Claude Code worker session. By default waits for completion and returns the worker's output. Set wait=false for long-running tasks.")]
    async fn spawn_worker(
        &self,
        Parameters(args): Parameters<SpawnWorkerArgs>,
    ) -> Result<CallToolResult, McpError> {
        let url = format!("{}/api/sessions", self.daemon_url);
        let mut body = serde_json::json!({
            "projectPath": args.project_path,
            "prompt": args.prompt,
        });

        if let Some(skill) = args.skill {
            body["skill"] = serde_json::Value::String(skill);
        }

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| McpError::internal_error(format!("HTTP request failed: {e}"), None))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to read response: {e}"), None))?;

        if !status.is_success() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Error {status}: {text}"
            ))]));
        }

        // Extract session ID from the create response.
        let session_id = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["id"].as_str().map(String::from));

        if !args.wait {
            return Ok(CallToolResult::success(vec![Content::text(text)]));
        }

        // Wait for the worker to complete, polling every 3 seconds.
        let Some(ref sid) = session_id else {
            return Ok(CallToolResult::success(vec![Content::text(text)]));
        };

        let session_url = format!("{}/api/sessions/{sid}", self.daemon_url);
        let max_wait = std::time::Duration::from_secs(240);
        let poll_interval = std::time::Duration::from_secs(3);
        let start = std::time::Instant::now();

        loop {
            tokio::time::sleep(poll_interval).await;

            if start.elapsed() > max_wait {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Worker {sid} is still running after {}s. Check status with session_recall.",
                    max_wait.as_secs()
                ))]));
            }

            let Ok(resp) = self.client.get(&session_url).send().await else {
                continue;
            };

            let Ok(body) = resp.text().await else {
                continue;
            };

            let Ok(session) = serde_json::from_str::<serde_json::Value>(&body) else {
                continue;
            };

            let status_str = session["status"].as_str().unwrap_or("");
            match status_str {
                "completed" | "failed" | "cancelled" | "interrupted" => {
                    // Session is done — return the full response including output.
                    let output = session["output"].as_str().unwrap_or("(no output captured)");
                    let result = format!(
                        "Worker {sid} {status_str}.\n\nOutput:\n{output}"
                    );
                    return Ok(CallToolResult::success(vec![Content::text(result)]));
                }
                // "needs_input", "auth_required", "running", "queued" — keep
                // polling. The auto-exit mechanism sends /exit after the Stop
                // hook, transitioning to completed.
                _ => {}
            }
        }
    }

    /// Execute a multi-step pipeline for coding tasks.
    #[tool(name = "execute_pipeline", description = "Execute a multi-step pipeline (brainstorm → design → code → review) for coding tasks. Non-blocking — starts the pipeline and returns immediately. The user can watch progress in the session monitor.")]
    async fn execute_pipeline(
        &self,
        Parameters(args): Parameters<ExecutePipelineArgs>,
    ) -> Result<CallToolResult, McpError> {
        // Step 1: Resolve pipeline_id — use provided or find the default.
        let pipeline_id = if let Some(id) = args.pipeline_id {
            id
        } else {
            // Find the default pipeline via GET /api/pipelines
            let url = format!("{}/api/pipelines", self.daemon_url);
            let response = self
                .client
                .get(&url)
                .send()
                .await
                .map_err(|e| McpError::internal_error(format!("HTTP request failed: {e}"), None))?;
            let text = response
                .text()
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("Failed to read response: {e}"), None)
                })?;
            let pipelines: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(|e| {
                McpError::internal_error(format!("Failed to parse pipelines: {e}"), None)
            })?;
            pipelines
                .iter()
                .find(|p| p["isDefault"].as_bool() == Some(true))
                .and_then(|p| p["id"].as_str())
                .map(String::from)
                .ok_or_else(|| {
                    McpError::internal_error("No default pipeline found".to_string(), None)
                })?
        };

        // Step 2: Execute via POST /api/pipelines/:id/execute
        let url = format!("{}/api/pipelines/{pipeline_id}/execute", self.daemon_url);
        let body = serde_json::json!({
            "projectPath": args.project_path,
            "prompt": args.prompt,
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| McpError::internal_error(format!("HTTP request failed: {e}"), None))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to read response: {e}"), None))?;

        if status.is_success() {
            let exec: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            let exec_id = exec["id"].as_str().unwrap_or("unknown");
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Pipeline execution started (ID: {exec_id}). The user can watch progress in the session monitor."
            ))]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Error {status}: {text}"
            ))]))
        }
    }
}

#[tool_handler]
impl ServerHandler for VigilMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Vigil MCP server — provides memory, session, worker, and pipeline management tools \
                 for the Praefectus AI session orchestrator."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "praefectus-vigil".into(),
                title: Some("Praefectus Vigil".into()),
                version: env!("CARGO_PKG_VERSION").into(),
                description: Some("MCP server for Vigil AI orchestration tools".into()),
                icons: None,
                website_url: None,
            },
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Filter a JSON array of sessions by `projectPath`, returning the filtered
/// JSON as a string. Falls back to the original text on parse errors.
fn filter_sessions_by_project(json_text: &str, project_path: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return json_text.to_owned();
    };

    let Some(arr) = value.as_array() else {
        return json_text.to_owned();
    };

    let filtered: Vec<&serde_json::Value> = arr
        .iter()
        .filter(|s| {
            s.get("projectPath")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|p| p == project_path)
        })
        .collect();

    serde_json::to_string_pretty(&filtered).unwrap_or_else(|_| json_text.to_owned())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the MCP server over stdin/stdout.
///
/// # Errors
///
/// Returns an error if tracing initialisation, transport setup, or the
/// MCP serve loop fails.
pub async fn run_mcp_server(daemon_url: String) -> Result<()> {
    // Initialise tracing to stderr — stdout is reserved for the MCP JSON-RPC
    // protocol and any stray writes would corrupt the stream.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(daemon_url, "starting Vigil MCP server");

    let server = VigilMcpServer::new(daemon_url);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
