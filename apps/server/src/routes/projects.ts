import type { FastifyPluginAsync } from 'fastify';
import { eq } from 'drizzle-orm';
import { z } from 'zod';
import { projects } from '../db/schema.js';

const CreateProjectInput = z.object({
  path: z.string().min(1),
  name: z.string().min(1),
});

const projectsRoute: FastifyPluginAsync = async (app) => {
  // List all registered projects
  app.get('/api/projects', async () => {
    return app.db.select().from(projects).all();
  });

  // Register a new project
  app.post('/api/projects', async (request, reply) => {
    const parsed = CreateProjectInput.safeParse(request.body);
    if (!parsed.success) {
      return reply.status(400).send({ error: 'Invalid input', details: parsed.error.issues });
    }

    const { path, name } = parsed.data;
    const project = app.db.insert(projects).values({
      path,
      name,
      lastUsedAt: Date.now(),
    }).returning().get();

    return reply.status(201).send(project);
  });

  // Unregister a project (URL-encoded path as param)
  app.delete<{ Params: { path: string } }>('/api/projects/:path', async (request, reply) => {
    const projectPath = decodeURIComponent(request.params.path);
    const existing = app.db.select().from(projects).where(eq(projects.path, projectPath)).get();
    if (!existing) {
      return reply.status(404).send({ error: 'Project not found' });
    }

    app.db.delete(projects).where(eq(projects.path, projectPath)).run();
    return { ok: true };
  });
};

export default projectsRoute;
