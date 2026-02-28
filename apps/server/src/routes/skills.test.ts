import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { buildApp } from '../app.js';

describe('skills routes', () => {
  const praefectusHome = `/tmp/pf-test-skills-${Date.now()}`;
  let app: Awaited<ReturnType<typeof buildApp>>;

  beforeAll(async () => {
    app = await buildApp({ praefectusHome });
  });

  afterAll(async () => {
    await app.close();
  });

  it('GET /api/skills should return empty array when skills dir is empty', async () => {
    const res = await app.inject({ method: 'GET', url: '/api/skills' });
    expect(res.statusCode).toBe(200);
    expect(res.json()).toEqual([]);
  });

  it('GET /api/skills should return skill names when .md files exist', async () => {
    const skillsDir = join(praefectusHome, 'skills');
    writeFileSync(join(skillsDir, 'code-review.md'), '# Code Review Skill');
    writeFileSync(join(skillsDir, 'testing.md'), '# Testing Skill');
    writeFileSync(join(skillsDir, 'notes.txt'), 'not a skill');

    const res = await app.inject({ method: 'GET', url: '/api/skills' });
    expect(res.statusCode).toBe(200);
    const body = res.json();
    expect(body).toHaveLength(2);

    const names = body.map((s: { name: string }) => s.name).sort();
    expect(names).toEqual(['code-review', 'testing']);

    // Each entry should have a path
    for (const skill of body) {
      expect(skill.path).toContain(skillsDir);
      expect(skill.path).toMatch(/\.md$/);
    }
  });
});
