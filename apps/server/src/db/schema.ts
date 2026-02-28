import { integer, sqliteTable, text } from 'drizzle-orm/sqlite-core';

export const sessions = sqliteTable('sessions', {
  id: text('id').primaryKey(),
  projectPath: text('project_path').notNull(),
  worktreePath: text('worktree_path'),
  tmuxSession: text('tmux_session'),
  prompt: text('prompt').notNull(),
  skillsUsed: text('skills_used'), // JSON array
  status: text('status').notNull().default('queued'),
  agentType: text('agent_type').notNull().default('claude'),
  role: text('role'),
  parentId: text('parent_id'),
  retryCount: integer('retry_count').default(0),
  startedAt: integer('started_at'),
  endedAt: integer('ended_at'),
  exitReason: text('exit_reason'),
  gitMetadata: text('git_metadata'), // JSON: { repoName, branch, commitHash, remoteUrl }
  pipelineId: text('pipeline_id'),
  pipelineStepIndex: integer('pipeline_step_index'),
});

export const events = sqliteTable('events', {
  id: integer('id').primaryKey({ autoIncrement: true }),
  sessionId: text('session_id').notNull(),
  eventType: text('event_type').notNull(),
  toolName: text('tool_name'),
  payload: text('payload'),
  timestamp: integer('timestamp').notNull(),
});

export const projects = sqliteTable('projects', {
  path: text('path').primaryKey(),
  name: text('name').notNull(),
  skillsDir: text('skills_dir'),
  lastUsedAt: integer('last_used_at'),
});

export const chainRules = sqliteTable('chain_rules', {
  id: integer('id').primaryKey({ autoIncrement: true }),
  triggerEvent: text('trigger_event').notNull(),
  sourceSkill: text('source_skill'),
  targetSkill: text('target_skill').notNull(),
  sameWorktree: integer('same_worktree').default(1),
});

export const pipelines = sqliteTable('pipelines', {
  id: text('id').primaryKey(),
  name: text('name').notNull(),
  description: text('description').default(''),
  steps: text('steps').notNull(), // JSON array of PipelineStep
  edges: text('edges').notNull(), // JSON array of PipelineEdge
  isDefault: integer('is_default').default(0),
  createdAt: integer('created_at').notNull(),
  updatedAt: integer('updated_at').notNull(),
});

export const notifications = sqliteTable('notifications', {
  id: integer('id').primaryKey({ autoIncrement: true }),
  sessionId: text('session_id').notNull(),
  type: text('type').notNull(),
  message: text('message').notNull(),
  sentAt: integer('sent_at'),
  readAt: integer('read_at'),
});

export const settings = sqliteTable('settings', {
  key: text('key').primaryKey(),
  value: text('value').notNull(),
});
