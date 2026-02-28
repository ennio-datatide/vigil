import { describe, expect, it, vi } from 'vitest';
import { TelegramNotifier } from './notifier.js';

const defaultConfig = {
  botToken: 'test-token',
  chatId: '12345',
  dashboardUrl: 'http://100.0.0.1:3000',
};

describe('TelegramNotifier', () => {
  it('should format and send a needs_input notification', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(defaultConfig, mockFetch);

    await notifier.send({
      sessionId: 'abc',
      type: 'needs_input',
      projectName: 'My Project',
      prompt: 'Add auth middleware',
    });

    expect(mockFetch).toHaveBeenCalledTimes(1);
    const [url, options] = mockFetch.mock.calls[0];
    expect(url).toContain('test-token');
    expect(options.body).toContain('needs input');
    expect(options.body).toContain('abc');
  });

  it('should not throw if telegram is not configured', async () => {
    const notifier = new TelegramNotifier(null);
    await expect(
      notifier.send({
        sessionId: 'abc',
        type: 'needs_input',
        projectName: 'Test',
        prompt: 'test',
      }),
    ).resolves.toBeUndefined();
  });

  it('should not call fetch when config is null', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(null, mockFetch);

    await notifier.send({
      sessionId: 'abc',
      type: 'needs_input',
      projectName: 'Test',
      prompt: 'test',
    });

    expect(mockFetch).not.toHaveBeenCalled();
  });

  it('should use correct emoji for error type', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(defaultConfig, mockFetch);

    await notifier.send({
      sessionId: 'sess-1',
      type: 'error',
      projectName: 'Proj',
      prompt: 'Do something',
    });

    const body = mockFetch.mock.calls[0][1].body;
    expect(body).toContain('\u{1F6A8}');
  });

  it('should use correct emoji for auth_required type', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(defaultConfig, mockFetch);

    await notifier.send({
      sessionId: 'sess-2',
      type: 'auth_required',
      projectName: 'Proj',
      prompt: 'Do something',
    });

    const body = mockFetch.mock.calls[0][1].body;
    expect(body).toContain('\u{26A0}\u{FE0F}');
  });

  it('should use correct emoji for session_done type (default case)', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(defaultConfig, mockFetch);

    await notifier.send({
      sessionId: 'sess-3',
      type: 'session_done',
      projectName: 'Proj',
      prompt: 'Do something',
    });

    const body = mockFetch.mock.calls[0][1].body;
    expect(body).toContain('\u{2705}');
  });

  it('should truncate prompt to 100 chars', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(defaultConfig, mockFetch);

    const longPrompt = 'A'.repeat(200);

    await notifier.send({
      sessionId: 'sess-4',
      type: 'needs_input',
      projectName: 'Proj',
      prompt: longPrompt,
    });

    const body = mockFetch.mock.calls[0][1].body;
    // The body should contain exactly 100 A's, not 200
    expect(body).toContain('A'.repeat(100));
    expect(body).not.toContain('A'.repeat(101));
  });

  it('should include dashboard URL with session ID link', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(defaultConfig, mockFetch);

    await notifier.send({
      sessionId: 'my-session-id',
      type: 'needs_input',
      projectName: 'Proj',
      prompt: 'Do something',
    });

    const body = mockFetch.mock.calls[0][1].body;
    expect(body).toContain('http://100.0.0.1:3000/dashboard/sessions/my-session-id');
  });

  it('should send to correct Telegram API URL', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(defaultConfig, mockFetch);

    await notifier.send({
      sessionId: 'abc',
      type: 'needs_input',
      projectName: 'Proj',
      prompt: 'Do something',
    });

    const url = mockFetch.mock.calls[0][0];
    expect(url).toBe('https://api.telegram.org/bottest-token/sendMessage');
  });

  it('should send with correct Content-Type and parse_mode', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(defaultConfig, mockFetch);

    await notifier.send({
      sessionId: 'abc',
      type: 'needs_input',
      projectName: 'Proj',
      prompt: 'task',
    });

    const options = mockFetch.mock.calls[0][1];
    expect(options.method).toBe('POST');
    expect(options.headers['Content-Type']).toBe('application/json');

    const parsed = JSON.parse(options.body);
    expect(parsed.chat_id).toBe('12345');
    expect(parsed.parse_mode).toBe('HTML');
  });

  it('should include project name in notification text', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    const notifier = new TelegramNotifier(defaultConfig, mockFetch);

    await notifier.send({
      sessionId: 'abc',
      type: 'needs_input',
      projectName: 'My Awesome Project',
      prompt: 'Do something',
    });

    const body = mockFetch.mock.calls[0][1].body;
    expect(body).toContain('My Awesome Project');
  });
});
