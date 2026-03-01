import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { buildApp } from '../app.js';
import { notifications } from '../db/schema.js';

describe('notifications routes', () => {
  let app: Awaited<ReturnType<typeof buildApp>>;

  beforeAll(async () => {
    app = await buildApp({
      praefectusHome: `/tmp/pf-test-notifications-${Date.now()}`,
      apiToken: undefined,
    });
  });

  afterAll(async () => {
    await app.close();
  });

  it('GET /api/notifications should return empty list initially', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/notifications' });
    expect(res.statusCode).toBe(200);
    expect(res.json()).toEqual([]);
  });

  it('GET /api/notifications should return inserted notifications', async () => {
    app.db
      .insert(notifications)
      .values({
        sessionId: 'sess-1',
        type: 'needs_input',
        message: 'Need user confirmation',
        sentAt: Date.now(),
      })
      .run();

    app.db
      .insert(notifications)
      .values({
        sessionId: 'sess-2',
        type: 'error',
        message: 'Something broke',
        sentAt: Date.now(),
      })
      .run();

    const res = await app.inject({ method: 'GET', url: '/api/notifications' });
    expect(res.statusCode).toBe(200);
    const body = res.json();
    expect(body).toHaveLength(2);
    // Most recent first (higher id first)
    expect(body[0].message).toBe('Something broke');
    expect(body[1].message).toBe('Need user confirmation');
  });

  it('GET /api/notifications?unread=true should filter unread only', async () => {
    // Mark the first notification as read
    const all = await app.inject({ method: 'GET', url: '/api/notifications' });
    const firstId = all.json()[1].id; // "Need user confirmation" is second (older)
    await app.inject({ method: 'PATCH', url: `/api/notifications/${firstId}/read` });

    const res = await app.inject({ method: 'GET', url: '/api/notifications?unread=true' });
    expect(res.statusCode).toBe(200);
    const body = res.json();
    expect(body).toHaveLength(1);
    expect(body[0].message).toBe('Something broke');
  });

  it('PATCH /api/notifications/:id/read should mark notification as read', async () => {
    const all = await app.inject({ method: 'GET', url: '/api/notifications' });
    const unreadNotification = all.json().find((n: Record<string, unknown>) => n.readAt === null);

    const res = await app.inject({
      method: 'PATCH',
      url: `/api/notifications/${unreadNotification.id}/read`,
    });
    expect(res.statusCode).toBe(200);
    const body = res.json();
    expect(body.readAt).toBeTypeOf('number');
    expect(body.id).toBe(unreadNotification.id);
  });
});
