import { mkdirSync } from 'node:fs';
import cors from '@fastify/cors';
import websocket from '@fastify/websocket';
import Fastify from 'fastify';
import { type PraefectusConfig, resolveConfig } from './config.js';
import { createDb, type Db, initializeSchema } from './db/client.js';
import { registerAuthHook } from './middleware/auth.js';
import eventsRoute from './routes/events.js';
import fsRoute from './routes/fs.js';
import notificationsRoute from './routes/notifications.js';
import pipelinesRoute from './routes/pipelines.js';
import projectsRoute from './routes/projects.js';
import sessionsRoute from './routes/sessions.js';
import settingsRoute from './routes/settings.js';
import skillsRoute from './routes/skills.js';
import { AgentSpawner } from './services/agent-spawner.js';
import { CleanupService } from './services/cleanup.js';
import { DigestService } from './services/digest-service.js';
import { EventBus } from './services/event-bus.js';
import { TelegramNotifier } from './services/notifier.js';
import { OutputManager } from './services/output-manager.js';
import { PipelineService } from './services/pipeline-service.js';
import { PtyManager } from './services/pty-manager.js';
import { RecoveryService } from './services/recovery.js';
import { SessionManager } from './services/session-manager.js';
import { SettingsService } from './services/settings-service.js';
import { SkillManager } from './services/skill-manager.js';
import { WorktreeManager } from './services/worktree-manager.js';
import dashboardWs from './ws/dashboard.js';
import terminalWs from './ws/terminal.js';

declare module 'fastify' {
  interface FastifyInstance {
    config: PraefectusConfig;
    db: Db;
    sqlite: import('better-sqlite3').Database;
    eventBus: EventBus;
    outputManager: OutputManager;
    ptyManager: PtyManager;
    agentSpawner: AgentSpawner;
    sessionManager: SessionManager;
    settingsService: SettingsService;
    pipelineService: PipelineService;
    notifier: TelegramNotifier;
  }
}

const CLEANUP_INTERVAL_MS = 6 * 60 * 60 * 1000; // 6 hours

/** Map session status to the event type names used in Telegram settings filter. */
function mapStatusToEventType(status: string): string {
  switch (status) {
    case 'needs_input':
      return 'needs_input';
    case 'auth_required':
      return 'auth_required';
    case 'completed':
      return 'session_done';
    case 'failed':
    case 'cancelled':
    case 'interrupted':
      return 'error';
    case 'running':
      return 'running';
    case 'queued':
      return 'queued';
    default:
      return status;
  }
}

function parseGitMetadata(raw: string): { branch: string; repoName: string; commitHash: string } | null {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

export async function buildApp(overrides?: Partial<PraefectusConfig>) {
  const config = resolveConfig(overrides);

  // Ensure directories exist
  mkdirSync(config.praefectusHome, { recursive: true });
  mkdirSync(config.skillsDir, { recursive: true });
  mkdirSync(config.logsDir, { recursive: true });
  mkdirSync(config.worktreeBase, { recursive: true });

  const { sqlite, db } = createDb(config.dbPath);
  initializeSchema(sqlite);
  const eventBus = new EventBus();
  const settingsService = new SettingsService(db);
  const pipelineService = new PipelineService(db);
  pipelineService.seedDefault();
  const outputManager = new OutputManager();
  const worktreeManager = new WorktreeManager(config.worktreeBase);
  const skillManager = new SkillManager(config.skillsDir);

  // Create SessionManager (decoupled from spawner via EventBus)
  const sessionManager = new SessionManager(eventBus, db);
  sessionManager.pipelineService = pipelineService;
  const ptyManager = new PtyManager();

  // Create AgentSpawner with onExit callback that feeds into SessionManager
  const agentSpawner = new AgentSpawner(
    worktreeManager,
    skillManager,
    config,
    outputManager,
    ptyManager,
    eventBus,
    (sessionId, code) => sessionManager.handleProcessExit(sessionId, code),
  );

  // Recovery: mark orphaned sessions as interrupted and notify
  const recovery = new RecoveryService(db);
  const { interrupted } = await recovery.recover();
  sessionManager.notifyInterrupted(interrupted);

  // Cleanup: periodic worktree garbage collection
  const cleanupService = new CleanupService(db, worktreeManager);
  const cleanupInterval = setInterval(() => {
    cleanupService.cleanupWorktrees().catch((err) => app.log.error(err, 'Cleanup failed'));
  }, CLEANUP_INTERVAL_MS);

  // Telegram notifier (optional)
  const telegramConfig =
    config.telegram && config.dashboardUrl
      ? {
          botToken: config.telegram.botToken,
          chatId: config.telegram.chatId,
          dashboardUrl: config.dashboardUrl,
        }
      : null;
  const notifier = new TelegramNotifier(telegramConfig);

  // Telegram: fire on actual status changes with rich metadata
  eventBus.on('status_changed', (data) => {
    const tgConfig = settingsService.getTelegramConfig();
    if (!tgConfig?.enabled) return;

    // Map status to notification event types for filtering
    const eventType = mapStatusToEventType(data.newStatus);
    if (!tgConfig.events.includes(eventType)) return;

    notifier.updateConfig({
      botToken: tgConfig.botToken,
      chatId: tgConfig.chatId,
      dashboardUrl: tgConfig.dashboardUrl,
    });

    const s = data.session;
    const gitMeta = s.gitMetadata ? parseGitMetadata(s.gitMetadata) : null;
    const duration =
      s.startedAt && s.endedAt
        ? TelegramNotifier.formatDuration(s.endedAt - s.startedAt)
        : s.startedAt
          ? TelegramNotifier.formatDuration(Date.now() - s.startedAt)
          : undefined;

    notifier
      .send({
        sessionId: s.id,
        type: eventType,
        projectName: s.projectPath.split('/').pop() ?? s.projectPath,
        prompt: s.prompt,
        oldStatus: data.oldStatus,
        newStatus: data.newStatus,
        agentType: s.agentType,
        gitBranch: gitMeta?.branch,
        duration,
        message: data.message,
      })
      .catch((err) => app.log.error(err, 'Telegram notification failed'));
  });

  // Keep in-app notification listener for system-level alerts (recovery, test)
  eventBus.on('notification', (data) => {
    if (data.sessionId !== 'system') return;
    const tgConfig = settingsService.getTelegramConfig();
    if (!tgConfig?.enabled || !tgConfig.events.includes(data.type)) return;

    notifier.updateConfig({
      botToken: tgConfig.botToken,
      chatId: tgConfig.chatId,
      dashboardUrl: tgConfig.dashboardUrl,
    });

    notifier
      .send({
        sessionId: 'system',
        type: data.type,
        projectName: 'Praefectus',
        prompt: data.message,
      })
      .catch((err) => app.log.error(err, 'Telegram notification failed'));
  });

  const app = Fastify({ logger: true });

  await app.register(cors, { origin: true });
  await app.register(websocket);

  // Auth hook — must come before route registration
  registerAuthHook(app, config.apiToken);

  // Decorate with shared instances
  app.decorate('config', config);
  app.decorate('db', db);
  app.decorate('sqlite', sqlite);
  app.decorate('eventBus', eventBus);
  app.decorate('outputManager', outputManager);
  app.decorate('ptyManager', ptyManager);
  app.decorate('agentSpawner', agentSpawner);
  app.decorate('sessionManager', sessionManager);
  app.decorate('settingsService', settingsService);
  app.decorate('pipelineService', pipelineService);
  app.decorate('notifier', notifier);

  // Health check
  app.get('/health', async () => ({ status: 'ok' }));

  // REST routes
  await app.register(eventsRoute);
  await app.register(sessionsRoute);
  await app.register(projectsRoute);
  await app.register(skillsRoute);
  await app.register(notificationsRoute);
  await app.register(fsRoute);
  await app.register(settingsRoute);
  await app.register(pipelinesRoute);

  // WebSocket routes
  await app.register(terminalWs);
  await app.register(dashboardWs);

  // Start session manager (handles hook events)
  sessionManager.start();

  // Daily morning digest via Telegram
  const digestService = new DigestService(db, notifier, settingsService);
  digestService.start();

  // Graceful shutdown
  app.addHook('onClose', async () => {
    sessionManager.stop();
    digestService.stop();
    clearInterval(cleanupInterval);
    ptyManager.disposeAll();
    outputManager.disposeAll();
    sqlite.close();
  });

  return app;
}
