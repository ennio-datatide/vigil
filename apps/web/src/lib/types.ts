// Re-export shared types
export type {
  AgentTypeType,
  CreatePipelineInputType,
  CreateSessionInputType,
  NotificationMessage,
  Pipeline,
  PipelineEdge,
  PipelineStep,
  Session,
  SessionRoleType,
  SessionStatusType,
  UpdatePipelineInputType,
  WsMessage,
} from '@praefectus/shared';

// Frontend-specific types
import type { Session } from '@praefectus/shared';

export interface SessionStore {
  sessions: Record<string, Session>;
  setSession: (session: Session) => void;
  removeSession: (id: string) => void;
  syncAll: (sessions: Session[]) => void;
}
