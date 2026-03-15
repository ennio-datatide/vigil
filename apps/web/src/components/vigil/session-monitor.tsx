'use client';

import { useMemo, useState } from 'react';
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

function groupSessions(sessions: Session[]) {
  const pipelineGroups: Record<string, Session[]> = {};
  const standalone: Session[] = [];

  for (const session of sessions) {
    if (session.pipelineId) {
      if (!pipelineGroups[session.pipelineId]) {
        pipelineGroups[session.pipelineId] = [];
      }
      pipelineGroups[session.pipelineId].push(session);
    } else {
      standalone.push(session);
    }
  }

  // Sort pipeline sessions by step index
  for (const group of Object.values(pipelineGroups)) {
    group.sort((a, b) => (a.pipelineStepIndex ?? 0) - (b.pipelineStepIndex ?? 0));
  }

  return { pipelineGroups, standalone };
}

function getPipelineProgress(sessions: Session[]): { current: number; total: number } {
  const total = sessions.length;
  let highestCompleted = -1;
  for (const s of sessions) {
    const idx = s.pipelineStepIndex ?? 0;
    if (s.status === 'completed') {
      highestCompleted = Math.max(highestCompleted, idx);
    }
  }
  const current = Math.min(highestCompleted + 2, total);
  return { current, total };
}

interface PipelineGroupProps {
  pipelineId: string;
  sessions: Session[];
  childrenMap: Record<string, Session[]>;
}

function PipelineGroup({ pipelineId: _pipelineId, sessions, childrenMap }: PipelineGroupProps) {
  const [expanded, setExpanded] = useState(true);
  const { current, total } = getPipelineProgress(sessions);

  return (
    <div>
      {/* Pipeline group header */}
      <button
        type="button"
        className="group flex w-full items-center gap-2 px-4 py-2.5 transition-colors hover:bg-white/[0.04]"
        onClick={() => setExpanded((prev) => !prev)}
      >
        {/* Chevron */}
        <svg
          className={`h-3 w-3 shrink-0 text-white/30 transition-transform ${expanded ? 'rotate-90' : ''}`}
          viewBox="0 0 16 16"
          fill="currentColor"
        >
          <path d="M6 4l4 4-4 4z" />
        </svg>

        {/* Pipeline icon */}
        <svg
          className="h-3.5 w-3.5 shrink-0 text-violet-400/70"
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M2 4h4M2 8h4M2 12h4M8 4h6M8 8h6M8 12h6" />
        </svg>

        {/* Pipeline name */}
        <span className="min-w-0 flex-1 truncate text-left text-[13px] font-medium text-white/60 group-hover:text-white/80">
          Pipeline
        </span>

        {/* Step progress */}
        <span className="shrink-0 rounded-full bg-violet-500/10 px-2 py-0.5 text-[11px] tabular-nums text-violet-400/80">
          step {current}/{total}
        </span>
      </button>

      {/* Pipeline sessions */}
      {expanded && <SessionTree sessions={sessions} childrenMap={childrenMap} depth={1} />}
    </div>
  );
}

export function SessionMonitor({ onClose }: { onClose?: () => void }) {
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

  const { pipelineGroups, standalone } = useMemo(() => groupSessions(rootSessions), [rootSessions]);

  const pipelineEntries = useMemo(() => Object.entries(pipelineGroups), [pipelineGroups]);

  const hasAnySessions = pipelineEntries.length > 0 || standalone.length > 0;

  return (
    <div className="flex h-full flex-col">
      {/* KPI Header */}
      <div className="flex shrink-0 items-center justify-between border-b border-white/[0.06] px-4 py-2.5">
        <p className="text-[13px] text-white/50">
          <span className="text-status-working font-medium">{counts.active}</span> active
          <span className="mx-1.5 text-white/20">·</span>
          <span className="text-status-needs-input font-medium">{counts.blocked}</span> blocked
          <span className="mx-1.5 text-white/20">·</span>
          <span className="text-status-completed font-medium">{counts.completed}</span> completed
        </p>
        {onClose && (
          <button
            type="button"
            onClick={onClose}
            className="rounded p-0.5 text-white/30 transition-colors hover:text-white/60"
            aria-label="Close session panel"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path
                d="M10.5 3.5L3.5 10.5M3.5 3.5l7 7"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
              />
            </svg>
          </button>
        )}
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto">
        {!hasAnySessions ? (
          <p className="px-4 py-8 text-center text-[13px] text-white/30">No sessions</p>
        ) : (
          <>
            {/* Pipeline groups */}
            {pipelineEntries.map(([pipelineId, pipelineSessions]) => (
              <PipelineGroup
                key={pipelineId}
                pipelineId={pipelineId}
                sessions={pipelineSessions}
                childrenMap={childrenMap}
              />
            ))}

            {/* Standalone sessions */}
            {standalone.length > 0 && (
              <SessionTree sessions={standalone} childrenMap={childrenMap} depth={0} />
            )}
          </>
        )}
      </div>
    </div>
  );
}
