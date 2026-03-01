import { EventEmitter } from 'node:events';

export interface SessionRow {
  id: string;
  projectPath: string;
  prompt: string;
  status: string;
  agentType: string;
  gitMetadata: string | null;
  startedAt: number | null;
  endedAt: number | null;
  exitReason: string | null;
}

export interface BusEvents {
  session_update: { sessionId: string; status: string; [key: string]: unknown };
  status_changed: {
    session: SessionRow;
    oldStatus: string;
    newStatus: string;
    message?: string;
  };
  hook_event: {
    sessionId: string;
    eventType: string;
    toolName: string | null;
    payload: Record<string, unknown>;
    timestamp: number;
  };
  auth_error: { agentType: string; sessionId: string };
  notification: { sessionId: string; type: string; message: string };
  session_removed: { sessionId: string };
  session_spawned: { sessionId: string; worktreePath: string; gitMetadata: string | null };
  session_spawn_failed: { sessionId: string; error: string };
}

type EventName = keyof BusEvents;

export class EventBus {
  private emitter = new EventEmitter();

  constructor() {
    this.emitter.setMaxListeners(50);
  }

  on<K extends EventName>(event: K, handler: (data: BusEvents[K]) => void): void {
    this.emitter.on(event, handler);
  }

  off<K extends EventName>(event: K, handler: (data: BusEvents[K]) => void): void {
    this.emitter.removeListener(event, handler);
  }

  emit<K extends EventName>(event: K, data: BusEvents[K]): void {
    this.emitter.emit(event, data);
  }
}
