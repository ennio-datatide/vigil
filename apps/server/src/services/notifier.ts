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
  oldStatus?: string;
  newStatus?: string;
  agentType?: string;
  gitBranch?: string;
  duration?: string;
  message?: string;
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

function formatDuration(ms: number): string {
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ${secs % 60}s`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ${mins % 60}m`;
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

    const statusLabel = (params.newStatus ?? params.type).replaceAll('_', ' ');
    const lines: string[] = [
      `${emojiFor(params.newStatus ?? params.type)} <b>${statusLabel}</b>`,
      '',
      `<b>Project:</b> ${params.projectName}`,
      `<b>Task:</b> ${params.prompt.slice(0, 120)}`,
    ];

    if (params.oldStatus && params.newStatus) {
      lines.push(
        `<b>Status:</b> ${params.oldStatus.replaceAll('_', ' ')} → ${params.newStatus.replaceAll('_', ' ')}`,
      );
    }

    if (params.agentType) {
      lines.push(`<b>Agent:</b> ${params.agentType}`);
    }

    if (params.gitBranch) {
      lines.push(`<b>Branch:</b> ${params.gitBranch}`);
    }

    if (params.duration) {
      lines.push(`<b>Duration:</b> ${params.duration}`);
    }

    if (params.message) {
      lines.push('', `💬 ${params.message.slice(0, 200)}`);
    }

    lines.push('', `${this.config.dashboardUrl}/dashboard/sessions/${params.sessionId}`);

    await this.sendRaw(lines.join('\n'));
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

  static formatDuration = formatDuration;
}
