import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { buildApp } from '../app.js';

describe('POST /events', () => {
  let app: Awaited<ReturnType<typeof buildApp>>;

  beforeAll(async () => {
    app = await buildApp({
      praefectusHome: `/tmp/pf-test-events-${Date.now()}`,
      apiToken: undefined,
    });

    // Initialize schema tables (in-memory / fresh DB needs DDL)
    const { initializeSchema } = await import('../db/client.js');
    initializeSchema(app.sqlite);

    // Seed a session so FK is valid
    const { sessions } = await import('../db/schema.js');
    app.db
      .insert(sessions)
      .values({
        id: 'sess-abc',
        projectPath: '/tmp/test',
        prompt: 'test',
        status: 'running',
        agentType: 'claude',
      })
      .run();
  });

  afterAll(async () => {
    await app.close();
  });

  it('should accept hook events and store them', async () => {
    const res = await app.inject({
      method: 'POST',
      url: '/events',
      payload: {
        session_id: 'sess-abc',
        data: {
          hook_event_name: 'PostToolUse',
          tool_name: 'Bash',
          tool_input: { command: 'ls' },
        },
      },
    });

    expect(res.statusCode).toBe(200);
    expect(res.json()).toEqual({ ok: true });
  });

  it('should reject invalid payloads', async () => {
    const res = await app.inject({
      method: 'POST',
      url: '/events',
      payload: { bad: 'data' },
    });

    expect(res.statusCode).toBe(400);
  });

  it('should emit events on the event bus', async () => {
    const events: { sessionId: string; eventType: string }[] = [];
    app.eventBus.on('hook_event', (e) =>
      events.push(e as { sessionId: string; eventType: string }),
    );

    await app.inject({
      method: 'POST',
      url: '/events',
      payload: {
        session_id: 'sess-abc',
        data: {
          hook_event_name: 'PreToolUse',
          tool_name: 'Edit',
        },
      },
    });

    expect(events).toHaveLength(1);
    expect(events[0].sessionId).toBe('sess-abc');
    expect(events[0].eventType).toBe('PreToolUse');
  });
});
