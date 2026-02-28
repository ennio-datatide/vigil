import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { buildApp } from '../app.js';

describe('E2E: Session lifecycle', () => {
  let app: Awaited<ReturnType<typeof buildApp>>;

  beforeAll(async () => {
    app = await buildApp({ praefectusHome: '/tmp/pf-e2e-test-' + Date.now() });

    await app.ready();
  });

  afterAll(async () => {
    await app.close();
  });

  it('should complete a full session lifecycle via API', async () => {
    // 1. Health check
    const health = await app.inject({ method: 'GET', url: '/health' });
    expect(health.statusCode).toBe(200);
    expect(health.json()).toEqual({ status: 'ok' });

    // 2. Register project
    const projectRes = await app.inject({
      method: 'POST',
      url: '/api/projects',
      payload: { path: '/tmp/test-project', name: 'Test Project' },
    });
    expect(projectRes.statusCode).toBe(201);

    // 3. Create session
    const createRes = await app.inject({
      method: 'POST',
      url: '/api/sessions',
      payload: { projectPath: '/tmp/test-project', prompt: 'Test prompt' },
    });
    expect(createRes.statusCode).toBe(201);
    const session = createRes.json();
    expect(session.id).toBeDefined();
    expect(session.status).toBe('queued');

    // 4. List sessions
    const listRes = await app.inject({ method: 'GET', url: '/api/sessions' });
    expect(listRes.statusCode).toBe(200);
    const sessions = listRes.json();
    expect(sessions.some((s: any) => s.id === session.id)).toBe(true);

    // 5. Get session by ID
    const getRes = await app.inject({ method: 'GET', url: `/api/sessions/${session.id}` });
    expect(getRes.statusCode).toBe(200);
    expect(getRes.json().id).toBe(session.id);

    // 6. Cancel session
    const cancelRes = await app.inject({ method: 'DELETE', url: `/api/sessions/${session.id}` });
    expect(cancelRes.statusCode).toBe(200);

    // 7. Verify cancelled
    const verifyRes = await app.inject({ method: 'GET', url: `/api/sessions/${session.id}` });
    expect(verifyRes.json().status).toBe('cancelled');
  });

  it('should post events via hook endpoint', async () => {
    // Seed a session for the event to reference
    const { sessions } = await import('../db/schema.js');
    app.db.insert(sessions).values({
      id: 'test-session',
      projectPath: '/tmp/test-project',
      prompt: 'event test',
      status: 'running',
      agentType: 'claude',
    }).run();

    const eventRes = await app.inject({
      method: 'POST',
      url: '/events',
      payload: {
        session_id: 'test-session',
        data: { hook_event_name: 'PreToolUse', tool_name: 'Read' },
      },
    });
    expect(eventRes.statusCode).toBe(200);
    expect(eventRes.json()).toEqual({ ok: true });
  });

  it('should manage projects', async () => {
    // List projects
    const listRes = await app.inject({ method: 'GET', url: '/api/projects' });
    expect(listRes.statusCode).toBe(200);

    // Delete project
    const delRes = await app.inject({
      method: 'DELETE',
      url: `/api/projects/${encodeURIComponent('/tmp/test-project')}`,
    });
    expect(delRes.statusCode).toBe(200);
  });

  it('should manage notifications', async () => {
    const listRes = await app.inject({ method: 'GET', url: '/api/notifications' });
    expect(listRes.statusCode).toBe(200);
  });
});
