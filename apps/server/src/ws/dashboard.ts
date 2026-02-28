import { eq } from 'drizzle-orm';
import type { FastifyPluginAsync } from 'fastify';
import { sessions } from '../db/schema.js';
import { withParsedGitMetadata } from '../utils/parse-git-metadata.js';
import { safeSend } from '../utils/safe-send.js';

const dashboardWs: FastifyPluginAsync = async (app) => {
  app.get('/ws/dashboard', { websocket: true }, async (socket, _request) => {
    // Send initial state sync — ALL sessions so completed ones remain visible
    const allSessions = app.db.select().from(sessions).all().map(withParsedGitMetadata);

    safeSend(socket, JSON.stringify({ type: 'state_sync', sessions: allSessions }));

    // Subscribe to session updates — fetch full session from DB
    const sessionHandler = (data: { sessionId: string; status: string }) => {
      const session = app.db.select().from(sessions).where(eq(sessions.id, data.sessionId)).get();

      if (session) {
        safeSend(
          socket,
          JSON.stringify({ type: 'session_update', session: withParsedGitMetadata(session) }),
        );
      }
    };

    // Subscribe to notifications — forward directly
    const notificationHandler = (data: { sessionId: string; type: string; message: string }) => {
      safeSend(socket, JSON.stringify({ type: 'notification', notification: data }));
    };

    // Subscribe to session removals
    const removedHandler = (data: { sessionId: string }) => {
      safeSend(socket, JSON.stringify({ type: 'session_removed', sessionId: data.sessionId }));
    };

    app.eventBus.on('session_update', sessionHandler);
    app.eventBus.on('session_removed', removedHandler);
    app.eventBus.on('notification', notificationHandler);

    // Cleanup on disconnect — unsubscribe from event bus
    socket.on('close', () => {
      app.eventBus.off('session_update', sessionHandler);
      app.eventBus.off('session_removed', removedHandler);
      app.eventBus.off('notification', notificationHandler);
    });
  });
};

export default dashboardWs;
