import { CreateSessionInput } from '@praefectus/shared';
import { eq } from 'drizzle-orm';
import type { FastifyPluginAsync } from 'fastify';
import { nanoid } from 'nanoid';
import { sessions } from '../db/schema.js';
import { withParsedGitMetadata } from '../utils/parse-git-metadata.js';

const sessionsRoute: FastifyPluginAsync = async (app) => {
  // List sessions
  app.get('/api/sessions', async () => {
    const rows = app.db.select().from(sessions).all();
    return rows.map(withParsedGitMetadata);
  });

  // Get single session
  app.get<{ Params: { id: string } }>('/api/sessions/:id', async (request, reply) => {
    const result = app.db.select().from(sessions).where(eq(sessions.id, request.params.id)).get();
    if (!result) return reply.status(404).send({ error: 'Session not found' });
    return withParsedGitMetadata(result);
  });

  // Create session and spawn agent (interactive PTY)
  app.post('/api/sessions', async (request, reply) => {
    const parsed = CreateSessionInput.safeParse(request.body);
    if (!parsed.success) {
      return reply.status(400).send({ error: 'Invalid input', details: parsed.error.issues });
    }

    const { projectPath, prompt, skill, role, parentId, skipPermissions, pipelineId } = parsed.data;
    const id = nanoid(12);

    // If a pipeline is selected, use the first step's skill and prompt
    let effectiveSkill = skill;
    let effectivePrompt = prompt;
    let pipelineStepIndex: number | null = null;

    if (pipelineId) {
      const pipeline = app.pipelineService.get(pipelineId);
      if (pipeline && pipeline.steps.length > 0) {
        const firstStep = pipeline.steps[0];
        effectiveSkill = effectiveSkill ?? firstStep.skill;
        effectivePrompt = effectivePrompt || firstStep.prompt;
        pipelineStepIndex = 0;
      }
    }

    const session = app.db
      .insert(sessions)
      .values({
        id,
        projectPath,
        prompt: effectivePrompt,
        skillsUsed: effectiveSkill ? JSON.stringify([effectiveSkill]) : null,
        status: 'queued',
        agentType: 'claude',
        role: role ?? null,
        parentId: parentId ?? null,
        pipelineId: pipelineId ?? null,
        pipelineStepIndex,
      })
      .returning()
      .get();

    app.eventBus.emit('session_update', { sessionId: id, status: 'queued' });

    // Spawn interactive PTY session asynchronously
    // Status updates are handled via session_spawned / session_spawn_failed events
    app.agentSpawner
      .spawnInteractive({ sessionId: id, projectPath, prompt: effectivePrompt, skipPermissions })
      .catch((err) => {
        app.log.error({ err, sessionId: id }, 'Failed to spawn agent');
      });

    return reply.status(201).send(withParsedGitMetadata(session));
  });

  // Resume a completed session — spawns claude --continue in the same worktree
  app.post<{ Params: { id: string } }>('/api/sessions/:id/resume', async (request, reply) => {
    const original = app.db.select().from(sessions).where(eq(sessions.id, request.params.id)).get();
    if (!original) return reply.status(404).send({ error: 'Session not found' });

    // Only allow resuming stopped sessions
    if (!['completed', 'failed', 'cancelled', 'interrupted'].includes(original.status)) {
      return reply.status(400).send({ error: 'Session must be stopped to resume' });
    }

    if (!original.worktreePath) {
      return reply
        .status(400)
        .send({ error: 'No worktree path found for session — cannot resume' });
    }

    const id = nanoid(12);

    const newSession = app.db
      .insert(sessions)
      .values({
        id,
        projectPath: original.projectPath,
        prompt: 'Resumed conversation',
        status: 'queued',
        agentType: 'claude',
        parentId: original.id,
        gitMetadata: original.gitMetadata,
      })
      .returning()
      .get();

    app.eventBus.emit('session_update', { sessionId: id, status: 'queued' });

    // Spawn with --continue in the original worktree
    // Status updates are handled via session_spawned / session_spawn_failed events
    app.agentSpawner
      .spawnInteractive({
        sessionId: id,
        projectPath: original.projectPath,
        continueInWorktree: original.worktreePath,
      })
      .catch((err) => {
        app.log.error({ err, sessionId: id }, 'Failed to resume session');
      });

    return reply.status(201).send(withParsedGitMetadata(newSession));
  });

  // Cancel session (stop the agent but keep the record)
  app.delete<{ Params: { id: string } }>('/api/sessions/:id', async (request, reply) => {
    const existing = app.db.select().from(sessions).where(eq(sessions.id, request.params.id)).get();
    if (!existing) return reply.status(404).send({ error: 'Session not found' });

    // Kill the child process by session ID
    await app.agentSpawner.kill(request.params.id);

    const updated = app.db
      .update(sessions)
      .set({ status: 'cancelled', endedAt: Date.now(), exitReason: 'user_cancelled' })
      .where(eq(sessions.id, request.params.id))
      .returning()
      .get();

    app.eventBus.emit('session_update', { sessionId: request.params.id, status: 'cancelled' });

    if (!updated) return reply.status(500).send({ error: 'Failed to update session' });
    return withParsedGitMetadata(updated);
  });

  // Restart a completed/failed/cancelled session in-place (same ID, same worktree, --continue)
  app.post<{ Params: { id: string } }>('/api/sessions/:id/restart', async (request, reply) => {
    const session = app.db.select().from(sessions).where(eq(sessions.id, request.params.id)).get();
    if (!session) return reply.status(404).send({ error: 'Session not found' });

    if (!['completed', 'failed', 'cancelled', 'interrupted'].includes(session.status)) {
      return reply.status(400).send({ error: 'Session must be stopped to restart' });
    }

    if (!session.worktreePath) {
      return reply.status(400).send({ error: 'No worktree path — cannot restart' });
    }

    // Reset session status
    app.db
      .update(sessions)
      .set({ status: 'queued', endedAt: null, exitReason: null })
      .where(eq(sessions.id, session.id))
      .run();

    app.eventBus.emit('session_update', { sessionId: session.id, status: 'queued' });

    // Ensure output buffer exists (may be gone after server restart)
    app.outputManager.createBuffer(session.id);
    app.outputManager.append(
      session.id,
      '\r\n\x1b[36m[Restarting session with --continue...]\x1b[0m\r\n',
    );

    // Spawn new PTY with --continue in the same worktree
    app.agentSpawner
      .spawnInteractive({
        sessionId: session.id,
        projectPath: session.projectPath,
        continueInWorktree: session.worktreePath,
      })
      .catch((err) => {
        app.log.error({ err, sessionId: session.id }, 'Failed to restart session');
      });

    const updated = app.db.select().from(sessions).where(eq(sessions.id, session.id)).get();
    if (!updated) return reply.status(500).send({ error: 'Failed to fetch updated session' });
    return withParsedGitMetadata(updated);
  });

  // Permanently remove a session from the database
  app.delete<{ Params: { id: string } }>('/api/sessions/:id/remove', async (request, reply) => {
    const existing = app.db.select().from(sessions).where(eq(sessions.id, request.params.id)).get();
    if (!existing) return reply.status(404).send({ error: 'Session not found' });

    // Kill the agent if still running
    await app.agentSpawner.kill(request.params.id);

    // Delete from database
    app.db.delete(sessions).where(eq(sessions.id, request.params.id)).run();

    app.eventBus.emit('session_removed', { sessionId: request.params.id });

    return { ok: true };
  });
};

export default sessionsRoute;
