import Database from 'better-sqlite3';
import { drizzle } from 'drizzle-orm/better-sqlite3';
import * as schema from './schema.js';

export function createDb(dbPath: string): { sqlite: Database.Database; db: ReturnType<typeof drizzle<typeof schema>> } {
  const sqlite = new Database(dbPath);
  sqlite.pragma('journal_mode = WAL');
  sqlite.pragma('busy_timeout = 5000');
  sqlite.pragma('synchronous = NORMAL');
  return { sqlite, db: drizzle(sqlite, { schema }) };
}

export function initializeSchema(sqlite: Database.Database): void {
  sqlite.exec(`
    CREATE TABLE IF NOT EXISTS sessions (
      id TEXT PRIMARY KEY,
      project_path TEXT NOT NULL,
      worktree_path TEXT,
      tmux_session TEXT,
      prompt TEXT NOT NULL,
      skills_used TEXT,
      status TEXT NOT NULL DEFAULT 'queued',
      agent_type TEXT NOT NULL DEFAULT 'claude',
      role TEXT,
      parent_id TEXT,
      retry_count INTEGER DEFAULT 0,
      started_at INTEGER,
      ended_at INTEGER,
      exit_reason TEXT,
      git_metadata TEXT
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
      path TEXT PRIMARY KEY,
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
    CREATE TABLE IF NOT EXISTS notifications (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      session_id TEXT NOT NULL,
      type TEXT NOT NULL,
      message TEXT NOT NULL,
      sent_at INTEGER,
      read_at INTEGER
    );
    CREATE TABLE IF NOT EXISTS settings (
      key TEXT PRIMARY KEY,
      value TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS pipelines (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL,
      description TEXT DEFAULT '',
      steps TEXT NOT NULL,
      edges TEXT NOT NULL,
      is_default INTEGER DEFAULT 0,
      created_at INTEGER NOT NULL,
      updated_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
    CREATE INDEX IF NOT EXISTS idx_events_session_id ON events(session_id);
    CREATE INDEX IF NOT EXISTS idx_notifications_session_id ON notifications(session_id);
  `);

  // Migration: add git_metadata column to existing databases
  try {
    sqlite.exec(`ALTER TABLE sessions ADD COLUMN git_metadata TEXT`);
  } catch {
    // Column already exists — ignore
  }

  // Migration: add pipeline columns to sessions
  try {
    sqlite.exec(`ALTER TABLE sessions ADD COLUMN pipeline_id TEXT`);
  } catch {
    // Column already exists — ignore
  }
  try {
    sqlite.exec(`ALTER TABLE sessions ADD COLUMN pipeline_step_index INTEGER`);
  } catch {
    // Column already exists — ignore
  }
}

export type Db = ReturnType<typeof createDb>['db'];
