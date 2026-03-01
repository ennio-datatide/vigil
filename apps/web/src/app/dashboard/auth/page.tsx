'use client';

import { useCallback, useEffect, useState } from 'react';
import { getToken, setToken, clearToken } from '@/lib/auth-token';

export default function AuthPage() {
  const [tokenInput, setTokenInput] = useState('');
  const [saved, setSaved] = useState(false);
  const [hasToken, setHasToken] = useState(false);

  useEffect(() => {
    setHasToken(!!getToken());
  }, []);

  const handleSave = useCallback(() => {
    if (!tokenInput.trim()) return;
    setToken(tokenInput.trim());
    setHasToken(true);
    setSaved(true);
    setTokenInput('');
    setTimeout(() => {
      window.location.href = '/dashboard';
    }, 500);
  }, [tokenInput]);

  const handleClear = useCallback(() => {
    clearToken();
    setHasToken(false);
    setSaved(false);
  }, []);

  return (
    <div className="p-4 max-w-md">
      <h2 className="mb-4 text-xl font-semibold tracking-tight">Authentication</h2>

      {hasToken && (
        <div className="mb-6 glass rounded-xl p-6">
          <div className="flex items-center gap-2 mb-3">
            <span className="h-2 w-2 rounded-full bg-status-working shadow-[0_0_6px_1px] shadow-status-working/50" />
            <span className="text-sm text-text-muted">Token configured</span>
          </div>
          <button
            type="button"
            onClick={handleClear}
            className="text-xs text-text-muted underline hover:text-text"
          >
            Clear token
          </button>
        </div>
      )}

      <div className="glass rounded-xl p-6">
        <h3 className="mb-2 text-sm font-medium">
          {hasToken ? 'Update API Token' : 'Enter API Token'}
        </h3>
        <p className="mb-3 text-sm text-text-muted">
          Paste the token from your server config or run{' '}
          <code className="text-accent">praefectus auth token</code>
        </p>
        <input
          type="password"
          value={tokenInput}
          onChange={(e) => setTokenInput(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSave()}
          placeholder="Enter API token..."
          className="w-full rounded-lg bg-bg border border-border-subtle p-3 text-sm font-mono mb-3 focus:outline-none focus:border-accent"
        />
        <button
          type="button"
          onClick={handleSave}
          disabled={!tokenInput.trim()}
          className="rounded-lg bg-accent text-bg px-4 py-2 text-sm font-medium disabled:opacity-40"
        >
          {saved ? 'Saved!' : 'Save Token'}
        </button>
      </div>

      <div className="mt-6 glass rounded-xl p-6">
        <h3 className="mb-2 text-sm font-medium">Agent Re-authentication</h3>
        <p className="mb-3 text-sm text-text-muted">
          If your Claude or Codex authentication has expired, use the CLI:
        </p>
        <div className="space-y-2">
          <div className="rounded-lg bg-bg font-mono text-xs p-3 border border-border-subtle text-accent">
            praefectus auth claude
          </div>
          <div className="rounded-lg bg-bg font-mono text-xs p-3 border border-border-subtle text-accent">
            praefectus auth codex
          </div>
        </div>
      </div>
    </div>
  );
}
