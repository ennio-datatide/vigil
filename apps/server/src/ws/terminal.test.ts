import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { buildApp } from '../app.js';
import { sessions } from '../db/schema.js';

describe('terminal WebSocket route', () => {
  let app: Awaited<ReturnType<typeof buildApp>>;

  beforeAll(async () => {
    app = await buildApp({ praefectusHome: '/tmp/pf-test-terminal-ws-' + Date.now() });
  });

  afterAll(async () => {
    await app.close();
  });

  describe('plugin registration', () => {
    it('should register the /ws/terminal/:sessionId route', () => {
      const routes = app.printRoutes();
      expect(routes).toContain('terminal/');
      expect(routes).toContain(':sessionId');
    });
  });

  describe('non-WebSocket requests to WS endpoint', () => {
    it('should return non-200 for plain HTTP GET to WS route', async () => {
      const res = await app.inject({
        method: 'GET',
        url: '/ws/terminal/nonexistent-session',
      });
      expect(res.statusCode).toBeGreaterThanOrEqual(400);
    });
  });

  describe('session lookup logic (via HTTP inject)', () => {
    it('should 404 or error for nonexistent session ID', async () => {
      const res = await app.inject({
        method: 'GET',
        url: '/ws/terminal/does-not-exist',
      });
      expect(res.statusCode).toBeGreaterThanOrEqual(400);
    });

    it('should seed a session without tmuxSession for validation testing', () => {
      app.db.insert(sessions).values({
        id: 'sess-no-tmux',
        projectPath: '/tmp/test-project',
        prompt: 'test no tmux',
        status: 'queued',
      }).run();

      const result = app.db.select().from(sessions).all();
      const found = result.find((s) => s.id === 'sess-no-tmux');
      expect(found).toBeDefined();
      expect(found!.tmuxSession).toBeNull();
    });

    it('should seed a session with a pid for connection testing', () => {
      app.db.insert(sessions).values({
        id: 'sess-with-pid',
        projectPath: '/tmp/test-project',
        prompt: 'test with pid',
        status: 'running',
        tmuxSession: '12345',
      }).run();

      const result = app.db.select().from(sessions).all();
      const found = result.find((s) => s.id === 'sess-with-pid');
      expect(found).toBeDefined();
      expect(found!.tmuxSession).toBe('12345');
    });
  });

  describe('outputManager decoration', () => {
    it('should have outputManager decorated on the app instance', () => {
      expect(app.outputManager).toBeDefined();
      expect(typeof app.outputManager.createBuffer).toBe('function');
      expect(typeof app.outputManager.append).toBe('function');
      expect(typeof app.outputManager.subscribe).toBe('function');
      expect(typeof app.outputManager.getHistory).toBe('function');
      expect(typeof app.outputManager.hasSession).toBe('function');
      expect(typeof app.outputManager.getActiveSessions).toBe('function');
      expect(typeof app.outputManager.disposeAll).toBe('function');
    });
  });

  describe('ptyManager decoration', () => {
    it('should have ptyManager decorated on the app instance', () => {
      expect(app.ptyManager).toBeDefined();
      expect(typeof app.ptyManager.create).toBe('function');
      expect(typeof app.ptyManager.write).toBe('function');
      expect(typeof app.ptyManager.resize).toBe('function');
      expect(typeof app.ptyManager.kill).toBe('function');
      expect(typeof app.ptyManager.isAlive).toBe('function');
    });
  });
});
