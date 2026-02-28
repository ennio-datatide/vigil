import Database from 'better-sqlite3';
import { eq } from 'drizzle-orm';
import { drizzle } from 'drizzle-orm/better-sqlite3';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Db } from '../db/client.js';
import * as schema from '../db/schema.js';
import { EventBus } from './event-bus.js';
import { SessionManager } from './session-manager.js';

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
  return db
    .insert(schema.sessions)
    .values({
      id: 'sess-1',
      projectPath: '/tmp/project',
      prompt: 'test prompt',
      status: 'running',
      agentType: 'claude',
      tmuxSession: 'pf-sess-1',
      startedAt: Date.now(),
      ...overrides,
    })
    .returning()
    .get();
}

describe('SessionManager', () => {
  let sqlite: Database.Database;
  let db: Db;
  let bus: EventBus;
  let manager: SessionManager;

  beforeEach(() => {
    const testDb = createTestDb();
    sqlite = testDb.sqlite;
    db = testDb.db as Db;
    bus = new EventBus();
    manager = new SessionManager(bus, db);
  });

  afterEach(() => {
    manager.stop();
    sqlite.close();
  });

  describe('Stop event handling', () => {
    it('should mark session as completed on Stop event with no errors', () => {
      manager.start();
      insertSession(db, { id: 'sess-1', status: 'running' });

      bus.emit('hook_event', {
        sessionId: 'sess-1',
        eventType: 'Stop',
        toolName: null,
        payload: { reason: 'done' },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-1'))
        .get();
      expect(session?.status).toBe('completed');
      expect(session?.endedAt).toBeDefined();
      expect(session?.endedAt).toBeGreaterThan(0);
      expect(session?.exitReason).toBe('completed');
    });

    it('should mark session as auth_required on Stop event with auth error', () => {
      manager.start();
      insertSession(db, { id: 'sess-auth', status: 'running' });

      bus.emit('hook_event', {
        sessionId: 'sess-auth',
        eventType: 'Stop',
        toolName: null,
        payload: { error: 'Authentication token expired. Please login again.' },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-auth'))
        .get();
      expect(session?.status).toBe('auth_required');
    });

    it('should detect auth errors with "unauthorized" keyword', () => {
      manager.start();
      insertSession(db, { id: 'sess-unauth', status: 'running' });

      bus.emit('hook_event', {
        sessionId: 'sess-unauth',
        eventType: 'Stop',
        toolName: null,
        payload: { message: 'Unauthorized access detected' },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-unauth'))
        .get();
      expect(session?.status).toBe('auth_required');
    });

    it('should detect auth errors with "login" keyword', () => {
      manager.start();
      insertSession(db, { id: 'sess-login', status: 'running' });

      bus.emit('hook_event', {
        sessionId: 'sess-login',
        eventType: 'Stop',
        toolName: null,
        payload: { error: 'Please login to continue' },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-login'))
        .get();
      expect(session?.status).toBe('auth_required');
    });

    it('should emit auth_error on bus when auth error detected', () => {
      manager.start();
      insertSession(db, { id: 'sess-auth-emit', status: 'running', agentType: 'claude' });
      const authHandler = vi.fn();
      bus.on('auth_error', authHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-auth-emit',
        eventType: 'Stop',
        toolName: null,
        payload: { error: 'Token expired' },
        timestamp: Date.now(),
      });

      expect(authHandler).toHaveBeenCalledWith({
        agentType: 'claude',
        sessionId: 'sess-auth-emit',
      });
    });

    it('should pause all other running sessions of the same agent type on auth error', () => {
      manager.start();
      insertSession(db, { id: 'sess-fail', status: 'running', agentType: 'claude' });
      insertSession(db, { id: 'sess-sibling-1', status: 'running', agentType: 'claude' });
      insertSession(db, { id: 'sess-sibling-2', status: 'running', agentType: 'claude' });
      insertSession(db, { id: 'sess-other-type', status: 'running', agentType: 'gemini' });

      bus.emit('hook_event', {
        sessionId: 'sess-fail',
        eventType: 'Stop',
        toolName: null,
        payload: { error: 'Authentication token expired' },
        timestamp: Date.now(),
      });

      const failSession = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-fail'))
        .get();
      expect(failSession?.status).toBe('auth_required');

      const sibling1 = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-sibling-1'))
        .get();
      expect(sibling1?.status).toBe('auth_required');
      expect(sibling1?.endedAt).toBeDefined();
      expect(sibling1?.endedAt).toBeGreaterThan(0);

      const sibling2 = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-sibling-2'))
        .get();
      expect(sibling2?.status).toBe('auth_required');
      expect(sibling2?.endedAt).toBeDefined();
      expect(sibling2?.endedAt).toBeGreaterThan(0);

      const otherType = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-other-type'))
        .get();
      expect(otherType?.status).toBe('running');
    });
  });

  describe('Chain trigger on Stop', () => {
    it('should insert follow-up session when chain rule matches', () => {
      manager.start();
      insertSession(db, {
        id: 'sess-chain',
        status: 'running',
        skillsUsed: JSON.stringify(['implement']),
        projectPath: '/tmp/project',
      });

      db.insert(schema.chainRules)
        .values({
          triggerEvent: 'Stop',
          sourceSkill: 'implement',
          targetSkill: 'review',
          sameWorktree: 1,
        })
        .run();

      const updateHandler = vi.fn();
      bus.on('session_update', updateHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-chain',
        eventType: 'Stop',
        toolName: null,
        payload: { reason: 'done' },
        timestamp: Date.now(),
      });

      // A new session record should exist in DB
      const allSessions = db.select().from(schema.sessions).all();
      const followUp = allSessions.find((s) => s.id !== 'sess-chain');
      expect(followUp).toBeDefined();
      expect(followUp?.parentId).toBe('sess-chain');
      expect(followUp?.status).toBe('queued');
      expect(followUp?.skillsUsed).toBe(JSON.stringify(['review']));
    });

    it('should not create follow-up when no chain rule matches', () => {
      manager.start();
      insertSession(db, {
        id: 'sess-nochain',
        status: 'running',
        skillsUsed: JSON.stringify(['implement']),
      });

      db.insert(schema.chainRules)
        .values({
          triggerEvent: 'SessionStart',
          sourceSkill: 'implement',
          targetSkill: 'review',
          sameWorktree: 1,
        })
        .run();

      bus.emit('hook_event', {
        sessionId: 'sess-nochain',
        eventType: 'Stop',
        toolName: null,
        payload: { reason: 'done' },
        timestamp: Date.now(),
      });

      const allSessions = db.select().from(schema.sessions).all();
      expect(allSessions).toHaveLength(1); // no follow-up created
    });

    it('should insert follow-up for wildcard source skill (null)', () => {
      manager.start();
      insertSession(db, {
        id: 'sess-wildchain',
        status: 'running',
        skillsUsed: JSON.stringify(['anything']),
        projectPath: '/tmp/wildcard-project',
      });

      db.insert(schema.chainRules)
        .values({
          triggerEvent: 'Stop',
          sourceSkill: null,
          targetSkill: 'cleanup',
          sameWorktree: 0,
        })
        .run();

      bus.emit('hook_event', {
        sessionId: 'sess-wildchain',
        eventType: 'Stop',
        toolName: null,
        payload: {},
        timestamp: Date.now(),
      });

      // A new session record should exist in DB for the follow-up
      const allSessions = db.select().from(schema.sessions).all();
      const followUp = allSessions.find((s) => s.id !== 'sess-wildchain');
      expect(followUp).toBeDefined();
      expect(followUp?.projectPath).toBe('/tmp/wildcard-project');
      expect(followUp?.prompt).toBe('Run skill: cleanup');
      expect(followUp?.skillsUsed).toBe(JSON.stringify(['cleanup']));
    });

    it('should NOT fire chain rules when Stop event contains auth errors', () => {
      manager.start();
      insertSession(db, {
        id: 'sess-auth-chain',
        status: 'running',
        skillsUsed: JSON.stringify(['implement']),
        projectPath: '/tmp/project',
      });

      db.insert(schema.chainRules)
        .values({
          triggerEvent: 'Stop',
          sourceSkill: 'implement',
          targetSkill: 'review',
          sameWorktree: 1,
        })
        .run();

      bus.emit('hook_event', {
        sessionId: 'sess-auth-chain',
        eventType: 'Stop',
        toolName: null,
        payload: { error: 'Authentication token expired. Please login again.' },
        timestamp: Date.now(),
      });

      // No follow-up should be created for auth errors
      const allSessions = db.select().from(schema.sessions).all();
      expect(allSessions).toHaveLength(1);

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-auth-chain'))
        .get();
      expect(session?.status).toBe('auth_required');
    });
  });

  describe('Process exit fallback', () => {
    it('should mark session as completed on exit code 0 when still running', () => {
      insertSession(db, { id: 'sess-exit-ok', status: 'running' });

      manager.handleProcessExit('sess-exit-ok', 0);

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-exit-ok'))
        .get();
      expect(session?.status).toBe('completed');
      expect(session?.exitReason).toBe('completed');
    });

    it('should mark session as failed on non-zero exit code', () => {
      insertSession(db, { id: 'sess-exit-fail', status: 'running' });

      manager.handleProcessExit('sess-exit-fail', 1);

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-exit-fail'))
        .get();
      expect(session?.status).toBe('failed');
      expect(session?.exitReason).toBe('error');
    });

    it('should skip if Stop hook already handled the session', () => {
      insertSession(db, { id: 'sess-already-done', status: 'completed' });

      manager.handleProcessExit('sess-already-done', 0);

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-already-done'))
        .get();
      expect(session?.status).toBe('completed');
    });

    it('should skip if session does not exist', () => {
      expect(() => manager.handleProcessExit('nonexistent', 0)).not.toThrow();
    });
  });

  describe('Notification hook event', () => {
    it('should mark session as needs_input on Notification with needs_input type', () => {
      manager.start();
      insertSession(db, { id: 'sess-input', status: 'running' });

      const notificationHandler = vi.fn();
      bus.on('notification', notificationHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-input',
        eventType: 'Notification',
        toolName: null,
        payload: { type: 'needs_input', message: 'Waiting for user confirmation' },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-input'))
        .get();
      expect(session?.status).toBe('needs_input');

      expect(notificationHandler).toHaveBeenCalledWith({
        sessionId: 'sess-input',
        type: 'needs_input',
        message: 'Waiting for user confirmation',
      });
    });

    it('should emit notification for non-needs_input types without changing status', () => {
      manager.start();
      insertSession(db, { id: 'sess-info', status: 'running' });

      const notificationHandler = vi.fn();
      bus.on('notification', notificationHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-info',
        eventType: 'Notification',
        toolName: null,
        payload: { type: 'error', message: 'Something went wrong' },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-info'))
        .get();
      expect(session?.status).toBe('running');

      expect(notificationHandler).toHaveBeenCalledWith({
        sessionId: 'sess-info',
        type: 'error',
        message: 'Something went wrong',
      });
    });

    it('should mark session as needs_input on Notification with notification_type elicitation_dialog', () => {
      manager.start();
      insertSession(db, { id: 'sess-elicit', status: 'running' });

      const notificationHandler = vi.fn();
      bus.on('notification', notificationHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-elicit',
        eventType: 'Notification',
        toolName: null,
        payload: {
          hook_event_name: 'Notification',
          notification_type: 'elicitation_dialog',
          message: 'Claude needs your input',
          title: 'Input needed',
        },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-elicit'))
        .get();
      expect(session?.status).toBe('needs_input');

      expect(notificationHandler).toHaveBeenCalledWith({
        sessionId: 'sess-elicit',
        type: 'needs_input',
        message: 'Claude needs your input',
      });
    });

    it('should mark session as needs_input on Notification with notification_type permission_prompt', () => {
      manager.start();
      insertSession(db, { id: 'sess-perm', status: 'running' });

      const notificationHandler = vi.fn();
      bus.on('notification', notificationHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-perm',
        eventType: 'Notification',
        toolName: null,
        payload: {
          hook_event_name: 'Notification',
          notification_type: 'permission_prompt',
          message: 'Claude needs your permission to use Bash',
          title: 'Permission needed',
        },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-perm'))
        .get();
      expect(session?.status).toBe('needs_input');

      expect(notificationHandler).toHaveBeenCalledWith({
        sessionId: 'sess-perm',
        type: 'needs_input',
        message: 'Claude needs your permission to use Bash',
      });
    });

    it('should mark session as needs_input on Notification with notification_type idle_prompt', () => {
      manager.start();
      insertSession(db, { id: 'sess-idle', status: 'running' });

      const notificationHandler = vi.fn();
      bus.on('notification', notificationHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-idle',
        eventType: 'Notification',
        toolName: null,
        payload: {
          hook_event_name: 'Notification',
          notification_type: 'idle_prompt',
          message: 'Claude has been idle',
        },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-idle'))
        .get();
      expect(session?.status).toBe('needs_input');

      expect(notificationHandler).toHaveBeenCalledWith({
        sessionId: 'sess-idle',
        type: 'needs_input',
        message: 'Claude has been idle',
      });
    });

    it('should not change session status for auth_success notification_type', () => {
      manager.start();
      insertSession(db, { id: 'sess-auth-ok', status: 'running' });

      const notificationHandler = vi.fn();
      bus.on('notification', notificationHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-auth-ok',
        eventType: 'Notification',
        toolName: null,
        payload: {
          hook_event_name: 'Notification',
          notification_type: 'auth_success',
          message: 'Authentication successful',
        },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-auth-ok'))
        .get();
      expect(session?.status).toBe('running');

      expect(notificationHandler).toHaveBeenCalledWith({
        sessionId: 'sess-auth-ok',
        type: 'auth_success',
        message: 'Authentication successful',
      });
    });
  });

  describe('PreToolUse AskUserQuestion detection', () => {
    it('should mark session as needs_input when PreToolUse fires for AskUserQuestion', () => {
      manager.start();
      insertSession(db, { id: 'sess-ask', status: 'running' });

      const notificationHandler = vi.fn();
      bus.on('notification', notificationHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-ask',
        eventType: 'PreToolUse',
        toolName: 'AskUserQuestion',
        payload: {
          hook_event_name: 'PreToolUse',
          tool_name: 'AskUserQuestion',
          tool_input: {
            questions: [
              {
                question: 'Which approach do you prefer?',
                options: [
                  { label: 'Option A', description: 'First approach' },
                  { label: 'Option B', description: 'Second approach' },
                ],
              },
            ],
          },
        },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-ask'))
        .get();
      expect(session?.status).toBe('needs_input');

      expect(notificationHandler).toHaveBeenCalledWith({
        sessionId: 'sess-ask',
        type: 'needs_input',
        message: 'Which approach do you prefer?',
      });
    });

    it('should not change status for PreToolUse with non-AskUserQuestion tools', () => {
      manager.start();
      insertSession(db, { id: 'sess-bash', status: 'running' });

      const notificationHandler = vi.fn();
      bus.on('notification', notificationHandler);

      bus.emit('hook_event', {
        sessionId: 'sess-bash',
        eventType: 'PreToolUse',
        toolName: 'Bash',
        payload: {
          hook_event_name: 'PreToolUse',
          tool_name: 'Bash',
          tool_input: { command: 'npm test' },
        },
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-bash'))
        .get();
      expect(session?.status).toBe('running');
      expect(notificationHandler).not.toHaveBeenCalled();
    });
  });

  describe('start and stop lifecycle', () => {
    it('should start listening for events on start()', () => {
      manager.start();
      insertSession(db, { id: 'sess-lifecycle', status: 'running' });

      bus.emit('hook_event', {
        sessionId: 'sess-lifecycle',
        eventType: 'Stop',
        toolName: null,
        payload: {},
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-lifecycle'))
        .get();
      expect(session?.status).toBe('completed');
    });

    it('should stop listening for events on stop()', () => {
      manager.start();
      manager.stop();

      insertSession(db, { id: 'sess-stopped', status: 'running' });

      bus.emit('hook_event', {
        sessionId: 'sess-stopped',
        eventType: 'Stop',
        toolName: null,
        payload: {},
        timestamp: Date.now(),
      });

      const session = db
        .select()
        .from(schema.sessions)
        .where(eq(schema.sessions.id, 'sess-stopped'))
        .get();
      expect(session?.status).toBe('running');
    });

    it('should be safe to call stop() multiple times', () => {
      manager.start();
      expect(() => {
        manager.stop();
        manager.stop();
      }).not.toThrow();
    });

    it('should be safe to call start() when already started', () => {
      expect(() => {
        manager.start();
        manager.start();
      }).not.toThrow();
    });
  });
});
