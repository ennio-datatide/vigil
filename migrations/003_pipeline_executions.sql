CREATE TABLE IF NOT EXISTS pipeline_executions (
    id                  TEXT PRIMARY KEY,
    pipeline_id         TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'queued',
    initial_prompt      TEXT NOT NULL,
    project_path        TEXT NOT NULL,
    current_step_index  INTEGER NOT NULL DEFAULT 0,
    step_sessions       TEXT NOT NULL DEFAULT '{}',
    step_outputs        TEXT NOT NULL DEFAULT '{}',
    created_at          INTEGER NOT NULL,
    completed_at        INTEGER
);

CREATE INDEX IF NOT EXISTS idx_pipeline_executions_pipeline_id ON pipeline_executions (pipeline_id);
CREATE INDEX IF NOT EXISTS idx_pipeline_executions_status ON pipeline_executions (status);
