'use client';

import { useRouter } from 'next/navigation';
import { useState } from 'react';
import type { Session } from '@/lib/types';

const STATUS_DOT_COLOR: Record<string, string> = {
  running: 'bg-status-working',
  needs_input: 'bg-status-needs-input',
  auth_required: 'bg-orange-500',
  queued: 'bg-status-queued',
  completed: 'bg-status-completed',
  failed: 'bg-status-error',
  cancelled: 'bg-status-error',
  interrupted: 'bg-status-error',
};

function formatDuration(startedAt: number | null, endedAt: number | null): string {
  if (!startedAt) return '';
  const end = endedAt ?? Date.now();
  const secs = Math.max(0, Math.floor((end - startedAt) / 1000));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  const remainMins = mins % 60;
  return `${hrs}h ${remainMins}m`;
}

interface SessionTreeProps {
  sessions: Session[];
  childrenMap: Record<string, Session[]>;
  depth: number;
}

export function SessionTree({ sessions, childrenMap, depth }: SessionTreeProps) {
  return (
    <div>
      {sessions.map((session) => (
        <SessionRow key={session.id} session={session} childrenMap={childrenMap} depth={depth} />
      ))}
    </div>
  );
}

interface SessionRowProps {
  session: Session;
  childrenMap: Record<string, Session[]>;
  depth: number;
}

function SessionRow({ session, childrenMap, depth }: SessionRowProps) {
  const router = useRouter();
  const children = childrenMap[session.id];
  const hasChildren = children && children.length > 0;
  const [expanded, setExpanded] = useState(true);

  const dotColor = STATUS_DOT_COLOR[session.status] ?? 'bg-status-completed';
  const duration = formatDuration(session.startedAt, session.endedAt);

  return (
    <div>
      <div
        className="group flex w-full items-center gap-2 py-2 transition-colors hover:bg-white/[0.04]"
        style={{ paddingLeft: `${16 + depth * 20}px`, paddingRight: '16px' }}
      >
        {/* Expand/collapse toggle */}
        {hasChildren ? (
          <button
            type="button"
            className="flex h-4 w-4 shrink-0 items-center justify-center"
            onClick={() => setExpanded((prev) => !prev)}
            aria-label={expanded ? 'Collapse' : 'Expand'}
          >
            <svg
              className={`h-3 w-3 text-white/30 transition-transform ${expanded ? 'rotate-90' : ''}`}
              viewBox="0 0 16 16"
              fill="currentColor"
            >
              <path d="M6 4l4 4-4 4z" />
            </svg>
          </button>
        ) : (
          <span className="h-4 w-4 shrink-0" />
        )}

        {/* Clickable session row */}
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
          onClick={() => router.push(`/dashboard/sessions/${session.id}`)}
        >
          {/* Status dot */}
          <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${dotColor}`} />

          {/* Prompt */}
          <span className="min-w-0 flex-1 truncate text-[13px] text-white/70 group-hover:text-white/90">
            {session.prompt}
          </span>

          {/* Duration */}
          {duration && (
            <span className="shrink-0 text-[11px] tabular-nums text-white/30">{duration}</span>
          )}

          {/* Child count badge */}
          {hasChildren && (
            <span className="shrink-0 rounded-full bg-white/[0.06] px-1.5 py-0.5 text-[10px] tabular-nums text-white/40">
              {children.length}
            </span>
          )}
        </button>
      </div>

      {/* Children */}
      {hasChildren && expanded && (
        <SessionTree sessions={children} childrenMap={childrenMap} depth={depth + 1} />
      )}
    </div>
  );
}
