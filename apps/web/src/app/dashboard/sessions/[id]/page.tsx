'use client';

import Link from 'next/link';
import { use, useEffect } from 'react';
import { StatusBadge } from '@/components/dashboard/status-badge';
import { TerminalPanel } from '@/components/dashboard/terminal-panel';
import { Tooltip } from '@/components/ui/tooltip';
import { useCancelSession, useRestartSession } from '@/lib/api';
import { useDashboardWs } from '@/lib/hooks/use-dashboard-ws';
import { useSessionStore } from '@/lib/stores/session-store';
import type { Session } from '@/lib/types';

function formatDuration(startedAt: number | null, endedAt?: number | null): string {
  if (!startedAt) return '--';
  const end = endedAt ?? Date.now();
  const seconds = Math.floor((end - startedAt) / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

/**
 * Fetch session from REST API and push into Zustand store.
 * Covers the case where the WS state_sync hasn't arrived yet
 * or the session completed before the dashboard WS connected.
 */
function useSessionFallback(id: string) {
  const { setSession, sessions } = useSessionStore();
  const exists = !!sessions[id];

  useEffect(() => {
    if (exists) return;

    let cancelled = false;
    fetch(`/api/sessions/${id}`)
      .then((res) => {
        if (!res.ok) return null;
        return res.json();
      })
      .then((data: Session | null) => {
        if (data && !cancelled) {
          setSession(data);
        }
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, [id, exists, setSession]);
}

function GitMetadataBadges({ session }: { session: Session }) {
  const meta = session.gitMetadata;
  if (!meta) return null;

  return (
    <div className="glass-strong shrink-0 border-b border-border-subtle px-4 py-2 flex items-center gap-2">
      <Tooltip text={`Repository: ${meta.repoName}`}>
        <span className="inline-flex items-center rounded bg-surface-alt px-2 py-0.5 font-mono text-xs text-text-muted">
          {meta.repoName}
        </span>
      </Tooltip>
      <Tooltip text={`Branch: ${meta.branch}`}>
        <span className="inline-flex items-center rounded bg-surface-alt px-2 py-0.5 font-mono text-xs text-accent">
          {meta.branch}
        </span>
      </Tooltip>
      <Tooltip text={`Commit: ${meta.commitHash}`}>
        <span className="inline-flex items-center rounded bg-surface-alt px-2 py-0.5 font-mono text-xs text-text-muted">
          {meta.commitHash}
        </span>
      </Tooltip>
      {meta.remoteUrl && (
        <Tooltip text={`Remote: ${meta.remoteUrl}`}>
          <span className="inline-flex items-center rounded bg-surface-alt px-2 py-0.5 font-mono text-xs text-text-muted truncate max-w-[200px]">
            {meta.remoteUrl.replace(/^https?:\/\//, '').replace(/\.git$/, '')}
          </span>
        </Tooltip>
      )}
    </div>
  );
}

export default function SessionDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = use(params);
  useDashboardWs();
  useSessionFallback(id);

  const session = useSessionStore((s) => s.sessions[id]);
  const cancelMutation = useCancelSession();
  const restartMutation = useRestartSession();
  const isActive = session && ['queued', 'running', 'needs_input'].includes(session.status);
  const canRestart =
    session && ['completed', 'failed', 'cancelled', 'interrupted'].includes(session.status);

  if (!session) {
    return (
      <div className="glass rounded-xl p-8 text-center mx-auto mt-12 max-w-md">
        <p className="text-text-muted">Loading session...</p>
        <Link href="/dashboard" className="mt-4 inline-block text-accent hover:underline">
          Back to Dashboard
        </Link>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Session info header — compact */}
      <div className="shrink-0 border-b border-border-subtle bg-[rgba(255,255,255,0.03)] px-6 py-4">
        <div className="flex items-center gap-4">
          <Link
            href="/dashboard"
            className="flex h-8 w-8 items-center justify-center rounded-lg border border-border-subtle text-text-muted hover:bg-surface-hover hover:text-text transition-colors"
          >
            <svg
              width="16"
              height="16"
              viewBox="0 0 16 16"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M10 3L5 8l5 5" />
            </svg>
          </Link>
          <div className="flex flex-1 items-center gap-3 min-w-0">
            <span className="shrink-0 text-[15px] font-semibold text-text">
              {session.agentType === 'claude' ? 'Claude' : 'Codex'}
              {session.role ? ` (${session.role})` : ''}
            </span>
            <StatusBadge status={session.status} />
            <span className="mx-1 h-4 w-px bg-border-subtle" />
            <p className="min-w-0 truncate text-[13px] text-text-muted">{session.prompt}</p>
          </div>
          <div className="flex shrink-0 items-center gap-3">
            <span className="font-mono text-xs tabular-nums text-text-faint">
              {formatDuration(session.startedAt, session.endedAt)}
            </span>
            {canRestart && (
              <button
                type="button"
                onClick={() => restartMutation.mutate(session.id)}
                disabled={restartMutation.isPending}
                className="btn-press rounded-lg bg-accent/15 px-3 py-1.5 text-xs text-accent hover:bg-accent/25 disabled:opacity-50"
              >
                {restartMutation.isPending ? 'Restarting...' : 'Restart'}
              </button>
            )}
            {isActive && (
              <button
                type="button"
                onClick={() => cancelMutation.mutate(session.id)}
                disabled={cancelMutation.isPending}
                className="btn-press rounded-lg border border-status-error/20 bg-status-error/[0.06] px-3.5 py-1.5 text-xs font-medium text-status-error hover:bg-status-error/10 disabled:opacity-50"
              >
                Cancel
              </button>
            )}
          </div>
        </div>
      </div>

      {/* Git metadata badges */}
      <GitMetadataBadges session={session} />

      {/* Terminal - takes remaining space (show for any non-queued session) */}
      {session.status !== 'queued' ? (
        <div className="min-h-0 flex-1">
          <TerminalPanel sessionId={session.id} />
        </div>
      ) : (
        <div className="flex flex-1 items-center justify-center text-text-muted">
          Waiting to start...
        </div>
      )}
    </div>
  );
}
