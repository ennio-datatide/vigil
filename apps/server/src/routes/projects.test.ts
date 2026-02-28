import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { buildApp } from '../app.js';

describe('projects routes', () => {
  let app: Awaited<ReturnType<typeof buildApp>>;

  beforeAll(async () => {
    app = await buildApp({ praefectusHome: '/tmp/pf-test-projects-' + Date.now() });
  });

  afterAll(async () => {
    await app.close();
  });

  it('GET /api/projects should return empty list initially', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/projects' });
    expect(res.statusCode).toBe(200);
    expect(res.json()).toEqual([]);
  });

  it('POST /api/projects should create a project', async () => {
    const res = await app.inject({
      method: 'POST',
      url: '/api/projects',
      payload: { path: '/tmp/my-project', name: 'My Project' },
    });
    expect(res.statusCode).toBe(201);
    const body = res.json();
    expect(body.path).toBe('/tmp/my-project');
    expect(body.name).toBe('My Project');
    expect(body.lastUsedAt).toBeTypeOf('number');
  });

  it('GET /api/projects should return the created project', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/projects' });
    expect(res.statusCode).toBe(200);
    const body = res.json();
    expect(body).toHaveLength(1);
    expect(body[0].name).toBe('My Project');
  });

  it('DELETE /api/projects/:path should remove the project', async () => {
    const encodedPath = encodeURIComponent('/tmp/my-project');
    const res = await app.inject({ method: 'DELETE', url: `/api/projects/${encodedPath}` });
    expect(res.statusCode).toBe(200);
    expect(res.json()).toEqual({ ok: true });

    // Verify it's gone
    const list = await app.inject({ method: 'GET', url: '/api/projects' });
    expect(list.json()).toEqual([]);
  });

  it('POST /api/projects should reject invalid input', async () => {
    const res = await app.inject({
      method: 'POST',
      url: '/api/projects',
      payload: { path: '', name: '' },
    });
    expect(res.statusCode).toBe(400);
  });
});
