import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import Database from 'better-sqlite3';
import { drizzle } from 'drizzle-orm/better-sqlite3';
import { eq } from 'drizzle-orm';
import * as schema from '../db/schema.js';
import { CleanupService } from './cleanup.js';
import type { Db } from '../db/client.js';
import { WORKTREE_RETENTION_HOURS } from '@praefectus/shared';

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

const OLD_TIMESTAMP = Date.now() - (WORKTREE_RETENTION_HOURS + 1) * 60 * 60 * 1000;
const RECENT_TIMESTAMP = Date.now() - 1 * 60 * 60 * 1000; // 1 hour ago

function insertSession(db: Db, overrides: Partial<typeof schema.sessions.$inferInsert> = {}) {
  return db.insert(schema.sessions).values({
    id: 'sess-1',
    projectPath: '/tmp/project',
    prompt: 'test prompt',
    status: 'completed',
    agentType: 'claude',
    tmuxSession: 'pf-sess-1',
    startedAt: OLD_TIMESTAMP - 1000,
    endedAt: OLD_TIMESTAMP,
    worktreePath: '/tmp/worktrees/sess-1',
    ...overrides,
  }).returning().get();
}

function createMockWorktreeManager(hasChanges = false) {
  return {
    create: vi.fn(),
    remove: vi.fn().mockResolvedValue(undefined),
    removeAll: vi.fn(),
    hasUnmergedChanges: vi.fn().mockResolvedValue(hasChanges),
    worktreeBase: '/tmp/worktrees',
  };
}

describe('CleanupService', () => {
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

  it('should remove worktree for old completed session without changes', async () => {
    insertSession(db, {
      id: 'sess-old',
      status: 'completed',
      endedAt: OLD_TIMESTAMP,
      worktreePath: '/tmp/worktrees/sess-old',
    });

    const mockWt = createMockWorktreeManager(false);
    const cleanup = new CleanupService(db, mockWt as any);

    const result = await cleanup.cleanupWorktrees();

    expect(result.removed).toBe(1);
    expect(result.skipped).toBe(0);
    expect(mockWt.hasUnmergedChanges).toHaveBeenCalledWith('/tmp/worktrees/sess-old');
    expect(mockWt.remove).toHaveBeenCalledWith('/tmp/worktrees/sess-old');

    // worktreePath should be nulled out
    const session = db.select().from(schema.sessions).where(eq(schema.sessions.id, 'sess-old')).get();
    expect(session?.worktreePath).toBeNull();
  });

  it('should skip worktree with unmerged changes', async () => {
    insertSession(db, {
      id: 'sess-dirty',
      status: 'completed',
      endedAt: OLD_TIMESTAMP,
      worktreePath: '/tmp/worktrees/sess-dirty',
    });

    const mockWt = createMockWorktreeManager(true); // has changes
    const cleanup = new CleanupService(db, mockWt as any);

    const result = await cleanup.cleanupWorktrees();

    expect(result.removed).toBe(0);
    expect(result.skipped).toBe(1);
    expect(mockWt.remove).not.toHaveBeenCalled();

    // worktreePath should remain
    const session = db.select().from(schema.sessions).where(eq(schema.sessions.id, 'sess-dirty')).get();
    expect(session?.worktreePath).toBe('/tmp/worktrees/sess-dirty');
  });

  it('should skip sessions within retention period', async () => {
    insertSession(db, {
      id: 'sess-recent',
      status: 'completed',
      endedAt: RECENT_TIMESTAMP,
      worktreePath: '/tmp/worktrees/sess-recent',
    });

    const mockWt = createMockWorktreeManager(false);
    const cleanup = new CleanupService(db, mockWt as any);

    const result = await cleanup.cleanupWorktrees();

    expect(result.removed).toBe(0);
    expect(result.skipped).toBe(0);
    expect(mockWt.hasUnmergedChanges).not.toHaveBeenCalled();
  });

  it('should skip sessions without worktree_path', async () => {
    insertSession(db, {
      id: 'sess-nowt',
      status: 'completed',
      endedAt: OLD_TIMESTAMP,
      worktreePath: null,
    });

    const mockWt = createMockWorktreeManager(false);
    const cleanup = new CleanupService(db, mockWt as any);

    const result = await cleanup.cleanupWorktrees();

    expect(result.removed).toBe(0);
    expect(result.skipped).toBe(0);
  });

  it('should skip when worktree removal throws', async () => {
    insertSession(db, {
      id: 'sess-err',
      status: 'completed',
      endedAt: OLD_TIMESTAMP,
      worktreePath: '/tmp/worktrees/sess-err',
    });

    const mockWt = createMockWorktreeManager(false);
    mockWt.hasUnmergedChanges.mockRejectedValue(new Error('filesystem error'));
    const cleanup = new CleanupService(db, mockWt as any);

    const result = await cleanup.cleanupWorktrees();

    expect(result.removed).toBe(0);
    expect(result.skipped).toBe(1);
  });

  it('should handle mixed sessions correctly', async () => {
    // Old session, no changes -> remove
    insertSession(db, {
      id: 'sess-clean',
      status: 'completed',
      endedAt: OLD_TIMESTAMP,
      worktreePath: '/tmp/worktrees/sess-clean',
    });
    // Old session, has changes -> skip
    insertSession(db, {
      id: 'sess-dirty',
      status: 'failed',
      endedAt: OLD_TIMESTAMP,
      worktreePath: '/tmp/worktrees/sess-dirty',
    });
    // Recent session -> not selected
    insertSession(db, {
      id: 'sess-recent',
      status: 'completed',
      endedAt: RECENT_TIMESTAMP,
      worktreePath: '/tmp/worktrees/sess-recent',
    });

    const mockWt = createMockWorktreeManager(false);
    mockWt.hasUnmergedChanges.mockImplementation(async (path: string) => {
      return path.includes('dirty');
    });
    const cleanup = new CleanupService(db, mockWt as any);

    const result = await cleanup.cleanupWorktrees();

    expect(result.removed).toBe(1);
    expect(result.skipped).toBe(1);
  });

  it('should return zeros when no eligible sessions exist', async () => {
    const mockWt = createMockWorktreeManager(false);
    const cleanup = new CleanupService(db, mockWt as any);

    const result = await cleanup.cleanupWorktrees();

    expect(result.removed).toBe(0);
    expect(result.skipped).toBe(0);
  });
});
