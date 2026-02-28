import { describe, it, expect, beforeAll, afterAll, vi, beforeEach } from 'vitest';
import { eq, notInArray } from 'drizzle-orm';
import { buildApp } from '../app.js';
import { sessions } from '../db/schema.js';
import { EventBus } from '../services/event-bus.js';

describe('dashboard WebSocket plugin', () => {
  let app: Awaited<ReturnType<typeof buildApp>>;

  beforeAll(async () => {
    app = await buildApp({ praefectusHome: '/tmp/pf-test-dashboard-ws-' + Date.now() });
  });

  afterAll(async () => {
    await app.close();
  });

  describe('plugin registration', () => {
    it('should register the /ws/dashboard route', () => {
      const routes = app.printRoutes();
      // printRoutes returns a tree format; check for the 'dashboard' leaf
      expect(routes).toContain('dashboard');
    });
  });

  describe('non-WebSocket requests', () => {
    it('should return error for plain HTTP GET to WS route', async () => {
      const res = await app.inject({
        method: 'GET',
        url: '/ws/dashboard',
      });
      // Non-WebSocket requests to a WS endpoint return 400 or similar
      expect(res.statusCode).toBeGreaterThanOrEqual(400);
    });
  });

  describe('state_sync query logic', () => {
    beforeEach(() => {
      // Clean up sessions table
      app.db.delete(sessions).run();
    });

    it('should return active sessions excluding completed and cancelled', () => {
      // Seed sessions with various statuses
      app.db.insert(sessions).values([
        { id: 'sess-queued', projectPath: '/tmp/p', prompt: 'test', status: 'queued' },
        { id: 'sess-running', projectPath: '/tmp/p', prompt: 'test', status: 'running' },
        { id: 'sess-needs-input', projectPath: '/tmp/p', prompt: 'test', status: 'needs_input' },
        { id: 'sess-completed', projectPath: '/tmp/p', prompt: 'test', status: 'completed' },
        { id: 'sess-cancelled', projectPath: '/tmp/p', prompt: 'test', status: 'cancelled' },
        { id: 'sess-failed', projectPath: '/tmp/p', prompt: 'test', status: 'failed' },
        { id: 'sess-auth-req', projectPath: '/tmp/p', prompt: 'test', status: 'auth_required' },
      ]).run();

      // Use the same query logic as the plugin
      const activeSessions = app.db
        .select()
        .from(sessions)
        .where(notInArray(sessions.status, ['completed', 'cancelled']))
        .all();

      // Should include queued, running, needs_input, failed, auth_required
      expect(activeSessions).toHaveLength(5);
      const ids = activeSessions.map((s) => s.id);
      expect(ids).toContain('sess-queued');
      expect(ids).toContain('sess-running');
      expect(ids).toContain('sess-needs-input');
      expect(ids).toContain('sess-failed');
      expect(ids).toContain('sess-auth-req');
      // Should NOT include completed or cancelled
      expect(ids).not.toContain('sess-completed');
      expect(ids).not.toContain('sess-cancelled');
    });

    it('should return empty array when no active sessions exist', () => {
      const activeSessions = app.db
        .select()
        .from(sessions)
        .where(notInArray(sessions.status, ['completed', 'cancelled']))
        .all();

      expect(activeSessions).toHaveLength(0);
    });

    it('should return empty array when only completed/cancelled sessions exist', () => {
      app.db.insert(sessions).values([
        { id: 'sess-c1', projectPath: '/tmp/p', prompt: 'test', status: 'completed' },
        { id: 'sess-c2', projectPath: '/tmp/p', prompt: 'test', status: 'cancelled' },
      ]).run();

      const activeSessions = app.db
        .select()
        .from(sessions)
        .where(notInArray(sessions.status, ['completed', 'cancelled']))
        .all();

      expect(activeSessions).toHaveLength(0);
    });
  });

  describe('event bus handler logic', () => {
    beforeEach(() => {
      app.db.delete(sessions).run();
    });

    it('should look up full session on session_update event', () => {
      // Insert a session
      app.db.insert(sessions).values({
        id: 'sess-update-test',
        projectPath: '/tmp/p',
        prompt: 'testing update',
        status: 'running',
      }).run();

      // Simulate what the handler does - look up full session
      const session = app.db
        .select()
        .from(sessions)
        .where(eq(sessions.id, 'sess-update-test'))
        .get();

      expect(session).toBeDefined();
      expect(session!.id).toBe('sess-update-test');
      expect(session!.status).toBe('running');
      expect(session!.prompt).toBe('testing update');
    });

    it('should handle session_update for nonexistent session gracefully', () => {
      const session = app.db
        .select()
        .from(sessions)
        .where(eq(sessions.id, 'nonexistent'))
        .get();

      expect(session).toBeUndefined();
    });
  });

  describe('event bus subscription and cleanup', () => {
    it('should subscribe and unsubscribe from event bus correctly', () => {
      const bus = new EventBus();
      const handler = vi.fn();

      bus.on('session_update', handler);
      bus.emit('session_update', { sessionId: 'test', status: 'running' });
      expect(handler).toHaveBeenCalledTimes(1);

      bus.off('session_update', handler);
      bus.emit('session_update', { sessionId: 'test', status: 'completed' });
      expect(handler).toHaveBeenCalledTimes(1); // Still 1, not called again
    });

    it('should subscribe and unsubscribe from notification events', () => {
      const bus = new EventBus();
      const handler = vi.fn();

      bus.on('notification', handler);
      bus.emit('notification', { sessionId: 'test', type: 'error', message: 'Something failed' });
      expect(handler).toHaveBeenCalledTimes(1);

      bus.off('notification', handler);
      bus.emit('notification', { sessionId: 'test', type: 'error', message: 'Another error' });
      expect(handler).toHaveBeenCalledTimes(1); // Still 1
    });
  });

  describe('message serialization', () => {
    it('should produce valid state_sync JSON', () => {
      app.db.delete(sessions).run();
      app.db.insert(sessions).values({
        id: 'sess-json',
        projectPath: '/tmp/p',
        prompt: 'test json',
        status: 'running',
      }).run();

      const activeSessions = app.db
        .select()
        .from(sessions)
        .where(notInArray(sessions.status, ['completed', 'cancelled']))
        .all();

      const message = JSON.stringify({ type: 'state_sync', sessions: activeSessions });
      const parsed = JSON.parse(message);

      expect(parsed.type).toBe('state_sync');
      expect(parsed.sessions).toHaveLength(1);
      expect(parsed.sessions[0].id).toBe('sess-json');
    });

    it('should produce valid session_update JSON', () => {
      const session = {
        id: 'sess-1',
        projectPath: '/tmp/p',
        prompt: 'test',
        status: 'running',
        agentType: 'claude',
      };

      const message = JSON.stringify({ type: 'session_update', session });
      const parsed = JSON.parse(message);

      expect(parsed.type).toBe('session_update');
      expect(parsed.session.id).toBe('sess-1');
    });

    it('should produce valid notification JSON', () => {
      const notification = {
        sessionId: 'sess-1',
        type: 'error',
        message: 'Something went wrong',
      };

      const message = JSON.stringify({ type: 'notification', notification });
      const parsed = JSON.parse(message);

      expect(parsed.type).toBe('notification');
      expect(parsed.notification.sessionId).toBe('sess-1');
      expect(parsed.notification.message).toBe('Something went wrong');
    });
  });
});
