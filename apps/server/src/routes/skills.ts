import { readdirSync } from 'node:fs';
import type { FastifyPluginAsync } from 'fastify';

const skillsRoute: FastifyPluginAsync = async (app) => {
  // List available skills by reading .md files from the skills directory
  app.get('/api/skills', async () => {
    try {
      const files = readdirSync(app.config.skillsDir);
      return files
        .filter((f) => f.endsWith('.md'))
        .map((f) => ({
          name: f.replace(/\.md$/, ''),
          path: `${app.config.skillsDir}/${f}`,
        }));
    } catch {
      return [];
    }
  });
};

export default skillsRoute;
