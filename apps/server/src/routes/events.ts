import type { FastifyPluginAsync } from 'fastify';
import { events } from '../db/schema.js';
import { HookPayload } from '@praefectus/shared';

const eventsRoute: FastifyPluginAsync = async (app) => {
  app.post('/events', async (request, reply) => {
    const parsed = HookPayload.safeParse(request.body);
    if (!parsed.success) {
      return reply.status(400).send({ error: 'Invalid payload', details: parsed.error.issues });
    }

    const { session_id, data } = parsed.data;
    const eventType = (data.hook_event_name as string) ?? 'unknown';
    const toolName = (data.tool_name as string) ?? null;
    const now = Date.now();

    // Persist event
    app.db.insert(events).values({
      sessionId: session_id,
      eventType,
      toolName,
      payload: JSON.stringify(data),
      timestamp: now,
    }).run();

    // Emit to event bus
    app.eventBus.emit('hook_event', {
      sessionId: session_id,
      eventType,
      toolName,
      payload: data as Record<string, unknown>,
      timestamp: now,
    });

    return { ok: true };
  });
};

export default eventsRoute;
