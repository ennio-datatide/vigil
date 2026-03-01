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
    <div className="mx-auto max-w-[640px] space-y-7 p-6 md:py-8">
      <div>
        <h2 className="text-[28px] font-extrabold -tracking-[0.04em] text-text">Settings</h2>
        <p className="mt-1 text-[13px] text-text-faint">Server configuration and integrations</p>
      </div>

      {/* Server Status */}
      <section className="flex items-center justify-between rounded-xl border border-border-subtle bg-[rgba(255,255,255,0.025)] p-5">
        <div>
          <h3 className="text-sm font-semibold text-text">Server Status</h3>
          <p className="mt-1 text-xs text-text-faint">Praefectus daemon on port 4000</p>
        </div>
        <div className="flex items-center gap-2">
          <span
            className={`h-2 w-2 rounded-full ${
              isConnected
                ? 'bg-status-working shadow-[0_0_8px] shadow-status-working/50'
                : 'bg-status-error'
            }`}
          />
          <span
            className={`text-[13px] font-medium ${isConnected ? 'text-status-working' : 'text-status-error'}`}
          >
            {isConnected ? 'Connected' : 'Disconnected'}
          </span>
        </div>
      </section>

      {/* Telegram Notifications */}
      <section className="space-y-5 rounded-xl border border-border-subtle bg-[rgba(255,255,255,0.025)] p-6">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-sm font-semibold text-text">Telegram Notifications</h3>
            <p className="mt-1 text-xs text-text-faint">Get alerts when agents need attention</p>
          </div>
          <button
            type="button"
            onClick={() => setEnabled((e) => !e)}
            className={`relative h-[22px] w-10 rounded-full transition-colors ${
              enabled ? 'bg-accent' : 'bg-surface-hover'
            }`}
          >
            <span
              className={`absolute top-[2px] h-[18px] w-[18px] rounded-full bg-white transition-all ${
                enabled ? 'left-[20px]' : 'left-[2px]'
              }`}
            />
          </button>
        </div>

        <div className="space-y-3.5">
          <div>
            <label className="mb-1.5 block text-xs font-medium text-text-muted">Bot Token</label>
            <input
              type="password"
              value={botToken}
              onChange={(e) => setBotToken(e.target.value)}
              placeholder="123456:ABC-DEF..."
              className="w-full rounded-lg border border-border-subtle bg-[rgba(255,255,255,0.03)] px-3.5 py-2.5 font-mono text-xs text-text-muted focus-accent transition-colors"
            />
          </div>
          <div>
            <label className="mb-1.5 block text-xs font-medium text-text-muted">Chat ID</label>
            <input
              type="text"
              value={chatId}
              onChange={(e) => setChatId(e.target.value)}
              placeholder="-1001234567890"
              className="w-full rounded-lg border border-border-subtle bg-[rgba(255,255,255,0.03)] px-3.5 py-2.5 font-mono text-xs text-text-muted focus-accent transition-colors"
            />
          </div>
          <div>
            <label className="mb-1.5 block text-xs font-medium text-text-muted">Dashboard URL</label>
            <input
              type="url"
              value={dashboardUrl}
              onChange={(e) => setDashboardUrl(e.target.value)}
              placeholder="http://localhost:3000"
              className="w-full rounded-lg border border-border-subtle bg-[rgba(255,255,255,0.03)] px-3.5 py-2.5 text-xs text-text-muted focus-accent transition-colors"
            />
          </div>
        </div>

        <div>
          <label className="mb-2.5 block text-xs font-medium text-text-muted">Notify on</label>
          <div className="flex flex-wrap gap-2">
            {EVENT_OPTIONS.map((opt) => {
              const checked = events.includes(opt.key);
              return (
                <button
                  key={opt.key}
                  type="button"
                  onClick={() => toggleEvent(opt.key)}
                  className={`flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs transition-colors ${
                    checked
                      ? 'border border-accent/20 bg-accent/10 text-text-muted'
                      : 'border border-border-subtle bg-[rgba(255,255,255,0.02)] text-text-faint'
                  }`}
                >
                  {checked ? (
                    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" className="shrink-0">
                      <rect width="14" height="14" rx="3" className="fill-accent" />
                      <path
                        d="M3.5 7L6 9.5L10.5 4.5"
                        stroke="white"
                        strokeWidth="1.5"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  ) : (
                    <span className="h-3.5 w-3.5 shrink-0 rounded-[3px] border border-border" />
                  )}
                  {opt.label}
                </button>
              );
            })}
          </div>
        </div>

        <div className="flex items-center gap-2.5">
          <button
            type="button"
            onClick={handleSave}
            disabled={saveMutation.isPending}
            className="btn-press rounded-lg bg-accent px-5 py-2.5 text-[13px] font-medium text-white hover:bg-accent-hover transition-colors disabled:opacity-50"
          >
            {saveMutation.isPending ? 'Saving...' : 'Save'}
          </button>
          <button
            type="button"
            onClick={handleTest}
            disabled={testMutation.isPending || !enabled}
            className="btn-press rounded-lg border border-border-subtle px-5 py-2.5 text-[13px] font-medium text-text-muted hover:bg-surface-hover transition-colors disabled:opacity-50"
          >
            {testMutation.isPending ? 'Sending...' : 'Test Connection'}
          </button>
        </div>
      </section>

      {/* Agent Authentication */}
      <section className="space-y-4 rounded-xl border border-border-subtle bg-[rgba(255,255,255,0.025)] p-6">
        <div>
          <h3 className="text-sm font-semibold text-text">Agent Authentication</h3>
          <p className="mt-1 text-xs text-text-faint">Re-authenticate agents using the CLI</p>
        </div>
        <div className="space-y-2">
          <div className="rounded-lg border border-[rgba(255,255,255,0.04)] bg-[rgba(0,0,0,0.3)] px-3.5 py-2.5 font-mono text-xs text-text-muted">
            $ praefectus auth claude
          </div>
          <div className="rounded-lg border border-[rgba(255,255,255,0.04)] bg-[rgba(0,0,0,0.3)] px-3.5 py-2.5 font-mono text-xs text-text-muted">
            $ praefectus auth codex
          </div>
        </div>
      </section>
    </div>
  );
}
