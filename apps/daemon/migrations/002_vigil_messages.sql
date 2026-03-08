-- Vigil chat message history.

CREATE TABLE IF NOT EXISTS vigil_messages (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    role           TEXT NOT NULL,
    content        TEXT NOT NULL,
    embedded_cards TEXT,             -- JSON array of embedded card descriptors
    created_at     INTEGER NOT NULL
);
