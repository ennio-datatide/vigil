import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { eq } from 'drizzle-orm';
import type { FastifyPluginAsync } from 'fastify';
import { sessions } from '../db/schema.js';
import { safeSend } from '../utils/safe-send.js';

const terminalWs: FastifyPluginAsync = async (app) => {
  app.get('/ws/terminal/:sessionId', { websocket: true }, async (socket, request) => {
    const { sessionId } = request.params as { sessionId: string };

    // Look up session in database
    const session = app.db.select().from(sessions).where(eq(sessions.id, sessionId)).get();
    if (!session) {
      socket.send(JSON.stringify({ error: 'Session not found' }));
      socket.close();
      return;
    }

    const ptyAlive = app.ptyManager.isAlive(sessionId);
    app.log.info({ sessionId, ptyAlive }, 'Terminal WS connected');

    // Tell client whether the PTY is live or read-only (history replay)
    safeSend(socket, JSON.stringify({ type: 'pty_status', alive: ptyAlive }));

    // Send history: try in-memory buffer first, fall back to log file on disk
    const history = app.outputManager.getHistory(sessionId);
    if (history) {
      socket.send(history);
    } else {
      // Buffer gone (server restart or session ended) — read from persisted log
      const logPath = join(app.config.logsDir, `${sessionId}.log`);
      if (existsSync(logPath)) {
        try {
          const logContent = readFileSync(logPath, 'utf-8');
          if (logContent) {
            socket.send(logContent);
          }
        } catch {
          // Log read failed, proceed without history
        }
      }
    }

    // Ensure output buffer exists so subscriptions work even after server restart.
    // When a session is restarted later, new PTY output will flow to this subscriber.
    app.outputManager.createBuffer(sessionId);

    // Subscribe to live output updates
    const unsubscribe = app.outputManager.subscribe(sessionId, (data: string) => {
      safeSend(socket, data);
    });

    // Listen for session_spawned events — when a session restarts, notify this client
    // that the PTY is alive again so it can re-enable input.
    const onSessionSpawned = (data: { sessionId: string }) => {
      if (data.sessionId === sessionId) {
        safeSend(socket, JSON.stringify({ type: 'pty_status', alive: true }));
      }
    };
    app.eventBus.on('session_spawned', onSessionSpawned);

    // Bidirectional: handle client → server messages when PTY is alive
    socket.on('message', (raw: Buffer | ArrayBuffer | Buffer[]) => {
      if (!app.ptyManager.isAlive(sessionId)) {
        app.log.warn({ sessionId }, 'Input dropped: PTY not alive');
        return;
      }

      try {
        // ws v8+ sends Buffer/ArrayBuffer/Buffer[] — normalize to string
        let str: string;
        if (Buffer.isBuffer(raw)) {
          str = raw.toString('utf-8');
        } else if (raw instanceof ArrayBuffer) {
          str = Buffer.from(raw).toString('utf-8');
        } else if (Array.isArray(raw)) {
          str = Buffer.concat(raw).toString('utf-8');
        } else {
          str = String(raw);
        }

        const msg = JSON.parse(str);

        if (msg.type === 'input' && typeof msg.data === 'string') {
          app.ptyManager.write(sessionId, msg.data);
        } else if (
          msg.type === 'resize' &&
          typeof msg.cols === 'number' &&
          typeof msg.rows === 'number'
        ) {
          app.ptyManager.resize(sessionId, msg.cols, msg.rows);
        }
      } catch {
        // Not JSON or malformed — ignore
      }
    });

    // Cleanup on WebSocket disconnect
    socket.on('close', () => {
      unsubscribe();
      app.eventBus.off('session_spawned', onSessionSpawned);
    });
  });
};

export default terminalWs;
