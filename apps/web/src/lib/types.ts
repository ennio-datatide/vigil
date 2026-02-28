// Re-export shared types
export type {
  Session,
  SessionStatusType,
  AgentTypeType,
  SessionRoleType,
  WsMessage,
  NotificationMessage,
  CreateSessionInputType,
  Pipeline,
  PipelineStep,
  PipelineEdge,
  CreatePipelineInputType,
  UpdatePipelineInputType,
} from '@praefectus/shared';

// Frontend-specific types
import type { Session } from '@praefectus/shared';

export interface SessionStore {
  sessions: Record<string, Session>;
  setSession: (session: Session) => void;
  removeSession: (id: string) => void;
  syncAll: (sessions: Session[]) => void;
}
