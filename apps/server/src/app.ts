import Fastify from 'fastify';
import cors from '@fastify/cors';
import websocket from '@fastify/websocket';
import { mkdirSync } from 'node:fs';
import { resolveConfig, type PraefectusConfig } from './config.js';
import { createDb, initializeSchema, type Db } from './db/client.js';
import { EventBus } from './services/event-bus.js';
import { OutputManager } from './services/output-manager.js';
import { WorktreeManager } from './services/worktree-manager.js';
import { SkillManager } from './services/skill-manager.js';
import { AgentSpawner } from './services/agent-spawner.js';
import { PtyManager } from './services/pty-manager.js';
import { SessionManager } from './services/session-manager.js';
import { RecoveryService } from './services/recovery.js';
import { CleanupService } from './services/cleanup.js';
import { TelegramNotifier } from './services/notifier.js';
import { SettingsService } from './services/settings-service.js';
import { PipelineService } from './services/pipeline-service.js';
import { eq } from 'drizzle-orm';
import { sessions } from './db/schema.js';
import terminalWs from './ws/terminal.js';
import dashboardWs from './ws/dashboard.js';
import eventsRoute from './routes/events.js';
import sessionsRoute from './routes/sessions.js';
import projectsRoute from './routes/projects.js';
import skillsRoute from './routes/skills.js';
import notificationsRoute from './routes/notifications.js';
import fsRoute from './routes/fs.js';
import settingsRoute from './routes/settings.js';
import pipelinesRoute from './routes/pipelines.js';

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

  // Recovery: mark orphaned sessions as interrupted
  const recovery = new RecoveryService(db);
  await recovery.recover();

  // Cleanup: periodic worktree garbage collection
  const cleanupService = new CleanupService(db, worktreeManager);
  const cleanupInterval = setInterval(() => {
    cleanupService.cleanupWorktrees().catch(err => app.log.error(err, 'Cleanup failed'));
  }, CLEANUP_INTERVAL_MS);

  // Telegram notifier (optional)
  const telegramConfig = config.telegram && config.dashboardUrl
    ? { botToken: config.telegram.botToken, chatId: config.telegram.chatId, dashboardUrl: config.dashboardUrl }
    : null;
  const notifier = new TelegramNotifier(telegramConfig);

  eventBus.on('notification', (data) => {
    const telegramConfig = settingsService.getTelegramConfig();
    if (telegramConfig && telegramConfig.enabled && telegramConfig.events.includes(data.type)) {
      const session = db.select().from(sessions).where(eq(sessions.id, data.sessionId)).get();
      if (session) {
        app.notifier.send({
          sessionId: data.sessionId,
          type: data.type,
          projectName: session.projectPath.split('/').pop() ?? session.projectPath,
          prompt: session.prompt,
        }).catch(err => app.log.error(err, 'Telegram notification failed'));
      }
    }
  });

  const app = Fastify({ logger: true });

  await app.register(cors, { origin: true });
  await app.register(websocket);

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

  // Graceful shutdown
  app.addHook('onClose', async () => {
    sessionManager.stop();
    clearInterval(cleanupInterval);
    ptyManager.disposeAll();
    outputManager.disposeAll();
    sqlite.close();
  });

  return app;
}
