'use client';

import Link from 'next/link';
import { useSessionStore } from '@/lib/stores/session-store';
import type { Session } from '@/lib/types';
import { SessionCard } from './session-card';
import { SessionCardSkeleton } from './session-card-skeleton';

const STATUS_PRIORITY: Record<string, number> = {
  needs_input: 0,
  auth_required: 1,
  running: 2,
  queued: 3,
  failed: 4,
  completed: 5,
  cancelled: 6,
  interrupted: 7,
};

function sortSessions(sessions: Session[]): Session[] {
  return [...sessions].sort((a, b) => {
    const pa = STATUS_PRIORITY[a.status] ?? 99;
    const pb = STATUS_PRIORITY[b.status] ?? 99;
    return pa - pb;
  });
}

export function SessionGrid({ search = '' }: { search?: string }) {
  const sessions = useSessionStore((s) => s.sessions);
  const initialized = useSessionStore((s) => s.initialized);
  const all = sortSessions(Object.values(sessions));
  const sorted = search
    ? all.filter(
        (s) =>
          s.prompt?.toLowerCase().includes(search.toLowerCase()) ||
          s.projectPath?.toLowerCase().includes(search.toLowerCase()),
      )
    : all;

  if (!initialized) {
    return (
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <SessionCardSkeleton key={i} />
        ))}
      </div>
    );
  }

  if (sorted.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-center">
        <svg
          className="mb-4 h-16 w-16 text-text-faint"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
        >
          <rect x="2" y="3" width="20" height="18" rx="2" />
          <path d="M7 8l3 3-3 3" />
          <line x1="13" y1="14" x2="17" y2="14" />
        </svg>
        <h3 className="mb-2 text-base font-semibold text-text">No sessions yet</h3>
        <p className="text-sm text-text-muted">Start your first agent session with the + button</p>
      </div>
    );
  }

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <h2 className="text-sm font-semibold text-text-muted">Active Sessions</h2>
        <Link
          href="/dashboard/history"
          className="text-xs text-text-faint hover:text-text-muted transition-colors"
        >
          View all
        </Link>
      </div>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
        {sorted.map((session) => (
          <SessionCard key={session.id} session={session} />
        ))}
      </div>
    </div>
  );
}
