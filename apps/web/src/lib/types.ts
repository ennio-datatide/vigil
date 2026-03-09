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
  sessionId?: string;
  executionId?: string;
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

// Pipeline execution types

export interface PipelineExecution {
  id: string;
  pipelineId: string;
  status: 'queued' | 'running' | 'completed' | 'failed';
  initialPrompt: string;
  projectPath: string;
  currentStepIndex: number;
  stepSessions: Record<string, string>;
  stepOutputs: Record<string, string>;
  createdAt: number;
  completedAt: number | null;
}

// Extended WebSocket message types

export type WsMessageExtended =
  | WsMessage
  | { type: 'child_spawned'; parentId: string; childId: string }
  | { type: 'child_completed'; parentId: string; childId: string; success: boolean }
  | { type: 'status_changed'; sessionId: string; oldStatus: string; newStatus: string }
  | { type: 'memory_updated'; memoryId: string }
  | { type: 'acta_refreshed'; projectPath: string };
