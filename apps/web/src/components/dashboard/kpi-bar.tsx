'use client';

import { Tooltip } from '@/components/ui/tooltip';
import { useSessionStore } from '@/lib/stores/session-store';

export function KpiBar() {
  const sessionsMap = useSessionStore((s) => s.sessions);
  const sessions = Object.values(sessionsMap);
  const active = sessions.filter((s) =>
    ['queued', 'running', 'needs_input'].includes(s.status),
  ).length;
  const blocked = sessions.filter((s) =>
    ['needs_input', 'auth_required'].includes(s.status),
  ).length;
  const completed = sessions.filter((s) => s.status === 'completed').length;
  const failed = sessions.filter((s) => s.status === 'failed').length;

  const kpis = [
    {
      label: 'Active',
      value: active,
      color: 'text-status-working',
      tip: 'Running, queued, or waiting sessions',
    },
    {
      label: 'Blocked',
      value: blocked,
      color: 'text-status-needs-input',
      tip: 'Waiting for user input or auth',
    },
    {
      label: 'Completed',
      value: completed,
      color: 'text-accent',
      tip: 'Successfully finished sessions',
    },
    {
      label: 'Failed',
      value: failed,
      color: 'text-status-error',
      tip: 'Sessions that exited with errors',
    },
  ];

  return (
    <div className="grid grid-cols-2 gap-3 p-4 md:grid-cols-4 md:p-6">
      {kpis.map((k) => (
        <Tooltip key={k.label} text={k.tip}>
          <div className="glass rounded-xl p-4 transition-colors hover:bg-surface-hover/50">
            <p className={`text-2xl font-bold tabular-nums ${k.color}`}>{k.value}</p>
            <p className="mt-0.5 text-xs font-medium uppercase tracking-wider text-text-muted">
              {k.label}
            </p>
          </div>
        </Tooltip>
      ))}
    </div>
  );
}
