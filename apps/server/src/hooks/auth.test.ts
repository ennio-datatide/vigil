import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { buildApp } from '../app.js';

describe('auth hook', () => {
  const TEST_TOKEN = 'test-secret-token-1234';

  describe('with token configured', () => {
    let app: Awaited<ReturnType<typeof buildApp>>;

    beforeAll(async () => {
      app = await buildApp({
        praefectusHome: `/tmp/pf-test-auth-${Date.now()}`,
        apiToken: TEST_TOKEN,
      });
    });

    afterAll(async () => {
      await app.close();
    });

    it('GET /health should be accessible without token', async () => {
      const res = await app.inject({ method: 'GET', url: '/health' });
      expect(res.statusCode).toBe(200);
    });

    it('GET /api/sessions should return 401 without token', async () => {
      const res = await app.inject({ method: 'GET', url: '/api/sessions' });
      expect(res.statusCode).toBe(401);
      expect(res.json()).toEqual({ error: 'Unauthorized' });
    });

    it('GET /api/sessions should return 401 with wrong token', async () => {
      const res = await app.inject({
        method: 'GET',
        url: '/api/sessions',
        headers: { authorization: 'Bearer wrong-token' },
      });
      expect(res.statusCode).toBe(401);
      expect(res.json()).toEqual({ error: 'Unauthorized' });
    });

    it('GET /api/sessions should return 200 with correct token', async () => {
      const res = await app.inject({
        method: 'GET',
        url: '/api/sessions',
        headers: { authorization: `Bearer ${TEST_TOKEN}` },
      });
      expect(res.statusCode).toBe(200);
    });

    it('should accept token via ?token= query param (for WebSocket)', async () => {
      const res = await app.inject({
        method: 'GET',
        url: `/api/sessions?token=${TEST_TOKEN}`,
      });
      expect(res.statusCode).toBe(200);
    });

    it('POST /events should return 401 without token', async () => {
      const res = await app.inject({
        method: 'POST',
        url: '/events',
        payload: { sessionId: 'x', event: 'test' },
      });
      expect(res.statusCode).toBe(401);
    });
  });

  describe('without token configured (dev/test mode)', () => {
    let app: Awaited<ReturnType<typeof buildApp>>;

    beforeAll(async () => {
      app = await buildApp({
        praefectusHome: `/tmp/pf-test-noauth-${Date.now()}`,
        apiToken: undefined,
      });
    });

    afterAll(async () => {
      await app.close();
    });

    it('GET /api/sessions should be accessible without token', async () => {
      const res = await app.inject({ method: 'GET', url: '/api/sessions' });
      expect(res.statusCode).toBe(200);
    });
  });
});
