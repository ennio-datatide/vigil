//! Domain model types mirroring the `SQLite` schema.
//!
//! All structs use `camelCase` serialization to match the frontend API contract.
//! Timestamps are Unix milliseconds (`i64`) to match the TypeScript frontend.

#![allow(dead_code)] // Models are defined ahead of their consumers.
#![allow(clippy::struct_field_names)] // Field names match the frontend JSON contract.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Current status of a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Queued,
    Running,
    NeedsInput,
    AuthRequired,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

/// Type of AI agent driving a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    Claude,
    Codex,
}

/// Role assigned to a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    Implementer,
    Reviewer,
    Fixer,
    Custom,
}

/// Reason a session exited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    Completed,
    Error,
    UserCancelled,
    ChainTriggered,
}

/// How a sub-session was spawned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnType {
    Branch,
    Worker,
}

/// Type of notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    NeedsInput,
    Error,
    AuthRequired,
    ChainComplete,
    SessionDone,
}

/// Git repository metadata attached to a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitMetadata {
    pub repo_name: String,
    pub branch: String,
    pub commit_hash: String,
    pub remote_url: Option<String>,
}

/// Type of a memory entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    Fact,
    Decision,
    Preference,
    Pattern,
    Failure,
    Todo,
}

/// Type of edge connecting two memories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEdgeType {
    RelatedTo,
    Updates,
    Contradicts,
    CausedBy,
    PartOf,
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// An orchestrated AI coding session.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub project_path: String,
    pub worktree_path: Option<String>,
    pub tmux_session: Option<String>,
    pub prompt: String,
    pub skills_used: Option<Vec<String>>,
    pub status: SessionStatus,
    pub agent_type: AgentType,
    pub role: Option<SessionRole>,
    pub parent_id: Option<String>,
    pub spawn_type: Option<SpawnType>,
    pub spawn_result: Option<String>,
    pub retry_count: i32,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub exit_reason: Option<ExitReason>,
    pub git_metadata: Option<GitMetadata>,
    pub pipeline_id: Option<String>,
    pub pipeline_step_index: Option<i32>,
}

/// A hook or lifecycle event recorded for a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: i64,
    pub session_id: String,
    pub event_type: String,
    pub tool_name: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub timestamp: i64,
}

/// A registered project directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub path: String,
    pub name: String,
    pub skills_dir: Option<String>,
    pub last_used_at: Option<i64>,
}

/// A visual position in the pipeline editor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// A single step in a pipeline.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStep {
    pub id: String,
    pub label: String,
    pub prompt: String,
    pub skill: Option<String>,
    pub position: Position,
}

/// A directed edge between two pipeline steps.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineEdge {
    pub id: String,
    pub source: String,
    pub target: String,
}

/// A multi-step pipeline definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<PipelineStep>,
    pub edges: Vec<PipelineEdge>,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A notification about a session event.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: i64,
    pub session_id: String,
    #[serde(rename = "type")]
    pub notification_type: NotificationType,
    pub message: String,
    pub sent_at: Option<i64>,
    pub read_at: Option<i64>,
}

/// A memory entry in the knowledge graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    pub project_path: String,
    pub content: String,
    pub memory_type: MemoryType,
    pub source_session_id: Option<String>,
    pub importance: f64,
    pub access_count: i64,
    pub created_at: i64,
    pub accessed_at: i64,
}

/// A directed edge between two memory entries.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEdge {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub edge_type: MemoryEdgeType,
    pub weight: f64,
    pub created_at: i64,
}
