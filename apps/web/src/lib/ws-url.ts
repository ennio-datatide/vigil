/**
 * Build a WebSocket URL that connects directly to the backend.
 *
 * Next.js rewrites only proxy HTTP — WebSocket upgrade requests
 * are NOT forwarded, so we connect to the Fastify server directly.
 */
export function wsUrl(path: string): string {
  if (typeof window === 'undefined') return '';

  const backendPort = process.env.NEXT_PUBLIC_API_PORT ?? '8000';
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';

  return `${protocol}//${window.location.hostname}:${backendPort}${path}`;
}
