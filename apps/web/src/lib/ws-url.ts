import { getToken } from './auth-token';

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
  const base = `${protocol}//${window.location.hostname}:${backendPort}${path}`;

  const token = getToken();
  if (token) {
    const separator = path.includes('?') ? '&' : '?';
    return `${base}${separator}token=${encodeURIComponent(token)}`;
  }
  return base;
}
