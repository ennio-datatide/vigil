import { eq } from 'drizzle-orm';
import { sessions } from '../db/schema.js';
import type { Db } from '../db/client.js';
import type { EventBus } from '../services/event-bus.js';

export function updateSessionStatus(
  db: Db,
  eventBus: EventBus,
  sessionId: string,
  updates: Partial<typeof sessions.$inferInsert>,
) {
  db.update(sessions).set(updates).where(eq(sessions.id, sessionId)).run();
  if (updates.status) {
    eventBus.emit('session_update', { sessionId, status: updates.status });
  }
}
