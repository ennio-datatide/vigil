# Praefectus Rust Backend Rewrite — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rewrite the Praefectus backend from TypeScript/Fastify to Rust/Axum, adding Memory, Sub-Sessions, Lictor, and Vigil systems.

**Architecture:** Single Rust binary (Axum + sqlx + LanceDB + redb + Rig) replacing apps/server. Next.js frontend unchanged. Same REST/WS API contract. New endpoints for Memory, Vigil, and Sub-Sessions.

**Tech Stack:** Rust 2024, tokio 1.44, axum 0.8, sqlx 0.8 (SQLite), lancedb 0.26, fastembed 4, redb 2.4, rig-core 0.31, clap 4.5, minijinja 2.8, tracing 0.1

**Code Style:** Follow `rust-code-style` skill. No mod.rs, pub(crate) default, thiserror+anyhow errors, four-type tool convention.

---

## Phase 0: Scaffold the Rust Crate

### Task 0.1: Initialize Cargo Project

**Files:**
- Create: `apps/daemon/Cargo.toml`
- Create: `apps/daemon/src/main.rs`
- Create: `apps/daemon/src/lib.rs`

**Step 1: Create the daemon directory and Cargo.toml**

```bash
mkdir -p apps/daemon/src
```

```toml
# apps/daemon/Cargo.toml
[package]
name = "praefectus"
version = "0.1.0"
edition = "2024"
rust-version = "1.91"

[dependencies]
# Async runtime
tokio = { version = "1.44", features = ["full"] }
futures = "0.3"

# HTTP/WS
axum = { version = "0.8", features = ["ws", "multipart"] }
tower-http = { version = "0.6", features = ["cors", "fs"] }
reqwest = { version = "0.13", features = ["json", "rustls-tls"], default-features = false }

# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "migrate", "chrono", "uuid"] }
lancedb = "0.26"
fastembed = "4"
redb = "2.4"

# LLM
rig-core = { version = "0.31", features = ["derive"] }

# CLI
clap = { version = "4.5", features = ["derive"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Error handling
thiserror = "2.0"
anyhow = "1.0"

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# Templates
minijinja = "2.8"

# Utilities
uuid = { version = "1.15", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
arc-swap = "1"
moka = { version = "0.12", features = ["future"] }
schemars = "1.2"
rustc-hash = "2"
fs-err = "3"

# Telegram
teloxide = { version = "0.17", features = ["macros"], optional = true }

[features]
default = ["telegram"]
telegram = ["dep:teloxide"]

[dev-dependencies]
insta = { version = "1", features = ["json", "redactions"] }
test-case = "3"
tempfile = "3"
axum-test = "16"

[lints.rust]
unsafe_code = "warn"
unreachable_pub = "warn"

[lints.clippy]
pedantic = { level = "warn", priority = -2 }
missing_errors_doc = "allow"
missing_panics_doc = "allow"
must_use_candidate = "allow"
module_name_repetitions = "allow"
too_many_lines = "allow"
too_many_arguments = "allow"
print_stdout = "warn"
print_stderr = "warn"
dbg_macro = "warn"
```

**Step 2: Create main.rs and lib.rs stubs**

```rust
// apps/daemon/src/main.rs
use clap::Parser;

#[derive(Parser)]
#[command(name = "praefectus", about = "Command center for agentic coding sessions")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Start the daemon
    Daemon {
        #[arg(short, long, default_value = "8000")]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Daemon { port } => praefectus::run(port).await?,
    }
    Ok(())
}
```

```rust
// apps/daemon/src/lib.rs
//! Praefectus — Command center for agentic coding sessions.

pub mod api;
pub mod config;
pub mod db;
pub mod events;
pub mod hooks;
pub mod process;
pub mod services;

mod error;

pub use error::{Error, Result};

/// Start the Praefectus daemon on the given port.
pub async fn run(port: u16) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("praefectus=debug,info")
        .init();

    tracing::info!(port, "starting praefectus daemon");
    Ok(())
}
```

**Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`
Expected: Compiles with warnings about unused modules

**Step 4: Commit**

```bash
git add apps/daemon/
git commit -m "feat: scaffold Rust daemon crate with dependencies"
```

---

### Task 0.2: Error Types

**Files:**
- Create: `apps/daemon/src/error.rs`

**Step 1: Define top-level error hierarchy**

```rust
// apps/daemon/src/error.rs
//! Application-level error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(Box<ConfigError>),

    #[error(transparent)]
    Db(Box<DbError>),

    #[error(transparent)]
    Session(Box<SessionError>),

    #[error(transparent)]
    Memory(Box<MemoryError>),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing config: {0}")]
    Missing(String),

    #[error("invalid config: {0}")]
    Invalid(String),
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("query failed: {0}")]
    Query(#[from] sqlx::Error),

    #[error("migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session not found: {0}")]
    NotFound(String),

    #[error("invalid state transition: {from} -> {to}")]
    InvalidTransition { from: String, to: String },

    #[error("spawn failed: {0}")]
    SpawnFailed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory not found: {0}")]
    NotFound(String),

    #[error("embedding failed: {0}")]
    EmbeddingFailed(String),

    #[error("search failed: {0}")]
    SearchFailed(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Session(e) => match e.as_ref() {
                SessionError::NotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            },
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = serde_json::json!({ "error": self.to_string() });
        (status, axum::Json(body)).into_response()
    }
}
```

**Step 2: Verify it compiles**

Run: `cd apps/daemon && cargo check`

**Step 3: Commit**

```bash
git add apps/daemon/src/error.rs
git commit -m "feat: add hierarchical error types with Axum response impl"
```

---

### Task 0.3: Configuration

**Files:**
- Create: `apps/daemon/src/config.rs`

**Step 1: Define config resolution**

```rust
// apps/daemon/src/config.rs
//! Configuration resolution for Praefectus daemon.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub praefectus_home: PathBuf,
    pub db_path: PathBuf,
    pub skills_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub pid_file: PathBuf,
    pub worktree_base: PathBuf,
    pub server_port: u16,
    pub web_port: u16,
    pub api_token: Option<String>,
    pub dashboard_url: Option<String>,
}

impl Config {
    pub fn resolve(port: u16) -> anyhow::Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        let praefectus_home = home.join(".praefectus");

        Ok(Self {
            db_path: praefectus_home.join("praefectus.db"),
            skills_dir: praefectus_home.join("skills"),
            logs_dir: praefectus_home.join("logs"),
            pid_file: praefectus_home.join("daemon.pid"),
            worktree_base: praefectus_home.join("worktrees"),
            server_port: port,
            web_port: 3000,
            api_token: std::env::var("PRAEFECTUS_API_TOKEN").ok(),
            dashboard_url: std::env::var("PRAEFECTUS_DASHBOARD_URL").ok(),
            praefectus_home,
        })
    }

    /// Ensure all required directories exist.
    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        for dir in [
            &self.praefectus_home,
            &self.skills_dir,
            &self.logs_dir,
            &self.worktree_base,
        ] {
            fs_err::create_dir_all(dir)?;
        }
        Ok(())
    }
}
```

**Step 2: Verify**

Run: `cd apps/daemon && cargo check`

**Step 3: Commit**

```bash
git add apps/daemon/src/config.rs
git commit -m "feat: add config resolution with directory setup"
```

---

### Task 0.4: Database Layer (SQLite + Migrations)

**Files:**
- Create: `apps/daemon/src/db.rs`
- Create: `apps/daemon/src/db/sqlite.rs`
- Create: `apps/daemon/src/db/models.rs`
- Create: `apps/daemon/migrations/001_initial.sql`

**Step 1: Write the initial migration matching existing schema**

```sql
-- apps/daemon/migrations/001_initial.sql
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY NOT NULL,
    project_path TEXT NOT NULL,
    worktree_path TEXT,
    tmux_session TEXT,
    prompt TEXT NOT NULL,
    skills_used TEXT,
    status TEXT NOT NULL DEFAULT 'queued',
    agent_type TEXT NOT NULL DEFAULT 'claude',
    role TEXT,
    parent_id TEXT REFERENCES sessions(id),
    spawn_type TEXT CHECK(spawn_type IN ('branch', 'worker')),
    spawn_result TEXT,
    retry_count INTEGER DEFAULT 0,
    started_at INTEGER,
    ended_at INTEGER,
    exit_reason TEXT,
    git_metadata TEXT,
    pipeline_id TEXT,
    pipeline_step_index INTEGER
);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    tool_name TEXT,
    payload TEXT,
    timestamp INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    path TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    skills_dir TEXT,
    last_used_at INTEGER
);

CREATE TABLE IF NOT EXISTS chain_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trigger_event TEXT NOT NULL,
    source_skill TEXT,
    target_skill TEXT NOT NULL,
    same_worktree INTEGER DEFAULT 1
);

CREATE TABLE IF NOT EXISTS pipelines (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT DEFAULT '',
    steps TEXT NOT NULL,
    edges TEXT NOT NULL,
    is_default INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    type TEXT NOT NULL,
    message TEXT NOT NULL,
    sent_at INTEGER,
    read_at INTEGER
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

-- New: Memory system tables
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY NOT NULL,
    project_path TEXT NOT NULL,
    content TEXT NOT NULL,
    memory_type TEXT NOT NULL,
    importance REAL NOT NULL DEFAULT 0.5,
    access_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    accessed_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    target_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    edge_type TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(source_id, target_id, edge_type)
);

CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_path);
CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_id);
CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id);
CREATE INDEX IF NOT EXISTS idx_notifications_session ON notifications(session_id);
CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project_path);
CREATE INDEX IF NOT EXISTS idx_memory_edges_source ON memory_edges(source_id);
CREATE INDEX IF NOT EXISTS idx_memory_edges_target ON memory_edges(target_id);
```

**Step 2: Define Rust models**

```rust
// apps/daemon/src/db/models.rs
//! Database model types matching SQLite schema.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// --- Session ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    Implementer,
    Reviewer,
    Fixer,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    Completed,
    Error,
    UserCancelled,
    ChainTriggered,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpawnType {
    Branch,
    Worker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitMetadata {
    pub repo_name: String,
    pub branch: String,
    pub commit_hash: String,
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

// --- Project ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub path: String,
    pub name: String,
    pub skills_dir: Option<String>,
    pub last_used_at: Option<i64>,
}

// --- Pipeline ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStep {
    pub id: String,
    pub skill: String,
    pub label: String,
    pub agent: AgentType,
    pub prompt: String,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineEdge {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

// --- Notification ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    NeedsInput,
    Error,
    AuthRequired,
    ChainComplete,
    SessionDone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

// --- Memory ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    Fact,
    Decision,
    Preference,
    Pattern,
    Failure,
    Todo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEdgeType {
    RelatedTo,
    Updates,
    Contradicts,
    CausedBy,
    PartOf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    pub project_path: String,
    pub content: String,
    pub memory_type: MemoryType,
    pub importance: f64,
    pub access_count: i64,
    pub created_at: i64,
    pub accessed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEdge {
    pub id: i64,
    pub source_id: String,
    pub target_id: String,
    pub edge_type: MemoryEdgeType,
    pub created_at: i64,
}

// --- Event ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: i64,
    pub session_id: String,
    pub event_type: String,
    pub tool_name: Option<String>,
    pub payload: Option<String>,
    pub timestamp: i64,
}
```

**Step 3: Create SQLite client**

```rust
// apps/daemon/src/db/sqlite.rs
//! SQLite database connection and query helpers.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;

use crate::error::{DbError, Error};

pub struct SqliteDb {
    pool: SqlitePool,
}

impl SqliteDb {
    pub async fn connect(path: &Path) -> Result<Self, Error> {
        let url = format!("sqlite:{}?mode=rwc", path.display());
        let options = SqliteConnectOptions::from_str(&url)
            .map_err(|e| Error::Db(Box::new(DbError::Query(e))))?
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| Error::Db(Box::new(DbError::Query(e))))?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| Error::Db(Box::new(DbError::Migration(e))))?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
```

**Step 4: Create db module root**

```rust
// apps/daemon/src/db.rs
//! Database layer: SQLite, LanceDB, redb.

pub mod models;
pub mod sqlite;

pub use sqlite::SqliteDb;
```

**Step 5: Verify it compiles**

Run: `cd apps/daemon && cargo check`

**Step 6: Commit**

```bash
git add apps/daemon/src/db.rs apps/daemon/src/db/ apps/daemon/migrations/
git commit -m "feat: add SQLite schema, models, and database client"
```

---

### Task 0.5: Event Bus

**Files:**
- Create: `apps/daemon/src/events.rs`

**Step 1: Define event types and broadcast bus**

```rust
// apps/daemon/src/events.rs
//! Event bus using tokio::broadcast for decoupled service communication.

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::db::models::{Notification, Session};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    SessionUpdate {
        session: Session,
    },
    StatusChanged {
        session: Session,
        old_status: String,
        new_status: String,
        message: Option<String>,
    },
    HookEvent {
        session_id: String,
        event_type: String,
        tool_name: Option<String>,
        payload: Option<serde_json::Value>,
        timestamp: i64,
    },
    SessionSpawned {
        session_id: String,
        worktree_path: String,
        git_metadata: Option<serde_json::Value>,
    },
    SessionSpawnFailed {
        session_id: String,
        error: String,
    },
    SessionRemoved {
        session_id: String,
    },
    NotificationCreated {
        notification: Notification,
    },
    ChildSpawned {
        parent_id: String,
        child_id: String,
        spawn_type: String,
    },
    ChildCompleted {
        parent_id: String,
        child_id: String,
        result: Option<String>,
    },
    MemoryUpdated {
        project_path: String,
        memory_id: String,
        action: String,
    },
    ActaRefreshed {
        project_path: String,
    },
}

pub struct EventBus {
    sender: broadcast::Sender<AppEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn sender(&self) -> &broadcast::Sender<AppEvent> {
        &self.sender
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.sender.subscribe()
    }

    pub fn emit(&self, event: AppEvent) {
        // .ok() because receivers may not exist yet
        self.sender.send(event).ok();
    }
}
```

**Step 2: Verify**

Run: `cd apps/daemon && cargo check`

**Step 3: Commit**

```bash
git add apps/daemon/src/events.rs
git commit -m "feat: add event bus with tokio::broadcast"
```

---

### Task 0.6: App Dependencies Bundle + Server Bootstrap

**Files:**
- Create: `apps/daemon/src/deps.rs`
- Modify: `apps/daemon/src/lib.rs`

**Step 1: Create deps bundle**

```rust
// apps/daemon/src/deps.rs
//! Shared application dependencies bundle.

use std::sync::Arc;
use tokio::sync::watch;

use crate::config::Config;
use crate::db::SqliteDb;
use crate::events::EventBus;

#[derive(Clone)]
pub struct AppDeps {
    pub config: Arc<Config>,
    pub db: Arc<SqliteDb>,
    pub event_bus: Arc<EventBus>,
    pub shutdown_tx: Arc<watch::Sender<bool>>,
    pub shutdown_rx: watch::Receiver<bool>,
}

impl AppDeps {
    pub async fn new(config: Config) -> crate::Result<Self> {
        config.ensure_dirs()?;

        let db = SqliteDb::connect(&config.db_path).await?;
        let event_bus = EventBus::new(256);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            config: Arc::new(config),
            db: Arc::new(db),
            event_bus: Arc::new(event_bus),
            shutdown_tx: Arc::new(shutdown_tx),
            shutdown_rx,
        })
    }
}
```

**Step 2: Update lib.rs with full server bootstrap**

```rust
// apps/daemon/src/lib.rs
//! Praefectus — Command center for agentic coding sessions.

pub mod api;
pub mod config;
pub mod db;
pub mod deps;
pub mod events;
pub mod hooks;
pub mod process;
pub mod services;

mod error;

pub use error::{Error, Result};

use crate::config::Config;
use crate::deps::AppDeps;

/// Start the Praefectus daemon on the given port.
pub async fn run(port: u16) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "praefectus=debug,info".parse().unwrap()),
        )
        .init();

    let config = Config::resolve(port)?;
    let deps = AppDeps::new(config).await?;

    let app = api::router(deps.clone());

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!(port, "praefectus daemon listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(deps.shutdown_rx.clone()))
        .await?;

    Ok(())
}

async fn shutdown_signal(mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("received ctrl+c, shutting down");
        }
        _ = shutdown_rx.changed() => {
            tracing::info!("shutdown signal received");
        }
    }
}
```

**Step 3: Create API router stub**

```rust
// apps/daemon/src/api.rs
//! Axum route definitions.

pub mod health;

use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;

use crate::deps::AppDeps;

pub fn router(deps: AppDeps) -> Router {
    let api = Router::new()
        .route("/health", get(health::health));

    Router::new()
        .nest("/api", api)
        .route("/health", get(health::health))
        .layer(CorsLayer::permissive())
        .with_state(deps)
}
```

```rust
// apps/daemon/src/api/health.rs
//! Health check endpoint.

use axum::Json;
use serde_json::{json, Value};

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}
```

**Step 4: Create empty module stubs**

```rust
// apps/daemon/src/hooks.rs
//! Claude Code hook templates and installation.

// apps/daemon/src/process.rs
//! PTY and child process management.

// apps/daemon/src/services.rs
//! Business logic services.
```

**Step 5: Verify the server starts**

Run: `cd apps/daemon && cargo run -- daemon --port 8001`
Expected: "praefectus daemon listening" log, health endpoint responds

Run: `curl http://localhost:8001/health`
Expected: `{"status":"ok"}`

**Step 6: Commit**

```bash
git add apps/daemon/
git commit -m "feat: bootable Axum server with health endpoint and dependency injection"
```

---

## Phase 1: Port Existing API (Feature Parity)

### Task 1.1: Auth Middleware

**Files:**
- Create: `apps/daemon/src/api/middleware.rs`

**Step 1: Implement timing-safe auth check**

```rust
// apps/daemon/src/api/middleware.rs
//! Authentication middleware.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::deps::AppDeps;
use crate::Error;

pub async fn auth_middleware(
    State(deps): State<AppDeps>,
    request: Request,
    next: Next,
) -> Result<Response, Error> {
    let Some(expected_token) = &deps.config.api_token else {
        return Ok(next.run(request).await);
    };

    let token = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(String::from)
        .or_else(|| {
            request
                .uri()
                .query()
                .and_then(|q| {
                    url::form_urlencoded::parse(q.as_bytes())
                        .find(|(k, _)| k == "token")
                        .map(|(_, v)| v.into_owned())
                })
        });

    match token {
        Some(ref t) if constant_time_eq(t.as_bytes(), expected_token.as_bytes()) => {
            Ok(next.run(request).await)
        }
        _ => Err(Error::Unauthorized),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}
```

**Step 2: Verify**

Run: `cd apps/daemon && cargo check`

**Step 3: Commit**

```bash
git add apps/daemon/src/api/middleware.rs
git commit -m "feat: add auth middleware with timing-safe token comparison"
```

---

### Task 1.2: Sessions CRUD Routes

**Files:**
- Create: `apps/daemon/src/api/sessions.rs`
- Create: `apps/daemon/src/services/session_store.rs`

This is a large task. Implement all session endpoints:
- `GET /api/sessions` — list all
- `GET /api/sessions/:id` — get one
- `POST /api/sessions` — create (queued, async spawn later)
- `DELETE /api/sessions/:id` — cancel
- `DELETE /api/sessions/:id/remove` — permanent delete
- `POST /api/sessions/:id/restart` — restart in-place
- `POST /api/sessions/:id/resume` — resume with new child session

Each handler is a thin function that delegates to `SessionStore` for DB queries.

**Step 1: Write tests for session store**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_list_sessions() {
        let db = test_db().await;
        let store = SessionStore::new(db);

        let session = store.create(CreateSessionInput {
            project_path: "/tmp/test".into(),
            prompt: "Fix the bug".into(),
            ..Default::default()
        }).await.unwrap();

        assert_eq!(session.status, SessionStatus::Queued);

        let all = store.list().await.unwrap();
        assert_eq!(all.len(), 1);
    }
}
```

**Step 2: Implement SessionStore with all CRUD methods**

**Step 3: Implement route handlers**

**Step 4: Register routes in api.rs**

**Step 5: Run tests**

Run: `cd apps/daemon && cargo test session`

**Step 6: Commit**

```bash
git commit -m "feat: add sessions CRUD routes and store"
```

---

### Task 1.3: Projects CRUD Routes

**Files:**
- Create: `apps/daemon/src/api/projects.rs`
- Create: `apps/daemon/src/services/project_store.rs`

Same pattern as sessions: store + handlers for GET/POST/DELETE.

---

### Task 1.4: Events Ingestion Route

**Files:**
- Create: `apps/daemon/src/api/events.rs`

POST `/events` — receives hook payloads, persists to DB, emits to event bus. No auth required.

---

### Task 1.5: Notifications Routes

**Files:**
- Create: `apps/daemon/src/api/notifications.rs`
- Create: `apps/daemon/src/services/notification_store.rs`

GET list, PATCH mark read, PATCH read-all, POST test notification.

---

### Task 1.6: Skills Route

**Files:**
- Create: `apps/daemon/src/api/skills.rs`
- Create: `apps/daemon/src/services/skill_manager.rs`

GET `/api/skills` — list .md files in skills_dir.

---

### Task 1.7: Pipelines CRUD Routes

**Files:**
- Create: `apps/daemon/src/api/pipelines.rs`
- Create: `apps/daemon/src/services/pipeline_store.rs`

Full CRUD + default pipeline seeding.

---

### Task 1.8: Settings Routes (Telegram)

**Files:**
- Create: `apps/daemon/src/api/settings.rs`
- Create: `apps/daemon/src/services/settings_store.rs`

GET/PUT telegram config, POST test.

---

### Task 1.9: File System Route

**Files:**
- Create: `apps/daemon/src/api/filesystem.rs`

GET `/api/fs/dirs?prefix=...` — directory autocomplete.

---

### Task 1.10: Dashboard WebSocket

**Files:**
- Create: `apps/daemon/src/api/ws_dashboard.rs`

WebSocket at `/ws/dashboard`:
- On connect: send `state_sync` with all sessions
- Subscribe to event bus: forward `session_update`, `session_removed`, `notification`
- Handle disconnect cleanup

---

### Task 1.11: Terminal WebSocket

**Files:**
- Create: `apps/daemon/src/api/ws_terminal.rs`
- Create: `apps/daemon/src/process/pty_manager.rs`
- Create: `apps/daemon/src/process/output_manager.rs`

WebSocket at `/ws/terminal/:sessionId`:
- PTY status message on connect
- History replay from buffer/disk
- Bidirectional: input → PTY, PTY output → client
- Resize support

---

### Task 1.12: Agent Spawner

**Files:**
- Create: `apps/daemon/src/process/agent_spawner.rs`
- Create: `apps/daemon/src/hooks/installer.rs`

Core process management:
- Create worktrees (git worktree add)
- Install hooks (emit-event.sh template)
- Symlink skills
- Spawn `claude` binary with PTY
- Stream JSONL output to OutputManager
- Capture git metadata
- Emit session_spawned / session_spawn_failed events

---

### Task 1.13: Session Manager (Event Processing)

**Files:**
- Create: `apps/daemon/src/services/session_manager.rs`

Subscribe to hook_event on event bus. Process:
- Stop → completed / auth_required
- Notification → needs_input / create notification
- Pipeline step advancement
- Process exit handling

---

### Task 1.14: Telegram Notifier

**Files:**
- Create: `apps/daemon/src/services/notifier.rs`

Subscribe to status_changed events. Format and send Telegram messages.

---

### Task 1.15: Recovery + Cleanup Services

**Files:**
- Create: `apps/daemon/src/services/recovery.rs`
- Create: `apps/daemon/src/services/cleanup.rs`

Recovery: mark orphaned running sessions as interrupted on startup.
Cleanup: periodic worktree GC (24h retention, skip unmerged).

---

### Task 1.16: Wire Everything Together

**Files:**
- Modify: `apps/daemon/src/api.rs` — register all routes
- Modify: `apps/daemon/src/lib.rs` — start all services
- Modify: `apps/daemon/src/deps.rs` — add all services to deps

Full integration: all routes registered, all services started, event bus wired.

**Verification:** Run existing frontend against Rust backend — all pages should work.

---

## Phase 2: Memory Service

### Task 2.1: LanceDB Setup

**Files:**
- Create: `apps/daemon/src/db/lance.rs`

Initialize LanceDB, create memories table with vector + text columns, fastembed model loading.

---

### Task 2.2: redb Setup

**Files:**
- Create: `apps/daemon/src/db/kv.rs`

Initialize redb, define tables for settings and encrypted secrets.

---

### Task 2.3: Memory Store (SQLite + LanceDB)

**Files:**
- Create: `apps/daemon/src/services/memory/store.rs`

CRUD operations on memories table. Embedding generation on save. Auto-association (similarity > 0.9 → Updates edge, > 0.7 → RelatedTo).

---

### Task 2.4: Hybrid Search

**Files:**
- Create: `apps/daemon/src/services/memory/search.rs`

Vector search + FTS via LanceDB. RRF reranking. Return ranked memories with scores.

---

### Task 2.5: Memory API Routes

**Files:**
- Create: `apps/daemon/src/api/memory.rs`

GET list, POST search, POST create, DELETE. Wire to router.

---

### Task 2.6: Memory Importance & Decay

**Files:**
- Create: `apps/daemon/src/services/memory/decay.rs`

Periodic background task: decay importance based on type and access patterns. Prune memories below threshold.

---

## Phase 3: Sub-Session Orchestration

### Task 3.1: Spawn Branch / Worker Tools

**Files:**
- Create: `apps/daemon/src/services/sub_session.rs`

Extend session creation: support parent_id and spawn_type. Branch gets read-only worktree access. Worker gets own worktree.

---

### Task 3.2: Retrigger Mechanism

**Files:**
- Modify: `apps/daemon/src/services/session_manager.rs`

When child completes, emit ChildCompleted event. Parent session receives result injection. Debounce 100ms, cap 3 retriggers.

---

### Task 3.3: Status Block Injection

**Files:**
- Modify: `apps/daemon/src/process/agent_spawner.rs`

Inject active children status into parent session turns.

---

### Task 3.4: Sub-Session API Routes

**Files:**
- Create: `apps/daemon/src/api/sub_sessions.rs`

GET `/api/sessions/:id/children`, POST `/api/sessions/:id/spawn`.

---

## Phase 4: Lictor (Context Compaction)

### Task 4.1: Context Size Monitor

**Files:**
- Create: `apps/daemon/src/services/lictor.rs`

Subscribe to hook events. Track token count estimates from JSONL. Evaluate thresholds.

---

### Task 4.2: Compaction Actions

**Files:**
- Modify: `apps/daemon/src/services/lictor.rs`

Background (>80%): spawn branch to summarize oldest 30%.
Aggressive (>85%): summarize 50%.
Emergency (>95%): hard truncation.

---

### Task 4.3: Overflow Recovery

**Files:**
- Modify: `apps/daemon/src/services/lictor.rs`

Catch context overflow errors from hook events. Compact and retry (max 2).

---

## Phase 5: Vigil (Project Overseer)

### Task 5.1: Vigil Rig Agent Setup

**Files:**
- Create: `apps/daemon/src/llm/vigil.rs`
- Create: `apps/daemon/prompts/vigil.md.j2`

Define Vigil as a Rig agent with tools: MemoryRecall, MemorySave, MemoryDelete, SessionRecall, ActaUpdate, SpawnWorker.

---

### Task 5.2: Vigil Service Lifecycle

**Files:**
- Create: `apps/daemon/src/services/vigil.rs`

One Vigil per project. Lazy start on first session. Subscribe to all session events. Extract memories on session completion.

---

### Task 5.3: Acta Generation

**Files:**
- Modify: `apps/daemon/src/services/vigil.rs`

Periodic synthesis of project memories into ~500 word briefing. Cache via ArcSwap. Inject into new session preambles.

---

### Task 5.4: Vigil API Routes

**Files:**
- Create: `apps/daemon/src/api/vigil.rs`

GET status, POST chat, GET acta.

---

### Task 5.5: Dashboard WS Extensions

**Files:**
- Modify: `apps/daemon/src/api/ws_dashboard.rs`

Add new event types: memory_updated, acta_refreshed, child_spawned, child_completed, vigil_message.

---

## Phase 6: Integration & Polish

### Task 6.1: OpenAPI Spec Generation

Generate OpenAPI 3.1 spec from route definitions. Replace packages/shared Zod schemas with spec-driven types.

---

### Task 6.2: Frontend API Type Generation

Generate TypeScript types from OpenAPI spec for the Next.js frontend.

---

### Task 6.3: End-to-End Testing

Full lifecycle tests: create session → hook events → status changes → notifications → memory extraction → Vigil synthesis.

---

### Task 6.4: CLI Rewrite (clap)

Port cli/ commands to Rust clap subcommands in the same binary: `praefectus daemon`, `praefectus start`, `praefectus ls`, `praefectus status`, `praefectus cleanup`.

---

### Task 6.5: Build & Release

Configure cargo build profiles. Single binary output. Update Turborepo config to include Rust build.

---

## Summary

| Phase | Tasks | Description |
|-------|-------|-------------|
| 0 | 0.1–0.6 | Scaffold: crate, errors, config, DB, events, deps, health |
| 1 | 1.1–1.16 | Port existing API: all routes, WS, services, spawner |
| 2 | 2.1–2.6 | Memory: LanceDB, store, search, decay, API |
| 3 | 3.1–3.4 | Sub-sessions: branch/worker spawn, retrigger, API |
| 4 | 4.1–4.3 | Lictor: monitor, compact, recover |
| 5 | 5.1–5.5 | Vigil: agent, lifecycle, acta, API, WS |
| 6 | 6.1–6.5 | Polish: OpenAPI, types, E2E, CLI, release |

**Total: ~35 tasks across 7 phases.**
