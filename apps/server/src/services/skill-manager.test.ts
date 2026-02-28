import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync, writeFileSync, mkdirSync, existsSync, readlinkSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { SkillManager } from './skill-manager.js';

describe('SkillManager', () => {
  let skillsDir: string;
  let worktreeDir: string;
  let manager: SkillManager;

  beforeAll(() => {
    skillsDir = mkdtempSync(join(tmpdir(), 'pf-skills-'));
    worktreeDir = mkdtempSync(join(tmpdir(), 'pf-worktree-'));
    manager = new SkillManager(skillsDir);

    // Create some test skills
    writeFileSync(join(skillsDir, 'implement.md'), '# Implement\nImplement the task.');
    writeFileSync(join(skillsDir, 'review.md'), '# Review\nReview the code.');
    writeFileSync(join(skillsDir, 'not-a-skill.txt'), 'ignore me');
  });

  it('should list available skills', () => {
    const skills = manager.listSkills();
    expect(skills).toHaveLength(2);
    expect(skills.map(s => s.name)).toContain('implement');
    expect(skills.map(s => s.name)).toContain('review');
    // Should NOT include .txt files
    expect(skills.map(s => s.name)).not.toContain('not-a-skill');
  });

  it('should install skills into a worktree', async () => {
    await manager.installSkills(worktreeDir);

    const claudeSkillsDir = join(worktreeDir, '.claude', 'skills');
    expect(existsSync(claudeSkillsDir)).toBe(true);

    const installed = readdirSync(claudeSkillsDir);
    expect(installed).toContain('implement.md');
    expect(installed).toContain('review.md');
    expect(installed).not.toContain('not-a-skill.txt');
  });

  it('should symlink skills (not copy)', async () => {
    const claudeSkillsDir = join(worktreeDir, '.claude', 'skills');
    const linkTarget = readlinkSync(join(claudeSkillsDir, 'implement.md'));
    expect(linkTarget).toBe(join(skillsDir, 'implement.md'));
  });

  it('should not overwrite existing project skills', async () => {
    const worktree2 = mkdtempSync(join(tmpdir(), 'pf-wt2-'));
    const projectSkillsDir = join(worktree2, '.claude', 'skills');
    mkdirSync(projectSkillsDir, { recursive: true });
    writeFileSync(join(projectSkillsDir, 'custom.md'), '# Custom Skill');

    await manager.installSkills(worktree2);

    // Custom skill should still be there
    expect(existsSync(join(projectSkillsDir, 'custom.md'))).toBe(true);
    // Shared skills also installed
    expect(existsSync(join(projectSkillsDir, 'implement.md'))).toBe(true);
  });

  it('should return empty list when skills dir is empty or missing', () => {
    const emptyManager = new SkillManager('/tmp/nonexistent-skills-dir');
    expect(emptyManager.listSkills()).toEqual([]);
  });
});
