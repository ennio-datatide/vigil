//! Vigil agent -- long-running LLM overseer for project sessions.
//!
//! Defines six tools that the Vigil agent uses to observe sessions,
//! manage memories, maintain the project briefing (acta), and spawn
//! worker sessions. The agent is built using `rig-core` 0.31.

#![allow(dead_code)] // Module is wired ahead of its consumers.

use std::sync::Arc;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::json;

use crate::db::kv::KvStore;
use crate::db::sqlite::SqliteDb;
use crate::services::memory_search::MemorySearch;
use crate::services::memory_store::{CreateMemoryInput, MemoryStore};
use crate::services::session_store::SessionStore;
use crate::services::sub_session::SubSessionService;

// ---------------------------------------------------------------------------
// Shared error type
// ---------------------------------------------------------------------------

/// Error returned by all Vigil tools.
#[derive(Debug, thiserror::Error)]
#[error("vigil tool error: {0}")]
pub(crate) struct VigilToolError(String);

// ---------------------------------------------------------------------------
// MemoryRecallTool
// ---------------------------------------------------------------------------

/// Search project memories by semantic similarity.
pub(crate) struct MemoryRecallTool {
    memory_search: MemorySearch,
}

impl MemoryRecallTool {
    pub(crate) fn new(memory_search: MemorySearch) -> Self {
        Self { memory_search }
    }
}

#[derive(Deserialize)]
pub(crate) struct MemoryRecallArgs {
    /// Natural-language search query.
    query: String,
    /// Project path to scope the search to.
    project_path: String,
    /// Maximum number of results to return (default 10).
    limit: Option<usize>,
}

impl Tool for MemoryRecallTool {
    const NAME: &'static str = "memory_recall";

    type Error = VigilToolError;
    type Args = MemoryRecallArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search project memories by semantic similarity. Returns the most \
                          relevant memories for the given query, optionally filtered by project."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural-language search query"
                    },
                    "project_path": {
                        "type": "string",
                        "description": "Absolute path of the project to search memories for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 10)"
                    }
                },
                "required": ["query", "project_path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let limit = args.limit.unwrap_or(10);
        let results = self
            .memory_search
            .search(&args.query, Some(&args.project_path), limit)
            .await
            .map_err(|e| VigilToolError(e.to_string()))?;

        serde_json::to_value(&results).map_err(|e| VigilToolError(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// MemorySaveTool
// ---------------------------------------------------------------------------

/// Save a new memory for a project.
pub(crate) struct MemorySaveTool {
    memory_store: MemoryStore,
}

impl MemorySaveTool {
    pub(crate) fn new(memory_store: MemoryStore) -> Self {
        Self { memory_store }
    }
}

#[derive(Deserialize)]
pub(crate) struct MemorySaveArgs {
    /// The content text of the memory.
    content: String,
    /// Classification: fact, decision, pattern, preference, todo.
    memory_type: String,
    /// Project this memory belongs to.
    project_path: String,
}

impl Tool for MemorySaveTool {
    const NAME: &'static str = "memory_save";

    type Error = VigilToolError;
    type Args = MemorySaveArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Save a new memory for the project. Memories are searchable by \
                          semantic similarity and used to provide context to future sessions."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The memory content text"
                    },
                    "memory_type": {
                        "type": "string",
                        "enum": ["fact", "decision", "pattern", "preference", "todo"],
                        "description": "Classification of this memory"
                    },
                    "project_path": {
                        "type": "string",
                        "description": "Absolute path of the project"
                    }
                },
                "required": ["content", "memory_type", "project_path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let memory_type =
            crate::services::memory_store::parse_memory_type(&args.memory_type);

        let input = CreateMemoryInput {
            content: args.content,
            memory_type,
            project_path: args.project_path,
            source_session_id: None,
            importance: None,
        };

        let memory = self
            .memory_store
            .create(&input)
            .await
            .map_err(|e| VigilToolError(e.to_string()))?;

        Ok(json!({ "id": memory.id }))
    }
}

// ---------------------------------------------------------------------------
// MemoryDeleteTool
// ---------------------------------------------------------------------------

/// Delete a memory by ID.
pub(crate) struct MemoryDeleteTool {
    memory_store: MemoryStore,
}

impl MemoryDeleteTool {
    pub(crate) fn new(memory_store: MemoryStore) -> Self {
        Self { memory_store }
    }
}

#[derive(Deserialize)]
pub(crate) struct MemoryDeleteArgs {
    /// The UUID of the memory to delete.
    memory_id: String,
}

impl Tool for MemoryDeleteTool {
    const NAME: &'static str = "memory_delete";

    type Error = VigilToolError;
    type Args = MemoryDeleteArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Delete an outdated or incorrect memory by ID.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "memory_id": {
                        "type": "string",
                        "description": "The UUID of the memory to delete"
                    }
                },
                "required": ["memory_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.memory_store
            .delete(&args.memory_id)
            .await
            .map_err(|e| VigilToolError(e.to_string()))?;

        Ok(json!({ "ok": true }))
    }
}

// ---------------------------------------------------------------------------
// SessionRecallTool
// ---------------------------------------------------------------------------

/// Look up session details by ID or list recent sessions for a project.
pub(crate) struct SessionRecallTool {
    db: Arc<SqliteDb>,
}

impl SessionRecallTool {
    pub(crate) fn new(db: Arc<SqliteDb>) -> Self {
        Self { db }
    }
}

#[derive(Deserialize)]
pub(crate) struct SessionRecallArgs {
    /// If provided, fetch a single session by ID.
    session_id: Option<String>,
    /// If provided (and `session_id` is absent), list sessions for this project.
    project_path: Option<String>,
}

impl Tool for SessionRecallTool {
    const NAME: &'static str = "session_recall";

    type Error = VigilToolError;
    type Args = SessionRecallArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Look up session details. Provide session_id to get a single session, \
                          or project_path to list recent sessions for that project."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "UUID of a specific session to look up"
                    },
                    "project_path": {
                        "type": "string",
                        "description": "Absolute project path to list sessions for"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let store = SessionStore::new(Arc::clone(&self.db));

        if let Some(id) = &args.session_id {
            let session = store
                .get(id)
                .await
                .map_err(|e| VigilToolError(e.to_string()))?;

            return serde_json::to_value(&session)
                .map_err(|e| VigilToolError(e.to_string()));
        }

        // List all sessions and optionally filter by project.
        let sessions = store
            .list()
            .await
            .map_err(|e| VigilToolError(e.to_string()))?;

        let filtered: Vec<_> = match &args.project_path {
            Some(path) => sessions
                .into_iter()
                .filter(|s| s.project_path == *path)
                .collect(),
            None => sessions,
        };

        serde_json::to_value(&filtered).map_err(|e| VigilToolError(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// ActaUpdateTool
// ---------------------------------------------------------------------------

/// Update the project briefing (acta) stored in the KV store.
pub(crate) struct ActaUpdateTool {
    kv: KvStore,
}

impl ActaUpdateTool {
    pub(crate) fn new(kv: KvStore) -> Self {
        Self { kv }
    }
}

#[derive(Deserialize)]
pub(crate) struct ActaUpdateArgs {
    /// Absolute path of the project.
    project_path: String,
    /// The new acta content (~500 words).
    content: String,
}

impl Tool for ActaUpdateTool {
    const NAME: &'static str = "acta_update";

    type Error = VigilToolError;
    type Args = ActaUpdateArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Update the project briefing (acta). The acta is a ~500 word summary \
                          injected as context into every new session for this project."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "project_path": {
                        "type": "string",
                        "description": "Absolute path of the project"
                    },
                    "content": {
                        "type": "string",
                        "description": "The new acta content (~500 words)"
                    }
                },
                "required": ["project_path", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let key = format!("acta:{}", args.project_path);
        self.kv
            .set(&key, &args.content)
            .map_err(|e| VigilToolError(e.to_string()))?;

        Ok(json!({ "ok": true }))
    }
}

// ---------------------------------------------------------------------------
// SpawnWorkerTool
// ---------------------------------------------------------------------------

/// Spawn a worker session for parallel tasks.
///
/// Unlike most spawn tools, Vigil does not have a parent session, so
/// this tool creates a top-level queued session directly via the
/// session store rather than going through [`SubSessionService`].
pub(crate) struct SpawnWorkerTool {
    db: Arc<SqliteDb>,
    sub_session: SubSessionService,
}

impl SpawnWorkerTool {
    pub(crate) fn new(db: Arc<SqliteDb>, sub_session: SubSessionService) -> Self {
        Self { db, sub_session }
    }
}

#[derive(Deserialize)]
pub(crate) struct SpawnWorkerArgs {
    /// Absolute path of the project.
    project_path: String,
    /// The prompt/task for the worker session.
    prompt: String,
    /// Optional skill to apply to the worker.
    skill: Option<String>,
}

impl Tool for SpawnWorkerTool {
    const NAME: &'static str = "spawn_worker";

    type Error = VigilToolError;
    type Args = SpawnWorkerArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Spawn an independent worker session for parallel tasks. The worker \
                          runs in its own worktree. Use sparingly and only for clearly \
                          independent work."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "project_path": {
                        "type": "string",
                        "description": "Absolute path of the project"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "The task description for the worker session"
                    },
                    "skill": {
                        "type": "string",
                        "description": "Optional skill to apply (e.g. 'tdd', 'refactor')"
                    }
                },
                "required": ["project_path", "prompt"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Vigil does not have a parent session, so create a top-level
        // queued session directly via the session store.
        let store = SessionStore::new(Arc::clone(&self.db));
        let session_id = uuid::Uuid::new_v4().to_string();

        let input = crate::services::session_store::CreateSessionInput {
            project_path: args.project_path.clone(),
            prompt: args.prompt.clone(),
            skill: args.skill,
            role: None,
            parent_id: None,
            spawn_type: None,
            skip_permissions: None,
            pipeline_id: None,
        };

        store
            .create(&session_id, &input)
            .await
            .map_err(|e| VigilToolError(format!("failed to create session: {e}")))?;

        Ok(json!({
            "session_id": session_id,
            "project_path": args.project_path,
            "status": "queued"
        }))
    }
}

// ---------------------------------------------------------------------------
// System prompt rendering
// ---------------------------------------------------------------------------

/// Render the Vigil system prompt from the Jinja2 template.
///
/// # Panics
///
/// Panics if the embedded template is invalid (programming error).
pub(crate) fn render_system_prompt(project_path: &str) -> String {
    let template_src = include_str!("../../prompts/vigil.md.j2");

    let mut env = minijinja::Environment::new();
    env.add_template("vigil", template_src)
        .expect("embedded vigil template must be valid");

    let tmpl = env.get_template("vigil").expect("template was just added");
    tmpl.render(minijinja::context! { project_path })
        .expect("vigil template rendering must not fail")
}

// ---------------------------------------------------------------------------
// Agent builder
// ---------------------------------------------------------------------------

/// Dependencies required to build a Vigil agent.
///
/// This is a subset of [`AppDeps`] so that the builder does not depend on
/// the full dependency container, making it easier to test.
pub(crate) struct VigilDeps {
    pub memory_search: MemorySearch,
    pub memory_store: MemoryStore,
    pub db: Arc<SqliteDb>,
    pub kv: KvStore,
    pub sub_session: SubSessionService,
}

/// Build a `ToolSet` containing all Vigil tools.
///
/// Returns a `rig::tool::ToolSet` that can be used with an agent builder.
/// The actual agent construction (which requires a `rig::providers::anthropic::Client`)
/// is left to the caller so that the API key dependency is isolated.
pub(crate) fn build_vigil_toolset(deps: &VigilDeps) -> rig::tool::ToolSet {
    let mut toolset = rig::tool::ToolSet::default();

    toolset.add_tool(MemoryRecallTool {
        memory_search: deps.memory_search.clone(),
    });
    toolset.add_tool(MemorySaveTool {
        memory_store: deps.memory_store.clone(),
    });
    toolset.add_tool(MemoryDeleteTool {
        memory_store: deps.memory_store.clone(),
    });
    toolset.add_tool(SessionRecallTool {
        db: Arc::clone(&deps.db),
    });
    toolset.add_tool(ActaUpdateTool {
        kv: deps.kv.clone(),
    });
    toolset.add_tool(SpawnWorkerTool {
        db: Arc::clone(&deps.db),
        sub_session: deps.sub_session.clone(),
    });

    toolset
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_system_prompt_contains_project_path() {
        let prompt = render_system_prompt("/home/user/my-project");
        assert!(
            prompt.contains("/home/user/my-project"),
            "prompt should contain the project path"
        );
        assert!(
            prompt.contains("Vigil"),
            "prompt should mention Vigil"
        );
        assert!(
            prompt.contains("Observe"),
            "prompt should list responsibilities"
        );
    }

    #[test]
    fn render_system_prompt_escapes_special_chars() {
        // Ensure paths with special characters are rendered correctly.
        let prompt = render_system_prompt("/tmp/project with spaces & symbols");
        assert!(prompt.contains("/tmp/project with spaces & symbols"));
    }

    #[tokio::test]
    async fn memory_recall_tool_definition_is_valid() {
        let db = crate::db::sqlite::SqliteDb::connect(
            &std::path::PathBuf::from(":memory:"),
        )
        .await
        .unwrap();
        let lance_dir = tempfile::tempdir().unwrap();
        let lance =
            crate::db::lance::LanceDb::connect(lance_dir.path()).await.unwrap();
        let search = MemorySearch::new(Arc::new(db), lance);

        let tool = MemoryRecallTool {
            memory_search: search,
        };

        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "memory_recall");
        assert!(!def.description.is_empty());

        // Parameters should be a valid JSON object with "properties".
        let params = &def.parameters;
        assert!(params.get("properties").is_some());
        assert!(params["properties"].get("query").is_some());
        assert!(params["properties"].get("project_path").is_some());
    }

    #[tokio::test]
    async fn memory_save_tool_definition_is_valid() {
        let db = crate::db::sqlite::SqliteDb::connect(
            &std::path::PathBuf::from(":memory:"),
        )
        .await
        .unwrap();
        let lance_dir = tempfile::tempdir().unwrap();
        let lance =
            crate::db::lance::LanceDb::connect(lance_dir.path()).await.unwrap();
        let store = MemoryStore::new(Arc::new(db), lance);

        let tool = MemorySaveTool {
            memory_store: store,
        };

        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "memory_save");
        assert!(def.parameters["properties"].get("content").is_some());
        assert!(def.parameters["properties"].get("memory_type").is_some());
    }

    #[tokio::test]
    async fn memory_delete_tool_definition_is_valid() {
        let db = crate::db::sqlite::SqliteDb::connect(
            &std::path::PathBuf::from(":memory:"),
        )
        .await
        .unwrap();
        let lance_dir = tempfile::tempdir().unwrap();
        let lance =
            crate::db::lance::LanceDb::connect(lance_dir.path()).await.unwrap();
        let store = MemoryStore::new(Arc::new(db), lance);

        let tool = MemoryDeleteTool {
            memory_store: store,
        };

        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "memory_delete");
        assert!(def.parameters["properties"].get("memory_id").is_some());
    }

    #[tokio::test]
    async fn session_recall_tool_definition_is_valid() {
        let db = crate::db::sqlite::SqliteDb::connect(
            &std::path::PathBuf::from(":memory:"),
        )
        .await
        .unwrap();

        let tool = SessionRecallTool {
            db: Arc::new(db),
        };

        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "session_recall");
        assert!(def.parameters["properties"].get("session_id").is_some());
        assert!(def.parameters["properties"].get("project_path").is_some());
    }

    #[test]
    fn acta_update_tool_stores_and_retrieves() {
        let dir = tempfile::tempdir().unwrap();
        let kv = KvStore::open(&dir.path().join("test.redb")).unwrap();

        let tool = ActaUpdateTool { kv: kv.clone() };
        let args = ActaUpdateArgs {
            project_path: "/tmp/project".to_string(),
            content: "This is the project briefing.".to_string(),
        };

        // Use block_on since KvStore::set is synchronous but Tool::call is async.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.call(args)).unwrap();
        assert_eq!(result, json!({ "ok": true }));

        // Verify it was stored.
        let stored = kv.get("acta:/tmp/project").unwrap();
        assert_eq!(stored.as_deref(), Some("This is the project briefing."));
    }

    #[tokio::test]
    async fn spawn_worker_creates_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let config = crate::config::Config::for_testing(dir.path());
        config.ensure_dirs().unwrap();

        let db = Arc::new(
            crate::db::sqlite::SqliteDb::connect(&config.db_path)
                .await
                .unwrap(),
        );
        let event_bus = Arc::new(crate::events::EventBus::new(64));
        let sub_session = SubSessionService::new(Arc::clone(&db), event_bus);

        let tool = SpawnWorkerTool {
            db: Arc::clone(&db),
            sub_session,
        };
        let args = SpawnWorkerArgs {
            project_path: "/tmp/project".to_string(),
            prompt: "do something".to_string(),
            skill: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_ok(), "spawn_worker should succeed: {result:?}");
        let value = result.unwrap();
        assert_eq!(value["status"], "queued");
        assert_eq!(value["project_path"], "/tmp/project");
        assert!(value["session_id"].is_string(), "should have a session ID");

        // Verify the session was actually created in the database.
        let store = SessionStore::new(Arc::clone(&db));
        let session_id = value["session_id"].as_str().unwrap();
        let session = store.get(session_id).await.unwrap();
        assert!(session.is_some(), "session should exist in database");
        let session = session.unwrap();
        assert_eq!(session.prompt, "do something");
    }
}
