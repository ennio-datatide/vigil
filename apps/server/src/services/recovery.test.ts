import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import Database from 'better-sqlite3';
import { drizzle } from 'drizzle-orm/better-sqlite3';
import { eq } from 'drizzle-orm';
import * as schema from '../db/schema.js';
import { RecoveryService } from './recovery.js';
import type { Db } from '../db/client.js';

function createTestDb() {
  const sqlite = new Database(':memory:');
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
  const db = drizzle(sqlite, { schema });
  return { sqlite, db };
}

function insertSession(db: Db, overrides: Partial<typeof schema.sessions.$inferInsert> = {}) {
  return db.insert(schema.sessions).values({
    id: 'sess-1',
    projectPath: '/tmp/project',
    prompt: 'test prompt',
    status: 'running',
    agentType: 'claude',
    startedAt: Date.now(),
    ...overrides,
  }).returning().get();
}

describe('RecoveryService', () => {
  let sqlite: Database.Database;
  let db: Db;

  beforeEach(() => {
    const testDb = createTestDb();
    sqlite = testDb.sqlite;
    db = testDb.db as Db;
  });

  afterEach(() => {
    sqlite.close();
  });

  it('should mark all running sessions as interrupted after restart', async () => {
    insertSession(db, { id: 'sess-1', status: 'running' });
    insertSession(db, { id: 'sess-2', status: 'running' });

    const recovery = new RecoveryService(db);
    const result = await recovery.recover();

    expect(result.interrupted).toBe(2);

    const sess1 = db.select().from(schema.sessions).where(eq(schema.sessions.id, 'sess-1')).get();
    expect(sess1?.status).toBe('interrupted');
    expect(sess1?.endedAt).toBeGreaterThan(0);
    expect(sess1?.exitReason).toBe('error');

    const sess2 = db.select().from(schema.sessions).where(eq(schema.sessions.id, 'sess-2')).get();
    expect(sess2?.status).toBe('interrupted');
  });

  it('should return zero when no running sessions exist', async () => {
    insertSession(db, { id: 'sess-done', status: 'completed' });

    const recovery = new RecoveryService(db);
    const result = await recovery.recover();

    expect(result.interrupted).toBe(0);
  });

  it('should not affect completed or queued sessions', async () => {
    insertSession(db, { id: 'sess-completed', status: 'completed' });
    insertSession(db, { id: 'sess-queued', status: 'queued' });
    insertSession(db, { id: 'sess-running', status: 'running' });

    const recovery = new RecoveryService(db);
    const result = await recovery.recover();

    expect(result.interrupted).toBe(1);

    const completed = db.select().from(schema.sessions).where(eq(schema.sessions.id, 'sess-completed')).get();
    expect(completed?.status).toBe('completed');

    const queued = db.select().from(schema.sessions).where(eq(schema.sessions.id, 'sess-queued')).get();
    expect(queued?.status).toBe('queued');

    const running = db.select().from(schema.sessions).where(eq(schema.sessions.id, 'sess-running')).get();
    expect(running?.status).toBe('interrupted');
  });
});
