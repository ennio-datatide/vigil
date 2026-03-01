import { eq } from 'drizzle-orm';
import type { Db } from '../db/client.js';
import { sessions } from '../db/schema.js';
import type { EventBus } from '../services/event-bus.js';

export function updateSessionStatus(
  db: Db,
  eventBus: EventBus,
  sessionId: string,
  updates: Partial<typeof sessions.$inferInsert>,
  message?: string,
) {
  // Read old status before updating
  const before = db.select().from(sessions).where(eq(sessions.id, sessionId)).get();
  const oldStatus = before?.status ?? 'unknown';

  db.update(sessions).set(updates).where(eq(sessions.id, sessionId)).run();

  if (updates.status) {
    eventBus.emit('session_update', { sessionId, status: updates.status });

    // Emit status_changed when the status actually changed
    if (updates.status !== oldStatus) {
      const after = db.select().from(sessions).where(eq(sessions.id, sessionId)).get();
      if (after) {
        eventBus.emit('status_changed', {
          session: {
            id: after.id,
            projectPath: after.projectPath,
            prompt: after.prompt,
            status: after.status,
            agentType: after.agentType,
            gitMetadata: after.gitMetadata,
            startedAt: after.startedAt,
            endedAt: after.endedAt,
            exitReason: after.exitReason,
          },
          oldStatus,
          newStatus: updates.status,
          message,
        });
      }
    }
  }
}
