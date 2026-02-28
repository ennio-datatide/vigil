import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { buildApp } from '../app.js';

describe('pipeline routes', () => {
  let app: Awaited<ReturnType<typeof buildApp>>;

  beforeAll(async () => {
    app = await buildApp({ praefectusHome: '/tmp/pf-test-pipelines-' + Date.now() });
  });

  afterAll(async () => {
    await app.close();
  });

  it('GET /api/pipelines should return seeded default pipeline', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/pipelines' });
    expect(res.statusCode).toBe(200);
    const body = res.json();
    // seedDefault() is called on startup, so we should have 1
    expect(body.length).toBeGreaterThanOrEqual(1);
    expect(body[0].isDefault).toBe(true);
  });

  it('GET /api/pipelines/:id should return a single pipeline', async () => {
    const list = await app.inject({ method: 'GET', url: '/api/pipelines' });
    const id = list.json()[0].id;

    const res = await app.inject({ method: 'GET', url: `/api/pipelines/${id}` });
    expect(res.statusCode).toBe(200);
    expect(res.json().id).toBe(id);
    expect(res.json().steps).toBeInstanceOf(Array);
    expect(res.json().edges).toBeInstanceOf(Array);
  });

  it('GET /api/pipelines/:id should return 404 for unknown id', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/pipelines/nonexistent' });
    expect(res.statusCode).toBe(404);
  });

  it('POST /api/pipelines should create a new pipeline', async () => {
    const res = await app.inject({
      method: 'POST',
      url: '/api/pipelines',
      payload: {
        name: 'Custom Pipeline',
        steps: [
          { id: 's1', skill: 'brainstorming', label: 'Brainstorm', agent: 'claude', prompt: 'Think', position: { x: 0, y: 0 } },
        ],
        edges: [],
      },
    });
    expect(res.statusCode).toBe(201);
    const body = res.json();
    expect(body.name).toBe('Custom Pipeline');
    expect(body.steps).toHaveLength(1);
    expect(body.isDefault).toBe(false);
  });

  it('POST /api/pipelines should return 400 for invalid input', async () => {
    const res = await app.inject({
      method: 'POST',
      url: '/api/pipelines',
      payload: { name: 'No Steps' },
    });
    expect(res.statusCode).toBe(400);
  });

  it('PUT /api/pipelines/:id should update a pipeline', async () => {
    // Create a pipeline to update
    const create = await app.inject({
      method: 'POST',
      url: '/api/pipelines',
      payload: {
        name: 'To Update',
        steps: [
          { id: 's1', skill: 'brainstorming', label: 'Brainstorm', agent: 'claude', prompt: 'Think', position: { x: 0, y: 0 } },
        ],
        edges: [],
      },
    });
    const id = create.json().id;

    const res = await app.inject({
      method: 'PUT',
      url: `/api/pipelines/${id}`,
      payload: { name: 'Updated Name' },
    });
    expect(res.statusCode).toBe(200);
    expect(res.json().name).toBe('Updated Name');
  });

  it('PUT /api/pipelines/:id should return 404 for unknown id', async () => {
    const res = await app.inject({
      method: 'PUT',
      url: '/api/pipelines/nonexistent',
      payload: { name: 'x' },
    });
    expect(res.statusCode).toBe(404);
  });

  it('DELETE /api/pipelines/:id should delete a non-default pipeline', async () => {
    const create = await app.inject({
      method: 'POST',
      url: '/api/pipelines',
      payload: {
        name: 'To Delete',
        steps: [
          { id: 's1', skill: 'brainstorming', label: 'Brainstorm', agent: 'claude', prompt: 'Think', position: { x: 0, y: 0 } },
        ],
        edges: [],
      },
    });
    const id = create.json().id;

    const res = await app.inject({ method: 'DELETE', url: `/api/pipelines/${id}` });
    expect(res.statusCode).toBe(200);
    expect(res.json()).toEqual({ ok: true });

    // Verify it's gone
    const get = await app.inject({ method: 'GET', url: `/api/pipelines/${id}` });
    expect(get.statusCode).toBe(404);
  });

  it('DELETE /api/pipelines/:id should return 404 for unknown id', async () => {
    const res = await app.inject({ method: 'DELETE', url: '/api/pipelines/nonexistent' });
    expect(res.statusCode).toBe(404);
  });

  it('DELETE should prevent deleting the only pipeline', async () => {
    // Create a fresh app with only the seeded default
    const freshApp = await buildApp({ praefectusHome: '/tmp/pf-test-pipelines-fresh-' + Date.now() });
    const list = await freshApp.inject({ method: 'GET', url: '/api/pipelines' });
    const defaultId = list.json()[0].id;

    const res = await freshApp.inject({ method: 'DELETE', url: `/api/pipelines/${defaultId}` });
    expect(res.statusCode).toBe(400);
    expect(res.json().error).toContain('Cannot delete');

    await freshApp.close();
  });
});
