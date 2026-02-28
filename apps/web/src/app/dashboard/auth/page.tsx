'use client';

import { useQuery } from '@tanstack/react-query';

export default function AuthPage() {
  const { data, isLoading } = useQuery({
    queryKey: ['health'],
    queryFn: async () => {
      const res = await fetch('/health');
      return res.json();
    },
    refetchInterval: 10000,
  });

  return (
    <div className="p-4">
      <h2 className="mb-4 text-xl font-semibold tracking-tight">Authentication</h2>

      <div className="mb-6 glass rounded-xl p-6">
        <h3 className="mb-2 text-sm font-medium">Server Status</h3>
        <div className="flex items-center gap-2">
          <span
            className={`h-2 w-2 rounded-full ${
              isLoading
                ? 'bg-status-needs-input'
                : data?.status === 'ok'
                  ? 'bg-status-working shadow-[0_0_6px_1px] shadow-status-working/50'
                  : 'bg-status-error'
            }`}
          />
          <span className="text-sm text-text-muted">
            {isLoading ? 'Checking...' : data?.status === 'ok' ? 'Connected' : 'Disconnected'}
          </span>
        </div>
      </div>

      <div className="glass rounded-xl p-6">
        <h3 className="mb-2 text-sm font-medium">Re-authenticate</h3>
        <p className="mb-3 text-sm text-text-muted">
          If your Claude or Codex authentication has expired, use the CLI to re-authenticate:
        </p>
        <div className="space-y-2">
          <div className="rounded-lg bg-bg font-mono text-xs p-3 border border-border-subtle text-accent">
            praefectus auth claude
          </div>
          <div className="rounded-lg bg-bg font-mono text-xs p-3 border border-border-subtle text-accent">
            praefectus auth codex
          </div>
        </div>
        <p className="mt-3 text-xs text-text-muted">
          This opens a tmux session where you can complete the authentication flow.
        </p>
      </div>
    </div>
  );
}
