# Praefectus Rust Backend Rewrite + New Systems

**Date:** 2026-03-07
**Status:** Approved

## Summary

Rewrite the Praefectus backend from TypeScript (Fastify) to Rust (Axum), adding four new systems inspired by Spacebot's architecture: Memory Service, Sub-Session Orchestration, Lictor (Context Compaction), and Vigil (Project Overseer). The Next.js frontend stays unchanged — same REST/WS API contract.

## Motivation

Praefectus needs persistent memory, sub-session orchestration, context compaction, and a project-level overseer. These systems require true concurrency (tokio), efficient embedding search (LanceDB), and robust process management — all areas where Rust excels. Spacebot (spacedriveapp/spacebot) already proved this exact stack works.

## Architecture

### High-Level Structure

```
praefectus/
├── apps/
│   ├── daemon/              # Rust backend (replaces apps/server)
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── lib.rs
│   │   │   ├── api/         # Axum routes (REST + WS)
│   │   │   ├── services/    # Business logic
│   │   │   ├── db/          # SQLite (sqlx) + LanceDB + redb
│   │   │   ├── events/      # EventBus (tokio broadcast)
│   │   │   ├── hooks/       # Claude Code hook templates
│   │   │   ├── process/     # PTY + child process management
│   │   │   ├── llm/         # Rig agents (Vigil, compaction, memory)
│   │   │   └── prompts/     # Jinja2 templates (.md.j2)
│   │   └── Cargo.toml
│   └── web/                 # Next.js frontend (unchanged)
├── cli/                     # Rewrite in Rust (clap) — single binary
└── docs/
    └── plans/
```

### Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (edition 2024) |
| Async runtime | tokio 1.44 |
| HTTP/WS | axum 0.8 + tower-http 0.6 |
| Relational DB | SQLite via sqlx 0.8 |
| Vector DB | LanceDB 0.26 + fastembed 4 |
| KV store | redb 2.4 |
| LLM framework | rig-core 0.31 |
| CLI | clap 4.5 (derive) |
| Serialization | serde 1.0 + serde_json 1.0 |
| Error handling | thiserror 2.0 + anyhow 1.0 |
| Observability | tracing 0.1 + tracing-subscriber 0.3 |
| Templates | minijinja 2.8 |
| JSON Schema | schemars 1.2 |
| Hot config | arc-swap 1 |
| Cache | moka 0.12 |

### Three Databases

- **SQLite** (sqlx): Sessions, projects, pipelines, notifications, memory graph nodes/edges, Vigil state, session history
- **LanceDB**: Vector embeddings + full-text search (Tantivy) + hybrid search (RRF) for memory recall
- **redb**: Key-value settings + encrypted secrets (AES-256-GCM with Argon2id key derivation)

### Event-Driven Core

```
EventBus (tokio::broadcast)
  ├── SessionManager (existing, reimplemented)
  ├── MemoryService (NEW)
  ├── SubSessionOrchestrator (NEW)
  ├── Lictor (NEW) — context compaction
  ├── Vigil (NEW) — per-project overseer
  └── Notifier (existing, reimplemented)
```

### Dependency Bundle

```rust
#[derive(Clone)]
pub struct AppDeps {
    pub db: Arc<Db>,              // SQLite
    pub lance: Arc<LanceDb>,      // Vector DB
    pub kv: Arc<redb::Database>,  // KV store
    pub session_manager: Arc<SessionManager>,
    pub memory: Arc<MemoryService>,
    pub vigil: Arc<VigilService>,
    pub lictor: Arc<LictorService>,
    pub event_tx: broadcast::Sender<AppEvent>,
    pub shutdown_rx: watch::Receiver<bool>,
}
```

## Code Style

Follow Spacebot + ruff + uv patterns. See `rust-code-style` skill for full reference.

Key conventions:
- No `mod.rs` — file-as-module-root
- `pub(crate)` by default, minimal `pub` surface
- `thiserror` domain errors boxed in top-level `Error` enum, `anyhow` escape hatch
- Four-type tool convention: `{Name}Tool`, `{Name}Args`, `{Name}Output`, `{Name}Error`
- Prompts as external Jinja2 templates in `prompts/`
- Clippy pedantic enabled globally
- No abbreviations in names

## System 1: Memory Service

### Memory Types

```rust
pub enum MemoryType {
    Fact,        // "The auth module uses JWT with RS256"
    Decision,    // "We chose Axum over Actix for simplicity"
    Preference,  // "User prefers explicit error handling"
    Pattern,     // "All routes follow handler-service-repo pattern"
    Failure,     // "Approach X failed because Y"
    Todo,        // "Need to add rate limiting"
}
```

### Memory Graph Edges

```rust
pub enum MemoryEdge {
    RelatedTo,    // Loose association
    Updates,      // Newer supersedes older (similarity > 0.9)
    Contradicts,  // Conflicting information
    CausedBy,     // Causal chain
    PartOf,       // Hierarchical grouping
}
```

### Storage Split

- **SQLite**: Memory nodes (id, content, type, importance, project_id, created_at, accessed_at, access_count), edges (source_id, target_id, edge_type)
- **LanceDB**: Embedding vectors + full-text index for hybrid search

### Importance & Decay

- 0.0–1.0 score based on type defaults + access frequency + recency
- `Failure` memories never decay
- `Decision` and `Pattern` decay slowly
- Vigil prunes low-importance memories periodically

### Acta (Memory Bulletin)

~500 word briefing synthesized by Vigil from project memories. Cached via `ArcSwap`. Injected into every new session's preamble.

## System 2: Sub-Session Orchestration

### Two Types

- **Branch**: Fork of parent context. Read-only worktree access. Returns curated conclusion. For thinking/research.
- **Worker**: Independent task executor. Own worktree. Can modify files. For parallel implementation work.

### Database Schema Addition

```sql
ALTER TABLE sessions ADD COLUMN parent_id TEXT REFERENCES sessions(id);
ALTER TABLE sessions ADD COLUMN spawn_type TEXT CHECK(spawn_type IN ('branch', 'worker'));
ALTER TABLE sessions ADD COLUMN spawn_result TEXT;
```

### Spawn Flow

1. Parent calls `spawn_branch` or `spawn_worker` tool
2. Server creates child session with `parent_id`
3. Child runs concurrently
4. Child completes → result stored → parent retriggered with result injected
5. Retriggers debounced (100ms), capped (max 3 per turn)

### Status Block

Injected into parent session turns showing active children state.

### Limits

- Max branches per session: 3
- Max workers per session: 5
- Max total concurrent sessions: 10

## System 3: Lictor (Context Compaction)

### Tiered Thresholds

| Level | Threshold | Action |
|-------|-----------|--------|
| Background | >80% | Summarize oldest 30% via branch |
| Aggressive | >85% | Summarize oldest 50% via branch |
| Emergency | >95% | Hard truncation (no LLM) |

### Key Properties

- Programmatic monitor, not an LLM process
- Never interrupts running session
- Compaction branches run concurrently
- Context overflow errors caught and retried (up to 2 retries)
- Dropped content logged for post-session review

## System 4: Vigil (Project Overseer)

### Responsibilities

1. **Observes** — Subscribes to all session events for its project
2. **Remembers** — Extracts key memories from completed sessions
3. **Curates Acta** — Periodically refreshes the project briefing
4. **Coordinates** — Auto-triggers pipeline steps, manages cross-session dependencies
5. **Converses** — Chat endpoint for querying project history

### Implementation

Long-running Rig agent with tools: `MemoryRecallTool`, `MemorySaveTool`, `MemoryDeleteTool`, `SessionRecallTool`, `ActaUpdateTool`, `SpawnWorkerTool`.

System prompt in `prompts/vigil.md.j2`.

### Lifecycle

- One Vigil per registered project
- Starts lazily on first session
- Persists across server restarts
- Sleeps when idle, wakes on events

## API Surface

### Existing (reimplemented in Axum)

All current endpoints maintained with same contract.

### New Endpoints

```
# Memory
GET    /api/memory/{projectPath}           # List memories
POST   /api/memory/{projectPath}/search    # Hybrid search
POST   /api/memory                         # Create memory
DELETE /api/memory/{id}                    # Delete memory

# Vigil
GET    /api/vigil/{projectId}              # Vigil status
POST   /api/vigil/{projectId}/chat         # Chat with Vigil
GET    /api/vigil/{projectId}/acta         # Current Acta

# Sub-sessions
GET    /api/sessions/{id}/children         # List children
POST   /api/sessions/{id}/spawn            # Spawn branch/worker
```

### New WebSocket Events

- `memory_updated`, `acta_refreshed`
- `child_spawned`, `child_completed`
- `vigil_message`

## Implementation Order

Memory → Sub-sessions → Lictor → Vigil

Each layer builds on the previous. Memory is the foundation everything else uses.

## Frontend Changes

The Next.js frontend needs new pages/components but no architectural changes:

- Memory browser page per project
- Vigil chat panel
- Sub-session tree view on session detail page
- Acta display on project overview
- Lictor status indicator on active sessions
