import type { FastifyPluginAsync } from 'fastify';
import { readdirSync, statSync } from 'node:fs';
import { resolve, dirname, basename } from 'node:path';
import { homedir } from 'node:os';

const fsRoute: FastifyPluginAsync = async (app) => {
  // List directories for path autocomplete
  app.get<{ Querystring: { prefix?: string } }>('/api/fs/dirs', async (request) => {
    const prefix = request.query.prefix?.trim() || '';

    // If empty or just ~, list home directory children
    const expanded = prefix.startsWith('~')
      ? prefix.replace('~', homedir())
      : prefix;

    if (!expanded || expanded === '/') {
      return { dirs: listDirs('/', 20) };
    }

    // Security: resolve to absolute, reject paths outside filesystem root
    const abs = resolve(expanded);

    // If the prefix ends with /, list children of that directory
    // Otherwise list siblings that match the partial name
    if (expanded.endsWith('/')) {
      return { dirs: listDirs(abs, 20) };
    }

    const parent = dirname(abs);
    const partial = basename(abs).toLowerCase();
    return { dirs: listDirs(parent, 20, partial) };
  });
};

function listDirs(dir: string, limit: number, filterPrefix?: string): string[] {
  try {
    const entries = readdirSync(dir, { withFileTypes: true });
    const dirs: string[] = [];

    for (const entry of entries) {
      if (dirs.length >= limit) break;
      if (entry.name.startsWith('.')) continue; // skip hidden
      if (!entry.isDirectory()) continue;
      if (filterPrefix && !entry.name.toLowerCase().startsWith(filterPrefix)) continue;

      const full = resolve(dir, entry.name);
      dirs.push(full);
    }

    return dirs.sort();
  } catch {
    return [];
  }
}

export default fsRoute;
