import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { buildApp } from '../app.js';

describe('sessions routes', () => {
  let app: Awaited<ReturnType<typeof buildApp>>;

  beforeAll(async () => {
    app = await buildApp({ praefectusHome: `/tmp/pf-test-sessions-${Date.now()}` });
    // Seed a project
    const { projects } = await import('../db/schema.js');
    app.db
      .insert(projects)
      .values({
        path: '/tmp/test-project',
        name: 'Test Project',
      })
      .run();
  });

  afterAll(async () => {
    await app.close();
  });

  it('GET /api/sessions should return empty list', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/sessions' });
    expect(res.statusCode).toBe(200);
    expect(res.json()).toEqual([]);
  });

  it('POST /api/sessions should create a queued session', async () => {
    const res = await app.inject({
      method: 'POST',
      url: '/api/sessions',
      payload: {
        projectPath: '/tmp/test-project',
        prompt: 'Add authentication',
      },
    });
    expect(res.statusCode).toBe(201);
    const body = res.json();
    expect(body.status).toBe('queued');
    expect(body.prompt).toBe('Add authentication');
    expect(body.id).toBeDefined();
  });

  it('GET /api/sessions should return created session', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/sessions' });
    expect(res.json()).toHaveLength(1);
  });

  it('GET /api/sessions/:id should return a single session', async () => {
    // First create a session
    const create = await app.inject({
      method: 'POST',
      url: '/api/sessions',
      payload: { projectPath: '/tmp/test-project', prompt: 'test get' },
    });
    const id = create.json().id;

    const res = await app.inject({ method: 'GET', url: `/api/sessions/${id}` });
    expect(res.statusCode).toBe(200);
    expect(res.json().id).toBe(id);
  });

  it('GET /api/sessions/:id should return 404 for unknown id', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/sessions/nonexistent' });
    expect(res.statusCode).toBe(404);
  });

  it('DELETE /api/sessions/:id should cancel a session', async () => {
    const create = await app.inject({
      method: 'POST',
      url: '/api/sessions',
      payload: { projectPath: '/tmp/test-project', prompt: 'test cancel' },
    });
    const id = create.json().id;

    const res = await app.inject({ method: 'DELETE', url: `/api/sessions/${id}` });
    expect(res.statusCode).toBe(200);
    expect(res.json().status).toBe('cancelled');
  });

  describe('POST /api/sessions/:id/resume', () => {
    it('should return 404 for nonexistent session', async () => {
      const res = await app.inject({
        method: 'POST',
        url: '/api/sessions/nonexistent/resume',
      });
      expect(res.statusCode).toBe(404);
    });

    it('should return 400 for running session', async () => {
      // Seed a running session directly
      const { sessions } = await import('../db/schema.js');
      app.db
        .insert(sessions)
        .values({
          id: 'running-sess',
          projectPath: '/tmp/test-project',
          prompt: 'running prompt',
          status: 'running',
        })
        .run();

      const res = await app.inject({
        method: 'POST',
        url: '/api/sessions/running-sess/resume',
      });
      expect(res.statusCode).toBe(400);
    });

    it('should return 400 for session without worktree path', async () => {
      const { sessions } = await import('../db/schema.js');
      app.db
        .insert(sessions)
        .values({
          id: 'no-wt-sess',
          projectPath: '/tmp/test-project',
          prompt: 'no worktree',
          status: 'completed',
          worktreePath: null,
        })
        .run();

      const res = await app.inject({
        method: 'POST',
        url: '/api/sessions/no-wt-sess/resume',
      });
      expect(res.statusCode).toBe(400);
    });

    it('should create a new session when resuming a completed session with worktree', async () => {
      const { sessions } = await import('../db/schema.js');
      app.db
        .insert(sessions)
        .values({
          id: 'completed-sess',
          projectPath: '/tmp/test-project',
          prompt: 'completed prompt',
          status: 'completed',
          worktreePath: '/tmp/some-worktree',
        })
        .run();

      const res = await app.inject({
        method: 'POST',
        url: '/api/sessions/completed-sess/resume',
      });
      expect(res.statusCode).toBe(201);
      const body = res.json();
      expect(body.parentId).toBe('completed-sess');
      expect(body.prompt).toBe('Resumed conversation');
      expect(body.status).toBe('queued');
    });
  });

  describe('git metadata', () => {
    it('should include parsed gitMetadata in session responses', async () => {
      const { sessions } = await import('../db/schema.js');
      app.db
        .insert(sessions)
        .values({
          id: 'meta-sess',
          projectPath: '/tmp/test-project',
          prompt: 'meta test',
          status: 'running',
          gitMetadata: JSON.stringify({
            repoName: 'my-repo',
            branch: 'main',
            commitHash: 'abc1234',
            remoteUrl: 'https://github.com/user/repo.git',
          }),
        })
        .run();

      const res = await app.inject({
        method: 'GET',
        url: '/api/sessions/meta-sess',
      });
      expect(res.statusCode).toBe(200);
      const body = res.json();
      expect(body.gitMetadata).toEqual({
        repoName: 'my-repo',
        branch: 'main',
        commitHash: 'abc1234',
        remoteUrl: 'https://github.com/user/repo.git',
      });
    });

    it('should return null gitMetadata when not set', async () => {
      const { sessions } = await import('../db/schema.js');
      app.db
        .insert(sessions)
        .values({
          id: 'no-meta-sess',
          projectPath: '/tmp/test-project',
          prompt: 'no meta',
          status: 'running',
        })
        .run();

      const res = await app.inject({
        method: 'GET',
        url: '/api/sessions/no-meta-sess',
      });
      expect(res.statusCode).toBe(200);
      expect(res.json().gitMetadata).toBeNull();
    });
  });
});
