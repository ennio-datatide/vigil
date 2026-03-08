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
import type { Session, WsMessage } from '@praefectus/shared';

export interface SessionStore {
  sessions: Record<string, Session>;
  setSession: (session: Session) => void;
  removeSession: (id: string) => void;
  syncAll: (sessions: Session[]) => void;
}

// Vigil chat types

export interface VigilMessage {
  id: number;
  role: 'user' | 'vigil';
  content: string;
  embeddedCards: EmbeddedCard[] | null;
  createdAt: number;
}

export type EmbeddedCardType = 'blocker' | 'status' | 'completion' | 'acta';

export interface EmbeddedCard {
  type: EmbeddedCardType;
  sessionId?: string;
  sessionPrompt?: string;
  question?: string;
  summary?: string;
  childCount?: number;
  acta?: string;
}

// Extended WebSocket message types

export type WsMessageExtended =
  | WsMessage
  | { type: 'child_spawned'; parentId: string; childId: string }
  | { type: 'child_completed'; parentId: string; childId: string; success: boolean }
  | { type: 'status_changed'; sessionId: string; oldStatus: string; newStatus: string }
  | { type: 'memory_updated'; memoryId: string }
  | { type: 'acta_refreshed'; projectPath: string };
