import { WORKTREE_RETENTION_HOURS } from '@praefectus/shared';
import { and, eq, isNotNull, lt } from 'drizzle-orm';
import type { Db } from '../db/client.js';
import { sessions } from '../db/schema.js';
import type { WorktreeManager } from './worktree-manager.js';

export class CleanupService {
  constructor(
    private db: Db,
    private worktreeManager: WorktreeManager,
  ) {}

  async cleanupWorktrees(): Promise<{ removed: number; skipped: number }> {
    let removed = 0;
    let skipped = 0;

    const cutoff = Date.now() - WORKTREE_RETENTION_HOURS * 60 * 60 * 1000;

    // Find completed/failed sessions older than retention period with worktree
    const oldSessions = this.db
      .select()
      .from(sessions)
      .where(and(isNotNull(sessions.worktreePath), lt(sessions.endedAt, cutoff)))
      .all();

    for (const session of oldSessions) {
      if (!session.worktreePath) continue;

      try {
        const hasChanges = await this.worktreeManager.hasUnmergedChanges(session.worktreePath);
        if (hasChanges) {
          skipped++;
          continue;
        }

        await this.worktreeManager.remove(session.worktreePath);
        this.db
          .update(sessions)
          .set({ worktreePath: null })
          .where(eq(sessions.id, session.id))
          .run();
        removed++;
      } catch (err) {
        // Log but continue — don't let one failed cleanup block others
        console.error(
          `Cleanup failed for session ${session.id} worktree ${session.worktreePath}:`,
          err,
        );
        skipped++;
      }
    }

    return { removed, skipped };
  }
}
