'use client';

import { useState } from 'react';
import { KpiBar } from '@/components/dashboard/kpi-bar';
import { NotificationBell } from '@/components/dashboard/notification-bell';
import { RecentActivity } from '@/components/dashboard/recent-activity';
import { SessionGrid } from '@/components/dashboard/session-grid';
import { SessionList } from '@/components/dashboard/session-list';
import { useDashboardWs } from '@/lib/hooks/use-dashboard-ws';
import { useSessionStore } from '@/lib/stores/session-store';

export default function DashboardPage() {
  useDashboardWs();
  const [search, setSearch] = useState('');

  const sessionsMap = useSessionStore((s) => s.sessions);
  const sessions = Object.values(sessionsMap);
  const activeCount = sessions.filter((s) =>
    ['queued', 'running', 'needs_input'].includes(s.status),
  ).length;
  const projectCount = new Set(
    sessions
      .filter((s) => ['queued', 'running', 'needs_input'].includes(s.status))
      .map((s) => s.projectPath),
  ).size;

  return (
    <div className="space-y-7 p-6 md:px-10 md:py-8">
      <div className="flex items-end justify-between">
        <div>
          <h1 className="text-[28px] font-extrabold -tracking-[0.04em] text-text">Dashboard</h1>
          <p className="mt-1 text-[13px] text-text-faint">
            {activeCount} agent{activeCount !== 1 ? 's' : ''} running across {projectCount} project
            {projectCount !== 1 ? 's' : ''}
          </p>
        </div>
        <div className="hidden items-center gap-3 md:flex">
          <div className="relative">
            <svg
              className="absolute left-3 top-1/2 -translate-y-1/2 text-text-faint"
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search sessions..."
              className="w-52 rounded-lg border border-border-subtle bg-[rgba(255,255,255,0.03)] py-2 pl-9 pr-3 text-xs text-text placeholder:text-text-faint focus-accent transition-colors"
            />
          </div>
          <NotificationBell />
        </div>
      </div>
      <KpiBar />
      <div className="md:hidden">
        <SessionList />
      </div>
      <div className="hidden md:block">
        <SessionGrid search={search} />
      </div>
      <div className="hidden md:block">
        <RecentActivity />
      </div>
    </div>
  );
}
