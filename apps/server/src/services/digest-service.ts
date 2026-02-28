import { desc, eq } from 'drizzle-orm';
import type { Db } from '../db/client.js';
import { projects, sessions } from '../db/schema.js';
import type { DigestProject, TelegramNotifier } from './notifier.js';
import type { SettingsService } from './settings-service.js';

const ONE_DAY_MS = 24 * 60 * 60 * 1000;

export class DigestService {
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private db: Db,
    private notifier: TelegramNotifier,
    private settingsService: SettingsService,
  ) {}

  start(): void {
    this.scheduleNext();
  }

  stop(): void {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  /** Calculate ms until next 8:00 AM local time. */
  private msUntilNextDigest(): number {
    const now = new Date();
    const next = new Date(now);
    next.setHours(8, 0, 0, 0);

    // If 8 AM already passed today, schedule for tomorrow
    if (next.getTime() <= now.getTime()) {
      next.setDate(next.getDate() + 1);
    }

    return next.getTime() - now.getTime();
  }

  private scheduleNext(): void {
    const delay = this.msUntilNextDigest();
    this.timer = setTimeout(() => {
      this.sendDigest().catch(() => {});
      this.scheduleNext();
    }, delay);
  }

  async sendDigest(): Promise<void> {
    const tgConfig = this.settingsService.getTelegramConfig();
    if (!tgConfig?.enabled) return;

    // Ensure notifier has the latest config from settings
    this.notifier.updateConfig({
      botToken: tgConfig.botToken,
      chatId: tgConfig.chatId,
      dashboardUrl: tgConfig.dashboardUrl,
    });

    const allProjects = this.db.select().from(projects).all();
    const since = Date.now() - ONE_DAY_MS;

    const digestProjects: DigestProject[] = allProjects.map((project) => {
      const recentSessions = this.db
        .select()
        .from(sessions)
        .where(eq(sessions.projectPath, project.path))
        .orderBy(desc(sessions.startedAt))
        .limit(5)
        .all()
        .filter(
          (s) =>
            (s.startedAt ?? 0) >= since || s.status === 'running' || s.status === 'needs_input',
        );

      return {
        name: project.name,
        path: project.path,
        sessions: recentSessions.map((s) => ({
          id: s.id,
          status: s.status,
          prompt: s.prompt,
        })),
      };
    });

    await this.notifier.sendDigest(digestProjects);
  }
}
