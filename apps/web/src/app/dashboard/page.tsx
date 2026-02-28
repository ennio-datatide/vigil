'use client';

import { KpiBar } from '@/components/dashboard/kpi-bar';
import { SessionGrid } from '@/components/dashboard/session-grid';
import { SessionList } from '@/components/dashboard/session-list';
import { useDashboardWs } from '@/lib/hooks/use-dashboard-ws';

export default function DashboardPage() {
  useDashboardWs(); // Connect WebSocket

  return (
    <div className="space-y-6 p-4 md:p-6">
      <h1 className="text-2xl font-semibold tracking-tight text-text">Dashboard</h1>
      <KpiBar />
      {/* Mobile: list view */}
      <div className="md:hidden">
        <SessionList />
      </div>
      {/* Desktop: grid view */}
      <div className="hidden md:block">
        <SessionGrid />
      </div>
    </div>
  );
}
