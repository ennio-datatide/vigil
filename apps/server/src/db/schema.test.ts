import Database from 'better-sqlite3';
import { drizzle } from 'drizzle-orm/better-sqlite3';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import * as schema from './schema.js';

describe('database schema', () => {
  let sqlite: Database.Database;
  let db: ReturnType<typeof drizzle>;

  beforeEach(() => {
    sqlite = new Database(':memory:');
    db = drizzle(sqlite, { schema });
    // Apply schema via push (for tests)
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
    `);
  });

  afterEach(() => {
    sqlite.close();
  });

  it('should insert and query a session', () => {
    const result = db
      .insert(schema.sessions)
      .values({
        id: 'test-123',
        projectPath: '/tmp/test-project',
        prompt: 'Add auth middleware',
        status: 'queued',
        agentType: 'claude',
      })
      .returning()
      .get();

    expect(result.id).toBe('test-123');
    expect(result.status).toBe('queued');
  });

  it('should insert and query events', () => {
    db.insert(schema.sessions)
      .values({
        id: 'sess-1',
        projectPath: '/tmp/test',
        prompt: 'test',
        status: 'running',
        agentType: 'claude',
      })
      .run();

    const event = db
      .insert(schema.events)
      .values({
        sessionId: 'sess-1',
        eventType: 'PostToolUse',
        toolName: 'Bash',
        payload: '{"command": "ls"}',
        timestamp: Date.now(),
      })
      .returning()
      .get();

    expect(event.eventType).toBe('PostToolUse');
    expect(event.toolName).toBe('Bash');
  });

  it('should insert and query projects', () => {
    const project = db
      .insert(schema.projects)
      .values({
        path: '/tmp/my-project',
        name: 'My Project',
      })
      .returning()
      .get();

    expect(project.name).toBe('My Project');
  });
});
