import { eq } from 'drizzle-orm';
import { sessions } from '../db/schema.js';
import type { Db } from '../db/client.js';

export class RecoveryService {
  constructor(private db: Db) {}

  async recover(): Promise<{ interrupted: number }> {
    let interrupted = 0;

    // After server restart, no child processes exist in memory.
    // All previously-running sessions are orphaned — mark them interrupted.
    const runningSessions = this.db
      .select()
      .from(sessions)
      .where(eq(sessions.status, 'running'))
      .all();

    for (const session of runningSessions) {
      this.db.update(sessions)
        .set({ status: 'interrupted', endedAt: Date.now(), exitReason: 'error' })
        .where(eq(sessions.id, session.id))
        .run();
      interrupted++;
    }

    return { interrupted };
  }
}
