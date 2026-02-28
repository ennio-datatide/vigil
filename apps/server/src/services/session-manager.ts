import { eq, and, ne } from 'drizzle-orm';
import { nanoid } from 'nanoid';
import { sessions, chainRules } from '../db/schema.js';
import type { Db } from '../db/client.js';
import type { EventBus, BusEvents } from './event-bus.js';
import type { PipelineService } from './pipeline-service.js';
import { updateSessionStatus } from '../utils/update-session-status.js';

const AUTH_KEYWORDS = ['auth', 'token', 'expired', 'unauthorized', 'login'];

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
      updateSessionStatus(this.db, this.bus, data.sessionId, {
        status: 'failed',
        endedAt: Date.now(),
        exitReason: 'error',
      });
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
    let session;
    try {
      session = this.db
        .select()
        .from(sessions)
        .where(eq(sessions.id, sessionId))
        .get();
    } catch {
      return;
    }

    if (!session || session.status !== 'running') {
      return;
    }

    // Stop hook didn't fire — use process exit code as fallback
    const status = code === 0 ? 'completed' : 'failed';
    updateSessionStatus(this.db, this.bus, sessionId, {
      status,
      endedAt: Date.now(),
      exitReason: code === 0 ? 'completed' : 'error',
    });

    if (code === 0) {
      this.processChainRules(sessionId);
    }
  }

  private handleHookEvent(data: BusEvents['hook_event']): void {
    const { sessionId, eventType, payload } = data;

    if (eventType === 'Stop') {
      this.handleStopEvent(sessionId, payload);
    } else if (eventType === 'Notification') {
      this.handleNotificationEvent(sessionId, payload);
    }
  }

  private handleStopEvent(sessionId: string, payload: Record<string, unknown>): void {
    const isAuth = containsAuthError(payload);

    if (isAuth) {
      this.db
        .update(sessions)
        .set({
          status: 'auth_required',
          endedAt: Date.now(),
          exitReason: 'error',
        })
        .where(eq(sessions.id, sessionId))
        .run();

      const session = this.db
        .select()
        .from(sessions)
        .where(eq(sessions.id, sessionId))
        .get();

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
          updateSessionStatus(this.db, this.bus, other.id, {
            status: 'auth_required', endedAt: Date.now(), exitReason: 'error',
          });
        }

        this.bus.emit('auth_error', {
          agentType: session.agentType,
          sessionId,
        });
      }
    } else {
      this.db
        .update(sessions)
        .set({
          status: 'completed',
          endedAt: Date.now(),
          exitReason: 'completed',
        })
        .where(eq(sessions.id, sessionId))
        .run();

      this.processChainRules(sessionId);
    }

    this.bus.emit('session_update', {
      sessionId,
      status: isAuth ? 'auth_required' : 'completed',
    });
  }

  private handleNotificationEvent(sessionId: string, payload: Record<string, unknown>): void {
    const type = (payload.type as string) ?? 'info';
    const message = (payload.message as string) ?? '';

    if (type === 'needs_input') {
      this.db
        .update(sessions)
        .set({ status: 'needs_input' })
        .where(eq(sessions.id, sessionId))
        .run();
    }

    this.bus.emit('notification', {
      sessionId,
      type,
      message,
    });
  }

  private processChainRules(sessionId: string): void {
    const session = this.db
      .select()
      .from(sessions)
      .where(eq(sessions.id, sessionId))
      .get();

    if (!session) {
      return;
    }

    // Pipeline-aware chaining: if this session belongs to a pipeline, advance to next step
    if (session.pipelineId && session.pipelineStepIndex !== null && this._pipelineService) {
      const nextStep = this._pipelineService.getNextStep(session.pipelineId, session.pipelineStepIndex);
      if (nextStep) {
        const followUpId = nanoid(12);
        const nextIndex = session.pipelineStepIndex + 1;

        this.db.insert(sessions).values({
          id: followUpId,
          projectPath: session.projectPath,
          prompt: nextStep.prompt,
          status: 'queued',
          agentType: nextStep.agent,
          parentId: sessionId,
          skillsUsed: JSON.stringify([nextStep.skill]),
          pipelineId: session.pipelineId,
          pipelineStepIndex: nextIndex,
        }).run();

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

        this.db.insert(sessions).values({
          id: followUpId,
          projectPath: session.projectPath,
          prompt: `Run skill: ${rule.targetSkill}`,
          status: 'queued',
          agentType: session.agentType,
          parentId: sessionId,
          skillsUsed: JSON.stringify([rule.targetSkill]),
        }).run();

        this.bus.emit('session_update', { sessionId: followUpId, status: 'queued' });
      }
    }
  }
}
