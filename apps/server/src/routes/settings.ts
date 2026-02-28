import type { FastifyPluginAsync } from 'fastify';
import { z } from 'zod';
import { TelegramNotifier } from '../services/notifier.js';

const TelegramSettingsSchema = z.object({
  botToken: z.string().min(1),
  chatId: z.string().min(1),
  dashboardUrl: z.string().url(),
  enabled: z.boolean(),
  events: z.array(z.string()),
});

const settingsRoute: FastifyPluginAsync = async (app) => {
  // Get Telegram settings (token masked)
  app.get('/api/settings/telegram', async () => {
    const config = app.settingsService.getTelegramConfig();
    if (!config) return { configured: false };
    return {
      configured: true,
      botToken: config.botToken.slice(0, 4) + '...' + config.botToken.slice(-4),
      chatId: config.chatId,
      dashboardUrl: config.dashboardUrl,
      enabled: config.enabled,
      events: config.events,
    };
  });

  // Save Telegram settings
  app.put('/api/settings/telegram', async (request, reply) => {
    const parsed = TelegramSettingsSchema.safeParse(request.body);
    if (!parsed.success) {
      return reply.status(400).send({ error: 'Invalid input', details: parsed.error.issues });
    }
    app.settingsService.setTelegramConfig(parsed.data);

    // Re-initialize notifier with new config
    const notifierConfig = parsed.data.enabled
      ? { botToken: parsed.data.botToken, chatId: parsed.data.chatId, dashboardUrl: parsed.data.dashboardUrl }
      : null;
    app.notifier = new TelegramNotifier(notifierConfig);

    return { ok: true };
  });

  // Test Telegram connection
  app.post('/api/settings/telegram/test', async (_request, reply) => {
    const config = app.settingsService.getTelegramConfig();
    if (!config || !config.enabled) {
      return reply.status(400).send({ error: 'Telegram not configured or disabled' });
    }

    const testNotifier = new TelegramNotifier({
      botToken: config.botToken,
      chatId: config.chatId,
      dashboardUrl: config.dashboardUrl,
    });

    try {
      await testNotifier.send({
        sessionId: 'test',
        type: 'info',
        projectName: 'Praefectus',
        prompt: 'Test notification — your Telegram integration is working!',
      });
      return { ok: true };
    } catch (err) {
      return reply.status(500).send({ error: 'Failed to send test message', details: String(err) });
    }
  });
};

export default settingsRoute;
