import { desc, eq, isNull } from 'drizzle-orm';
import type { FastifyPluginAsync } from 'fastify';
import { notifications } from '../db/schema.js';

const notificationsRoute: FastifyPluginAsync = async (app) => {
  // List all notifications, most recent first
  // Optional query param ?unread=true to filter only unread
  app.get<{ Querystring: { unread?: string } }>('/api/notifications', async (request) => {
    const { unread } = request.query;

    if (unread === 'true') {
      return app.db
        .select()
        .from(notifications)
        .where(isNull(notifications.readAt))
        .orderBy(desc(notifications.id))
        .all();
    }

    return app.db.select().from(notifications).orderBy(desc(notifications.id)).all();
  });

  // Send a test notification through the full pipeline (DB + event bus + Telegram)
  app.post('/api/notifications/test', async () => {
    const message = 'Test notification from Praefectus';
    const type = 'session_done';

    app.db
      .insert(notifications)
      .values({ sessionId: 'system', type, message, sentAt: Date.now() })
      .run();

    app.eventBus.emit('notification', { sessionId: 'system', type, message });

    return { ok: true, message: 'Test notification sent' };
  });

  // Mark a notification as read
  app.patch<{ Params: { id: string } }>('/api/notifications/:id/read', async (request, reply) => {
    const id = Number(request.params.id);
    if (Number.isNaN(id)) {
      return reply.status(400).send({ error: 'Invalid notification id' });
    }

    const existing = app.db.select().from(notifications).where(eq(notifications.id, id)).get();
    if (!existing) {
      return reply.status(404).send({ error: 'Notification not found' });
    }

    const updated = app.db
      .update(notifications)
      .set({ readAt: Date.now() })
      .where(eq(notifications.id, id))
      .returning()
      .get();

    return updated;
  });
};

export default notificationsRoute;
