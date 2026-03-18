-- Initial schema for the Praefectus daemon.

CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY,
    project_path    TEXT NOT NULL,
    worktree_path   TEXT,
    tmux_session    TEXT,
    prompt          TEXT NOT NULL,
    skills_used     TEXT,                -- JSON array
    status          TEXT NOT NULL DEFAULT 'queued',
    agent_type      TEXT NOT NULL DEFAULT 'claude',
    role            TEXT,
    parent_id       TEXT,
    spawn_type      TEXT,
    spawn_result    TEXT,
    retry_count     INTEGER DEFAULT 0,
    started_at      INTEGER,
    ended_at        INTEGER,
    exit_reason     TEXT,
    git_metadata    TEXT,                -- JSON object
    pipeline_id     TEXT,
    pipeline_step_index INTEGER
);

CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions (status);
CREATE INDEX IF NOT EXISTS idx_sessions_project_path ON sessions (project_path);
CREATE INDEX IF NOT EXISTS idx_sessions_parent_id ON sessions (parent_id);
CREATE INDEX IF NOT EXISTS idx_sessions_pipeline_id ON sessions (pipeline_id);

CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL,
    event_type  TEXT NOT NULL,
    tool_name   TEXT,
    payload     TEXT,
    timestamp   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_session_id ON events (session_id);
CREATE INDEX IF NOT EXISTS idx_events_event_type ON events (event_type);

CREATE TABLE IF NOT EXISTS projects (
    path         TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    skills_dir   TEXT,
    last_used_at INTEGER
);

CREATE TABLE IF NOT EXISTS chain_rules (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    trigger_event  TEXT NOT NULL,
    source_skill   TEXT,
    target_skill   TEXT NOT NULL,
    same_worktree  INTEGER DEFAULT 1
);

CREATE TABLE IF NOT EXISTS pipelines (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT DEFAULT '',
    steps       TEXT NOT NULL,           -- JSON array of PipelineStep
    edges       TEXT NOT NULL,           -- JSON array of PipelineEdge
    is_default  INTEGER DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL,
    type        TEXT NOT NULL,
    message     TEXT NOT NULL,
    sent_at     INTEGER,
    read_at     INTEGER
);

CREATE INDEX IF NOT EXISTS idx_notifications_session_id ON notifications (session_id);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memories (
    id                TEXT PRIMARY KEY,
    project_path      TEXT NOT NULL,
    memory_type       TEXT NOT NULL,
    content           TEXT NOT NULL,
    source_session_id TEXT,
    importance        REAL NOT NULL DEFAULT 0.5,
    access_count      INTEGER NOT NULL DEFAULT 0,
    created_at        INTEGER NOT NULL,
    accessed_at       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memories_project_path ON memories (project_path);
CREATE INDEX IF NOT EXISTS idx_memories_memory_type ON memories (memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_source_session_id ON memories (source_session_id);

CREATE TABLE IF NOT EXISTS memory_edges (
    id         TEXT PRIMARY KEY,
    source_id  TEXT NOT NULL,
    target_id  TEXT NOT NULL,
    edge_type  TEXT NOT NULL,
    weight     REAL NOT NULL DEFAULT 1.0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_edges_source_id ON memory_edges (source_id);
CREATE INDEX IF NOT EXISTS idx_memory_edges_target_id ON memory_edges (target_id);
