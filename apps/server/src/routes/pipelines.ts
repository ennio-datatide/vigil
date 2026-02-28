import { CreatePipelineInput, UpdatePipelineInput } from '@praefectus/shared';
import type { FastifyPluginAsync } from 'fastify';

const pipelinesRoute: FastifyPluginAsync = async (app) => {
  // List all pipelines
  app.get('/api/pipelines', async () => {
    return app.pipelineService.list();
  });

  // Get single pipeline
  app.get<{ Params: { id: string } }>('/api/pipelines/:id', async (request, reply) => {
    const pipeline = app.pipelineService.get(request.params.id);
    if (!pipeline) return reply.status(404).send({ error: 'Pipeline not found' });
    return pipeline;
  });

  // Create pipeline
  app.post('/api/pipelines', async (request, reply) => {
    const parsed = CreatePipelineInput.safeParse(request.body);
    if (!parsed.success) {
      return reply.status(400).send({ error: 'Invalid input', details: parsed.error.issues });
    }

    const pipeline = app.pipelineService.create(parsed.data);
    return reply.status(201).send(pipeline);
  });

  // Update pipeline
  app.put<{ Params: { id: string } }>('/api/pipelines/:id', async (request, reply) => {
    const parsed = UpdatePipelineInput.safeParse(request.body);
    if (!parsed.success) {
      return reply.status(400).send({ error: 'Invalid input', details: parsed.error.issues });
    }

    const existing = app.pipelineService.get(request.params.id);
    if (!existing) {
      return reply.status(404).send({ error: 'Pipeline not found' });
    }

    const pipeline = app.pipelineService.update(request.params.id, parsed.data);
    return pipeline;
  });

  // Delete pipeline
  app.delete<{ Params: { id: string } }>('/api/pipelines/:id', async (request, reply) => {
    const pipeline = app.pipelineService.get(request.params.id);
    if (!pipeline) return reply.status(404).send({ error: 'Pipeline not found' });

    // Prevent deleting the only default pipeline
    if (pipeline.isDefault) {
      const all = app.pipelineService.list();
      if (all.length <= 1) {
        return reply.status(400).send({ error: 'Cannot delete the only pipeline' });
      }
    }

    app.pipelineService.delete(request.params.id);
    return { ok: true };
  });
};

export default pipelinesRoute;
