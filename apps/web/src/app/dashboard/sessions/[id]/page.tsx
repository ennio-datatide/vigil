'use client';

import { use, useEffect } from 'react';
import Link from 'next/link';
import { useSessionStore } from '@/lib/stores/session-store';
import { useDashboardWs } from '@/lib/hooks/use-dashboard-ws';
import { useCancelSession, useRestartSession } from '@/lib/api';
import { StatusBadge } from '@/components/dashboard/status-badge';
import { TerminalPanel } from '@/components/dashboard/terminal-panel';
import { Tooltip } from '@/components/ui/tooltip';
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

    return () => { cancelled = true; };
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
  const canRestart = session && ['completed', 'failed', 'cancelled', 'interrupted'].includes(session.status);

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
      {/* Session info header */}
      <div className="glass-strong shrink-0 border-b border-border-subtle p-4 md:p-6">
        <div className="mb-2 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Link href="/dashboard" className="rounded-lg p-1 text-text-muted hover:bg-surface-hover hover:text-text transition-colors">
              &larr;
            </Link>
            <h2 className="text-xl font-semibold tracking-tight">
              {session.agentType === 'claude' ? 'Claude' : 'Codex'}
              {session.role ? ` (${session.role})` : ''}
            </h2>
            <StatusBadge status={session.status} />
          </div>
          <div className="flex items-center gap-2">
            <span className="text-xs text-text-muted">
              {formatDuration(session.startedAt, session.endedAt)}
            </span>
            {canRestart && (
              <button
                onClick={() => restartMutation.mutate(session.id)}
                disabled={restartMutation.isPending}
                className="btn-press min-h-[44px] rounded-md bg-accent/15 px-3 py-2 text-xs text-accent hover:bg-accent/25 disabled:opacity-50"
              >
                {restartMutation.isPending ? 'Restarting...' : 'Restart'}
              </button>
            )}
            {isActive && (
              <button
                onClick={() => cancelMutation.mutate(session.id)}
                disabled={cancelMutation.isPending}
                className="btn-press min-h-[44px] rounded-md px-3 py-2 text-xs text-status-error hover:bg-status-error/10 disabled:opacity-50"
              >
                Cancel
              </button>
            )}
          </div>
        </div>
        <p className="text-sm text-text-muted leading-relaxed">{session.prompt}</p>
        <div className="mt-1 text-xs text-text-faint">
          Project: {session.projectPath}
          {session.skillsUsed && ` | Skills: ${session.skillsUsed}`}
          {session.parentId && ` | Parent: ${session.parentId}`}
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
