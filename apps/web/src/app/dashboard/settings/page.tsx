'use client';

import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { useSaveTelegramSettings, useTelegramSettingsQuery, useTestTelegram } from '@/lib/api';
import { useToast } from '@/lib/stores/toast-store';

const EVENT_OPTIONS = [
  { key: 'needs_input', label: 'Needs Input', desc: 'Agent is waiting for user input' },
  { key: 'error', label: 'Error', desc: 'Session failed with an error' },
  { key: 'auth_required', label: 'Auth Required', desc: 'Agent needs re-authentication' },
  { key: 'completed', label: 'Completed', desc: 'Session finished successfully' },
];

export default function SettingsPage() {
  const toast = useToast();
  const { data: health } = useQuery({
    queryKey: ['health'],
    queryFn: () => fetch('/health').then((r) => r.json()),
    refetchInterval: 10_000,
  });
  const { data: telegram } = useTelegramSettingsQuery();
  const saveMutation = useSaveTelegramSettings();
  const testMutation = useTestTelegram();

  const [botToken, setBotToken] = useState('');
  const [chatId, setChatId] = useState('');
  const [dashboardUrl, setDashboardUrl] = useState('');
  const [enabled, setEnabled] = useState(false);
  const [events, setEvents] = useState<string[]>(['needs_input', 'error', 'auth_required']);

  // Populate form when data loads
  useEffect(() => {
    if (telegram?.configured) {
      setBotToken(telegram.botToken ?? '');
      setChatId(telegram.chatId ?? '');
      setDashboardUrl(telegram.dashboardUrl ?? '');
      setEnabled(telegram.enabled ?? false);
      setEvents(telegram.events ?? ['needs_input', 'error', 'auth_required']);
    }
  }, [telegram]);

  const toggleEvent = (key: string) => {
    setEvents((prev) => (prev.includes(key) ? prev.filter((e) => e !== key) : [...prev, key]));
  };

  const handleSave = () => {
    saveMutation.mutate(
      { botToken, chatId, dashboardUrl, enabled, events },
      {
        onSuccess: () => toast.success('Telegram settings saved'),
        onError: () => toast.error('Failed to save settings'),
      },
    );
  };

  const handleTest = () => {
    testMutation.mutate(undefined, {
      onSuccess: () => toast.success('Test message sent!'),
      onError: () => toast.error('Failed to send test message'),
    });
  };

  const isConnected = health?.status === 'ok';

  return (
    <div className="mx-auto max-w-2xl space-y-8 p-6">
      <h2 className="text-xl font-semibold tracking-tight">Settings</h2>

      {/* Server Status */}
      <section className="glass rounded-xl p-6 space-y-4">
        <h3 className="mb-3 text-sm font-medium uppercase tracking-wider text-text-muted">
          Server Status
        </h3>
        <div className="flex items-center gap-2">
          <span
            className={`h-3 w-3 rounded-full ${isConnected ? 'bg-accent shadow-[0_0_6px_1px] shadow-accent/50' : 'bg-status-error'}`}
          />
          <span className="text-sm">{isConnected ? 'Connected' : 'Disconnected'}</span>
        </div>
        {!isConnected && (
          <p className="mt-2 text-xs text-text-muted">
            Run <code className="font-mono text-accent">praefectus up</code> to start the server.
          </p>
        )}
      </section>

      {/* Telegram Notifications */}
      <section className="glass rounded-xl p-6 space-y-4">
        <h3 className="mb-4 text-sm font-medium uppercase tracking-wider text-text-muted">
          Telegram Notifications
        </h3>
        <div className="space-y-4">
          <label className="flex items-center gap-3 cursor-pointer">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
              className="h-4 w-4 rounded accent-accent"
            />
            <span className="text-sm">Enable Telegram notifications</span>
          </label>

          <div>
            <label className="mb-1 block text-xs text-text-muted">Bot Token</label>
            <input
              type="password"
              value={botToken}
              onChange={(e) => setBotToken(e.target.value)}
              placeholder="123456:ABC-DEF..."
              className="w-full rounded-lg border border-border-subtle bg-bg px-3 py-2 text-sm font-mono focus-accent transition-colors"
            />
          </div>

          <div>
            <label className="mb-1 block text-xs text-text-muted">Chat ID</label>
            <input
              type="text"
              value={chatId}
              onChange={(e) => setChatId(e.target.value)}
              placeholder="-1001234567890"
              className="w-full rounded-lg border border-border-subtle bg-bg px-3 py-2 text-sm font-mono focus-accent transition-colors"
            />
          </div>

          <div>
            <label className="mb-1 block text-xs text-text-muted">Dashboard URL</label>
            <input
              type="url"
              value={dashboardUrl}
              onChange={(e) => setDashboardUrl(e.target.value)}
              placeholder="http://localhost:3000"
              className="w-full rounded-lg border border-border-subtle bg-bg px-3 py-2 text-sm focus-accent transition-colors"
            />
          </div>

          <div>
            <label className="mb-2 block text-xs text-text-muted">Notify on these events:</label>
            <div className="space-y-2">
              {EVENT_OPTIONS.map((opt) => (
                <label key={opt.key} className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={events.includes(opt.key)}
                    onChange={() => toggleEvent(opt.key)}
                    className="h-4 w-4 rounded accent-accent"
                  />
                  <span className="text-sm">{opt.label}</span>
                  <span className="text-xs text-text-muted">&mdash; {opt.desc}</span>
                </label>
              ))}
            </div>
          </div>

          <div className="flex gap-3 pt-2">
            <button
              type="button"
              onClick={handleSave}
              disabled={saveMutation.isPending}
              className="btn-press rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-hover transition-colors disabled:opacity-50"
            >
              {saveMutation.isPending ? 'Saving...' : 'Save'}
            </button>
            <button
              type="button"
              onClick={handleTest}
              disabled={testMutation.isPending || !enabled}
              className="btn-press rounded-lg border border-border-subtle px-4 py-2 text-sm text-text-muted hover:bg-surface-hover transition-colors disabled:opacity-50"
            >
              {testMutation.isPending ? 'Sending...' : 'Test Connection'}
            </button>
          </div>
        </div>
      </section>

      {/* Remote Access */}
      <section className="glass rounded-xl p-6 space-y-4">
        <h3 className="mb-3 text-sm font-medium uppercase tracking-wider text-text-muted">
          Remote Access
        </h3>
        <p className="mb-3 text-sm text-text-muted">
          Access this dashboard from your phone or another device on your network:
        </p>
        <div className="space-y-2 font-mono text-xs">
          <div className="rounded-lg bg-bg p-3 border border-border-subtle">
            <span className="text-text-muted">Local network: </span>
            <span className="text-accent select-all">
              http://{typeof window !== 'undefined' ? window.location.hostname : 'localhost'}:
              {typeof window !== 'undefined' ? window.location.port : '3000'}
            </span>
          </div>
          <div className="rounded-lg bg-bg p-3 border border-border-subtle">
            <span className="text-text-muted">Tailscale: </span>
            <span className="text-text">
              Run <code className="text-accent">tailscale ip -4</code> to get your Tailscale IP,
              then visit <code className="text-accent">http://&lt;tailscale-ip&gt;:3000</code>
            </span>
          </div>
        </div>
        <p className="mt-3 text-xs text-text-muted">
          Tip: Tailscale works from anywhere, not just your home WiFi. Install Tailscale on your
          phone and connect with the same account.
        </p>
      </section>

      {/* CLI Auth */}
      <section className="glass rounded-xl p-6 space-y-4">
        <h3 className="mb-3 text-sm font-medium uppercase tracking-wider text-text-muted">
          Agent Authentication
        </h3>
        <p className="mb-3 text-sm text-text-muted">
          If agents report auth errors, re-authenticate with:
        </p>
        <div className="space-y-2 font-mono text-xs">
          <div className="rounded-lg bg-bg p-3 border border-border-subtle text-accent">
            praefectus auth claude
          </div>
          <div className="rounded-lg bg-bg p-3 border border-border-subtle text-accent">
            praefectus auth codex
          </div>
        </div>
      </section>
    </div>
  );
}
