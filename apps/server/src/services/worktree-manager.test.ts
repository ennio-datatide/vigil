import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { execSync } from 'node:child_process';
import { mkdtempSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { WorktreeManager } from './worktree-manager.js';

describe('WorktreeManager', () => {
  let repoDir: string;
  let worktreeBase: string;
  let manager: WorktreeManager;

  beforeAll(() => {
    // Create a temp git repo
    repoDir = mkdtempSync(join(tmpdir(), 'pf-wt-test-'));
    execSync('git init && git commit --allow-empty -m "init"', { cwd: repoDir });
    worktreeBase = mkdtempSync(join(tmpdir(), 'pf-wt-base-'));
    manager = new WorktreeManager(worktreeBase);
  });

  afterAll(() => {
    manager.removeAll(repoDir);
  });

  it('should create a worktree', async () => {
    const result = await manager.create(repoDir, 'test-session-1');
    expect(result.worktreePath).toContain('test-session-1');
    expect(result.branch).toBe('praefectus/test-session-1');
    expect(existsSync(result.worktreePath)).toBe(true);
  });

  it('should remove a worktree', async () => {
    const { worktreePath } = await manager.create(repoDir, 'test-session-2');
    await manager.remove(worktreePath);
    expect(existsSync(worktreePath)).toBe(false);
  });

  it('should detect unmerged changes', async () => {
    const { worktreePath } = await manager.create(repoDir, 'test-session-3');
    // Create an uncommitted file
    execSync(`echo "test" > test-file.txt`, { cwd: worktreePath });
    execSync('git add test-file.txt', { cwd: worktreePath });

    const hasChanges = await manager.hasUnmergedChanges(worktreePath);
    expect(hasChanges).toBe(true);
  });
});
