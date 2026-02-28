import { eq } from 'drizzle-orm';
import type { Db } from '../db/client.js';
import { settings } from '../db/schema.js';

export interface TelegramSettings {
  botToken: string;
  chatId: string;
  dashboardUrl: string;
  enabled: boolean;
  events: string[];
}

export class SettingsService {
  constructor(private db: Db) {}

  get(key: string): string | null {
    const row = this.db.select().from(settings).where(eq(settings.key, key)).get();
    return row?.value ?? null;
  }

  set(key: string, value: string): void {
    this.db
      .insert(settings)
      .values({ key, value })
      .onConflictDoUpdate({ target: settings.key, set: { value } })
      .run();
  }

  getTelegramConfig(): TelegramSettings | null {
    const raw = this.get('telegram');
    if (!raw) return null;
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }

  setTelegramConfig(config: TelegramSettings): void {
    this.set('telegram', JSON.stringify(config));
  }
}
