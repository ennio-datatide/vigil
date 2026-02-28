interface TelegramConfig {
  botToken: string;
  chatId: string;
  dashboardUrl: string;
}

interface NotificationParams {
  sessionId: string;
  type: string;
  projectName: string;
  prompt: string;
}

export interface DigestProject {
  name: string;
  path: string;
  sessions: { id: string; status: string; prompt: string }[];
}

const EMOJI: Record<string, string> = {
  needs_input: '\u{1F514}',
  error: '\u{1F6A8}',
  auth_required: '\u{26A0}\u{FE0F}',
  session_done: '\u{2705}',
  interrupted: '\u{1F6D1}',
  completed: '\u{2705}',
  running: '\u{1F680}',
  queued: '\u{23F3}',
  failed: '\u{274C}',
  cancelled: '\u{1F6AB}',
};

function emojiFor(type: string): string {
  return EMOJI[type] ?? '\u{2705}';
}

export class TelegramNotifier {
  constructor(
    private config: TelegramConfig | null,
    private fetchFn: typeof fetch = fetch,
  ) {}

  updateConfig(config: TelegramConfig): void {
    this.config = config;
  }

  async send(params: NotificationParams): Promise<void> {
    if (!this.config) return;

    const text = [
      `${emojiFor(params.type)} Session ${params.type.replaceAll('_', ' ')}`,
      `Project: ${params.projectName}`,
      `Task: ${params.prompt.slice(0, 100)}`,
      '',
      `${this.config.dashboardUrl}/dashboard/sessions/${params.sessionId}`,
    ].join('\n');

    await this.sendRaw(text);
  }

  async sendDigest(projects: DigestProject[]): Promise<void> {
    if (!this.config) return;

    const lines: string[] = ['\u{1F4CB} Daily Status Report', ''];

    if (projects.length === 0) {
      lines.push('No registered projects.');
    }

    for (const project of projects) {
      lines.push(`\u{1F4C1} <b>${project.name}</b>`);

      if (project.sessions.length === 0) {
        lines.push('  No recent sessions');
      }

      for (const s of project.sessions) {
        lines.push(`  ${emojiFor(s.status)} ${s.status} — ${s.prompt.slice(0, 60)}`);
      }
      lines.push('');
    }

    lines.push(`${this.config.dashboardUrl}/dashboard`);

    await this.sendRaw(lines.join('\n'));
  }

  private async sendRaw(text: string): Promise<void> {
    if (!this.config) return;

    const res = await this.fetchFn(
      `https://api.telegram.org/bot${this.config.botToken}/sendMessage`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          chat_id: this.config.chatId,
          text,
          parse_mode: 'HTML',
        }),
      },
    );

    if (!res.ok) {
      const body = await res.text().catch(() => 'unknown');
      throw new Error(`Telegram API error ${res.status}: ${body}`);
    }
  }
}
