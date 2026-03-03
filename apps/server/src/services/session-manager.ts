import { and, eq, ne } from 'drizzle-orm';
import { nanoid } from 'nanoid';
import type { Db } from '../db/client.js';
import { chainRules, notifications, sessions } from '../db/schema.js';
import { updateSessionStatus } from '../utils/update-session-status.js';
import type { BusEvents, EventBus } from './event-bus.js';
import type { PipelineService } from './pipeline-service.js';

const AUTH_KEYWORDS = ['auth', 'token', 'expired', 'unauthorized', 'login'];
const NEEDS_INPUT_TYPES = ['elicitation_dialog', 'permission_prompt', 'idle_prompt'];

function containsAuthError(payload: Record<string, unknown>): boolean {
  const payloadStr = JSON.stringify(payload).toLowerCase();
  return AUTH_KEYWORDS.some((keyword) => payloadStr.includes(keyword));
}

export class SessionManager {
  private hookEventHandler: ((data: BusEvents['hook_event']) => void) | null = null;
  private spawnedHandler: ((data: BusEvents['session_spawned']) => void) | null = null;
  private spawnFailedHandler: ((data: BusEvents['session_spawn_failed']) => void) | null = null;

  private _pipelineService: PipelineService | null = null;

  constructor(
    private bus: EventBus,
    private db: Db,
  ) {}

  set pipelineService(service: PipelineService) {
    this._pipelineService = service;
  }

  start(): void {
    if (this.hookEventHandler) {
      return;
    }

    this.hookEventHandler = (data) => this.handleHookEvent(data);
    this.bus.on('hook_event', this.hookEventHandler);

    this.spawnedHandler = (data) => {
      updateSessionStatus(this.db, this.bus, data.sessionId, {
        status: 'running',
        worktreePath: data.worktreePath,
        startedAt: Date.now(),
        gitMetadata: data.gitMetadata,
      });
    };
    this.bus.on('session_spawned', this.spawnedHandler);

    this.spawnFailedHandler = (data) => {
      updateSessionStatus(
        this.db,
        this.bus,
        data.sessionId,
        {
          status: 'failed',
          endedAt: Date.now(),
          exitReason: 'error',
        },
        `Session failed to start: ${data.error}`,
      );
    };
    this.bus.on('session_spawn_failed', this.spawnFailedHandler);
  }

  stop(): void {
    if (this.hookEventHandler) {
      this.bus.off('hook_event', this.hookEventHandler);
      this.hookEventHandler = null;
    }
    if (this.spawnedHandler) {
      this.bus.off('session_spawned', this.spawnedHandler);
      this.spawnedHandler = null;
    }
    if (this.spawnFailedHandler) {
      this.bus.off('session_spawn_failed', this.spawnFailedHandler);
      this.spawnFailedHandler = null;
    }
  }

  handleProcessExit(sessionId: string, code: number | null): void {
    // Guard against calls after DB close (e.g., during shutdown)
    let session: typeof sessions.$inferSelect | undefined;
    try {
      session = this.db.select().from(sessions).where(eq(sessions.id, sessionId)).get();
    } catch {
      return;
    }

    if (!session || session.status !== 'running') {
      return;
    }

    // Stop hook didn't fire — use process exit code as fallback
    const status = code === 0 ? 'completed' : 'failed';
    const message =
      code === 0 ? 'Session completed successfully' : `Session exited with code ${code}`;

    updateSessionStatus(
      this.db,
      this.bus,
      sessionId,
      {
        status,
        endedAt: Date.now(),
        exitReason: code === 0 ? 'completed' : 'error',
      },
      message,
    );

    if (code === 0) {
      this.processChainRules(sessionId);
      this.emitNotification(sessionId, 'session_done', message);
    }
  }

  private handleHookEvent(data: BusEvents['hook_event']): void {
    const { sessionId, eventType, toolName, payload } = data;

    if (eventType === 'Stop') {
      this.handleStopEvent(sessionId, payload);
    } else if (eventType === 'Notification') {
      this.handleNotificationEvent(sessionId, payload);
    } else if (eventType === 'PreToolUse' && toolName === 'AskUserQuestion') {
      this.handleAskUserQuestion(sessionId, payload);
    }
  }

  private handleAskUserQuestion(sessionId: string, payload: Record<string, unknown>): void {
    // Extract the first question text from AskUserQuestion tool_input
    const toolInput = payload.tool_input as Record<string, unknown> | undefined;
    const questions = toolInput?.questions as Array<Record<string, unknown>> | undefined;
    const firstQuestion = (questions?.[0]?.question as string) ?? 'Claude needs your input';

    updateSessionStatus(this.db, this.bus, sessionId, { status: 'needs_input' }, firstQuestion);
    this.emitNotification(sessionId, 'needs_input', firstQuestion);
  }

  private handleStopEvent(sessionId: string, payload: Record<string, unknown>): void {
    const isAuth = containsAuthError(payload);

    if (isAuth) {
      updateSessionStatus(
        this.db,
        this.bus,
        sessionId,
        {
          status: 'auth_required',
          endedAt: Date.now(),
          exitReason: 'error',
        },
        'Session requires authentication',
      );

      const session = this.db.select().from(sessions).where(eq(sessions.id, sessionId)).get();

      if (session) {
        const otherRunningSessions = this.db
          .select()
          .from(sessions)
          .where(
            and(
              eq(sessions.status, 'running'),
              eq(sessions.agentType, session.agentType),
              ne(sessions.id, sessionId),
            ),
          )
          .all();

        for (const other of otherRunningSessions) {
          updateSessionStatus(
            this.db,
            this.bus,
            other.id,
            {
              status: 'auth_required',
              endedAt: Date.now(),
              exitReason: 'error',
            },
            'Session requires authentication (related agent auth failure)',
          );
        }

        this.bus.emit('auth_error', {
          agentType: session.agentType,
          sessionId,
        });
      }
    } else {
      updateSessionStatus(
        this.db,
        this.bus,
        sessionId,
        {
          status: 'completed',
          endedAt: Date.now(),
          exitReason: 'completed',
        },
        'Session completed successfully',
      );

      this.processChainRules(sessionId);
    }

    // Emit in-app notification
    const notifType = isAuth ? 'auth_required' : 'session_done';
    const notifMessage = isAuth
      ? 'Session requires authentication'
      : 'Session completed successfully';
    this.emitNotification(sessionId, notifType, notifMessage);
  }

  private handleNotificationEvent(sessionId: string, payload: Record<string, unknown>): void {
    // Claude Code Notification hook sends `notification_type` (e.g., 'elicitation_dialog',
    // 'permission_prompt'). Legacy/internal events use `type`. Support both.
    const notificationType =
      (payload.notification_type as string) ?? (payload.type as string) ?? 'info';
    const message = (payload.message as string) ?? '';

    // Map Claude Code notification types to our internal types
    const isNeedsInput =
      notificationType === 'needs_input' || NEEDS_INPUT_TYPES.includes(notificationType);

    const internalType = isNeedsInput ? 'needs_input' : notificationType;

    if (isNeedsInput) {
      updateSessionStatus(this.db, this.bus, sessionId, { status: 'needs_input' }, message);
    }

    this.emitNotification(sessionId, internalType, message);
  }

  /** Persist notification to DB and emit to event bus (in-app bell + dashboard). */
  private emitNotification(sessionId: string, type: string, message: string): void {
    this.db.insert(notifications).values({ sessionId, type, message, sentAt: Date.now() }).run();

    this.bus.emit('notification', { sessionId, type, message });
  }

  private processChainRules(sessionId: string): void {
    const session = this.db.select().from(sessions).where(eq(sessions.id, sessionId)).get();

    if (!session) {
      return;
    }

    // Pipeline-aware chaining: if this session belongs to a pipeline, advance to next step
    if (session.pipelineId && session.pipelineStepIndex !== null && this._pipelineService) {
      const nextStep = this._pipelineService.getNextStep(
        session.pipelineId,
        session.pipelineStepIndex,
      );
      if (nextStep) {
        const followUpId = nanoid(12);
        const nextIndex = session.pipelineStepIndex + 1;

        this.db
          .insert(sessions)
          .values({
            id: followUpId,
            projectPath: session.projectPath,
            prompt: nextStep.prompt,
            status: 'queued',
            agentType: nextStep.agent,
            parentId: sessionId,
            skillsUsed: JSON.stringify([nextStep.skill]),
            pipelineId: session.pipelineId,
            pipelineStepIndex: nextIndex,
          })
          .run();

        this.bus.emit('session_update', { sessionId: followUpId, status: 'queued' });
      }
      return; // Pipeline chaining handled; skip legacy chain rules
    }

    // Legacy chain rules fallback
    const parsedSkills: string[] = session.skillsUsed ? JSON.parse(session.skillsUsed) : [];
    const rules = this.db
      .select()
      .from(chainRules)
      .where(eq(chainRules.triggerEvent, 'Stop'))
      .all();

    for (const rule of rules) {
      if (rule.sourceSkill === null || parsedSkills.includes(rule.sourceSkill)) {
        const followUpId = nanoid(12);

        this.db
          .insert(sessions)
          .values({
            id: followUpId,
            projectPath: session.projectPath,
            prompt: `Run skill: ${rule.targetSkill}`,
            status: 'queued',
            agentType: session.agentType,
            parentId: sessionId,
            skillsUsed: JSON.stringify([rule.targetSkill]),
          })
          .run();

        this.bus.emit('session_update', { sessionId: followUpId, status: 'queued' });
      }
    }
  }
}
