'use client';

import { useMemo } from 'react';
import { useSessionStore } from '@/lib/stores/session-store';
import type { Session } from '@/lib/types';
import { SessionTree } from './session-tree';

const STATUS_PRIORITY: Record<string, number> = {
  needs_input: 0,
  auth_required: 1,
  running: 2,
  queued: 3,
  completed: 4,
  failed: 5,
  cancelled: 6,
  interrupted: 7,
};

export function SessionMonitor() {
  const sessions = useSessionStore((s) => s.sessions);

  const allSessions = useMemo(() => Object.values(sessions), [sessions]);

  const rootSessions = useMemo(() => {
    return allSessions
      .filter((s) => !s.parentId)
      .sort((a, b) => (STATUS_PRIORITY[a.status] ?? 99) - (STATUS_PRIORITY[b.status] ?? 99));
  }, [allSessions]);

  const counts = useMemo(() => {
    let active = 0;
    let blocked = 0;
    let completed = 0;
    for (const s of allSessions) {
      if (s.status === 'running' || s.status === 'queued') active++;
      else if (s.status === 'needs_input' || s.status === 'auth_required') blocked++;
      else completed++;
    }
    return { active, blocked, completed };
  }, [allSessions]);

  const childrenMap = useMemo(() => {
    const map: Record<string, Session[]> = {};
    for (const s of allSessions) {
      if (s.parentId) {
        if (!map[s.parentId]) map[s.parentId] = [];
        map[s.parentId].push(s);
      }
    }
    // Sort children within each group by the same priority
    for (const children of Object.values(map)) {
      children.sort(
        (a, b) => (STATUS_PRIORITY[a.status] ?? 99) - (STATUS_PRIORITY[b.status] ?? 99),
      );
    }
    return map;
  }, [allSessions]);

  return (
    <div className="flex h-full flex-col">
      {/* KPI Header */}
      <div className="shrink-0 border-b border-white/[0.06] px-4 py-2.5">
        <p className="text-[13px] text-white/50">
          <span className="text-status-working font-medium">{counts.active}</span> active
          <span className="mx-1.5 text-white/20">·</span>
          <span className="text-status-needs-input font-medium">{counts.blocked}</span> blocked
          <span className="mx-1.5 text-white/20">·</span>
          <span className="text-status-completed font-medium">{counts.completed}</span> completed
        </p>
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto">
        {rootSessions.length === 0 ? (
          <p className="px-4 py-8 text-center text-[13px] text-white/30">No sessions</p>
        ) : (
          <SessionTree sessions={rootSessions} childrenMap={childrenMap} depth={0} />
        )}
      </div>
    </div>
  );
}
