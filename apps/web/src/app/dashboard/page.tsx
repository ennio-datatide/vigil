'use client';

import { KpiBar } from '@/components/dashboard/kpi-bar';
import { SessionGrid } from '@/components/dashboard/session-grid';
import { SessionList } from '@/components/dashboard/session-list';
import { useDashboardWs } from '@/lib/hooks/use-dashboard-ws';
import { useSessionStore } from '@/lib/stores/session-store';

export default function DashboardPage() {
  useDashboardWs();

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
      </div>
      <KpiBar />
      <div className="md:hidden">
        <SessionList />
      </div>
      <div className="hidden md:block">
        <SessionGrid />
      </div>
    </div>
  );
}
