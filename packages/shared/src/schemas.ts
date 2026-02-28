import { z } from 'zod';

export const SessionStatus = z.enum([
  'queued',
  'running',
  'needs_input',
  'auth_required',
  'completed',
  'failed',
  'cancelled',
  'interrupted',
]);

export const AgentType = z.enum(['claude', 'codex']);

export const SessionRole = z.enum(['implementer', 'reviewer', 'fixer', 'custom']);

export const ExitReason = z.enum(['completed', 'error', 'user_cancelled', 'chain_triggered']);

export const HookEventType = z.enum([
  'SessionStart',
  'PreToolUse',
  'PostToolUse',
  'PostToolUseFailure',
  'Stop',
  'SubagentStart',
  'SubagentStop',
  'Notification',
]);

export const NotificationType = z.enum([
  'needs_input',
  'error',
  'auth_required',
  'chain_complete',
  'session_done',
]);

export const GitMetadataSchema = z.object({
  repoName: z.string(),
  branch: z.string(),
  commitHash: z.string(),
  remoteUrl: z.string().nullable(),
});

export const SessionSchema = z.object({
  id: z.string(),
  projectPath: z.string(),
  worktreePath: z.string().nullable(),
  tmuxSession: z.string().nullable(),
  prompt: z.string(),
  skillsUsed: z.array(z.string()).nullable(),
  status: SessionStatus,
  agentType: AgentType,
  role: SessionRole.nullable(),
  parentId: z.string().nullable(),
  retryCount: z.number().default(0),
  startedAt: z.number().nullable(),
  endedAt: z.number().nullable(),
  exitReason: ExitReason.nullable(),
  gitMetadata: GitMetadataSchema.nullable().optional(),
  pipelineId: z.string().nullable().optional(),
  pipelineStepIndex: z.number().nullable().optional(),
});

export const EventSchema = z.object({
  id: z.number(),
  sessionId: z.string(),
  eventType: HookEventType,
  toolName: z.string().nullable(),
  payload: z.string(),
  timestamp: z.number(),
});

export const ProjectSchema = z.object({
  path: z.string(),
  name: z.string(),
  skillsDir: z.string().nullable(),
  lastUsedAt: z.number().nullable(),
});

export const PipelineStepSchema = z.object({
  id: z.string(),
  skill: z.string(),
  label: z.string(),
  agent: AgentType,
  prompt: z.string(),
  position: z.object({ x: z.number(), y: z.number() }),
});

export const PipelineEdgeSchema = z.object({
  source: z.string(),
  target: z.string(),
});

export const PipelineSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string(),
  steps: z.array(PipelineStepSchema),
  edges: z.array(PipelineEdgeSchema),
  isDefault: z.boolean(),
  createdAt: z.number(),
  updatedAt: z.number(),
});

export const CreatePipelineInput = z.object({
  name: z.string(),
  description: z.string().optional().default(''),
  steps: z.array(PipelineStepSchema),
  edges: z.array(PipelineEdgeSchema),
  isDefault: z.boolean().optional().default(false),
});

export const UpdatePipelineInput = z.object({
  name: z.string().optional(),
  description: z.string().optional(),
  steps: z.array(PipelineStepSchema).optional(),
  edges: z.array(PipelineEdgeSchema).optional(),
  isDefault: z.boolean().optional(),
});

export const CreateSessionInput = z.object({
  projectPath: z.string(),
  prompt: z.string(),
  skill: z.string().optional(),
  role: SessionRole.optional(),
  parentId: z.string().optional(),
  skipPermissions: z.boolean().optional(),
  pipelineId: z.string().optional(),
});

export const HookPayload = z.object({
  session_id: z.string(),
  data: z.record(z.unknown()),
});
