import { timingSafeEqual } from 'node:crypto';
import type { FastifyInstance } from 'fastify';

export function registerAuthHook(app: FastifyInstance, apiToken: string | undefined) {
  if (!apiToken) return;

  const tokenBuffer = Buffer.from(apiToken);

  app.addHook('onRequest', async (request, reply) => {
    if (request.url === '/health') return;

    const authHeader = request.headers.authorization;
    let provided: string | undefined;

    if (authHeader?.startsWith('Bearer ')) {
      provided = authHeader.slice(7);
    } else {
      const url = new URL(request.url, 'http://localhost');
      provided = url.searchParams.get('token') ?? undefined;
    }

    if (!provided) {
      return reply.status(401).send({ error: 'Unauthorized' });
    }

    const providedBuffer = Buffer.from(provided);

    if (
      providedBuffer.length !== tokenBuffer.length ||
      !timingSafeEqual(providedBuffer, tokenBuffer)
    ) {
      return reply.status(401).send({ error: 'Unauthorized' });
    }
  });
}
