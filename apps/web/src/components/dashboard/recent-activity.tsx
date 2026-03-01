'use client';

import Link from 'next/link';
import { useSessionStore } from '@/lib/stores/session-store';
import type { Session } from '@/lib/types';

function formatTimeAgo(ts: number | null): string {
  if (!ts) return '';
  const diff = Date.now() - ts;
  const mins = Math.floor(diff / 60_000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

function projectName(path: string): string {
  const parts = path.split('/');
  return parts[parts.length - 1] || path;
}

const STATUS_LABEL: Record<string, string> = {
  completed: 'Completed',
  failed: 'Failed',
  cancelled: 'Cancelled',
  interrupted: 'Interrupted',
};

const STATUS_COLOR: Record<string, string> = {
  completed: 'text-text-muted',
  failed: 'text-status-error',
  cancelled: 'text-text-faint',
  interrupted: 'text-status-needs-input',
};

const DOT_COLOR: Record<string, string> = {
  completed: 'bg-text-muted',
  failed: 'bg-status-error',
  cancelled: 'bg-text-faint',
  interrupted: 'bg-status-needs-input',
};

export function RecentActivity() {
  const sessions = useSessionStore((s) => s.sessions);
  const finished = Object.values(sessions)
    .filter((s: Session) => ['completed', 'failed', 'cancelled', 'interrupted'].includes(s.status))
    .sort((a, b) => (b.endedAt ?? 0) - (a.endedAt ?? 0))
    .slice(0, 5);

  if (finished.length === 0) return null;

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h2 className="text-sm font-semibold text-text-muted">Recent Activity</h2>
          <div className="h-px flex-1 bg-border-subtle" />
        </div>
        <Link
          href="/dashboard/history"
          className="text-xs text-text-faint hover:text-text-muted transition-colors"
        >
          History
        </Link>
      </div>
      <div className="space-y-1">
        {finished.map((session) => (
          <Link
            key={session.id}
            href={`/dashboard/sessions/${session.id}`}
            className="flex items-center gap-3 rounded-lg px-3 py-2.5 transition-colors hover:bg-surface-hover"
          >
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${DOT_COLOR[session.status] ?? 'bg-text-faint'}`}
            />
            <span
              className={`w-24 shrink-0 text-xs font-medium ${STATUS_COLOR[session.status] ?? 'text-text-faint'}`}
            >
              {STATUS_LABEL[session.status] ?? session.status}
            </span>
            <span className="min-w-0 flex-1 truncate text-sm text-text-muted">
              {session.prompt}
            </span>
            <span className="shrink-0 text-xs text-text-faint">
              {projectName(session.projectPath)}
            </span>
            <span className="shrink-0 text-xs text-text-faint tabular-nums">
              {formatTimeAgo(session.endedAt)}
            </span>
          </Link>
        ))}
      </div>
    </div>
  );
}
