import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { join } from 'node:path';
import { existsSync } from 'node:fs';

const exec = promisify(execFile);

export class WorktreeManager {
  constructor(private worktreeBase: string) {}

  async create(repoPath: string, sessionId: string): Promise<{ worktreePath: string; branch: string }> {
    const branch = `praefectus/${sessionId}`;
    const worktreePath = join(this.worktreeBase, sessionId);

    await exec('git', ['worktree', 'add', '-b', branch, worktreePath], { cwd: repoPath });

    return { worktreePath, branch };
  }

  async remove(worktreePath: string): Promise<void> {
    if (!existsSync(worktreePath)) return;
    try {
      const { stdout } = await exec('git', ['rev-parse', '--git-common-dir'], { cwd: worktreePath });
      const gitDir = stdout.trim();
      const repoPath = join(gitDir, '..');
      await exec('git', ['worktree', 'remove', worktreePath, '--force'], { cwd: repoPath });
    } catch {
      // Worktree already gone
    }
  }

  async removeAll(repoPath: string): Promise<void> {
    try {
      const { stdout } = await exec('git', ['worktree', 'list', '--porcelain'], { cwd: repoPath });
      const worktrees = stdout.split('\n')
        .filter(line => line.startsWith('worktree '))
        .map(line => line.replace('worktree ', ''))
        .filter(path => path.includes(this.worktreeBase));

      for (const wt of worktrees) {
        await this.remove(wt);
      }
    } catch {
      // No worktrees
    }
  }

  async hasUnmergedChanges(worktreePath: string): Promise<boolean> {
    try {
      const { stdout } = await exec('git', ['status', '--porcelain'], { cwd: worktreePath });
      return stdout.trim().length > 0;
    } catch {
      return false;
    }
  }
}
