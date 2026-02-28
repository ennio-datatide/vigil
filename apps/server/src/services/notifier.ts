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

export class TelegramNotifier {
  constructor(
    private config: TelegramConfig | null,
    private fetchFn: typeof fetch = fetch,
  ) {}

  async send(params: NotificationParams): Promise<void> {
    if (!this.config) return;

    const emoji = params.type === 'needs_input' ? '\u{1F514}' :
                  params.type === 'error' ? '\u{1F6A8}' :
                  params.type === 'auth_required' ? '\u{26A0}\u{FE0F}' :
                  '\u{2705}';

    const text = [
      `${emoji} Session ${params.type.replace('_', ' ')}`,
      `Project: ${params.projectName}`,
      `Task: ${params.prompt.slice(0, 100)}`,
      '',
      `${this.config.dashboardUrl}/dashboard/sessions/${params.sessionId}`,
    ].join('\n');

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
