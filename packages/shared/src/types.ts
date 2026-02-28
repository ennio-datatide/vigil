import type { z } from 'zod';
import type {
  AgentType,
  CreatePipelineInput,
  CreateSessionInput,
  EventSchema,
  GitMetadataSchema,
  HookEventType,
  NotificationType,
  PipelineEdgeSchema,
  PipelineSchema,
  PipelineStepSchema,
  ProjectSchema,
  SessionRole,
  SessionSchema,
  SessionStatus,
  UpdatePipelineInput,
} from './schemas.js';

export type Session = z.infer<typeof SessionSchema>;
export type SessionStatusType = z.infer<typeof SessionStatus>;
export type AgentTypeType = z.infer<typeof AgentType>;
export type SessionRoleType = z.infer<typeof SessionRole>;
export type Event = z.infer<typeof EventSchema>;
export type Project = z.infer<typeof ProjectSchema>;
export type CreateSessionInputType = z.infer<typeof CreateSessionInput>;
export type GitMetadata = z.infer<typeof GitMetadataSchema>;
export type HookEventTypeType = z.infer<typeof HookEventType>;
export type NotificationTypeType = z.infer<typeof NotificationType>;
export type Pipeline = z.infer<typeof PipelineSchema>;
export type PipelineStep = z.infer<typeof PipelineStepSchema>;
export type PipelineEdge = z.infer<typeof PipelineEdgeSchema>;
export type CreatePipelineInputType = z.input<typeof CreatePipelineInput>;
export type UpdatePipelineInputType = z.input<typeof UpdatePipelineInput>;

// WebSocket message types (server → client)
export type WsMessage =
  | { type: 'state_sync'; sessions: Session[] }
  | { type: 'session_update'; session: Session }
  | { type: 'session_removed'; sessionId: string }
  | { type: 'notification'; notification: NotificationMessage };

export interface NotificationMessage {
  id: number;
  sessionId: string;
  type: NotificationTypeType;
  message: string;
  sentAt: number;
  readAt: number | null;
}
