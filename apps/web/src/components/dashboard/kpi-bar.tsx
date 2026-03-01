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
      subtitle: 'agents',
      color: 'text-status-working',
      tip: 'Running, queued, or waiting sessions',
    },
    {
      label: 'Blocked',
      value: blocked,
      subtitle: 'needs input',
      color: 'text-status-needs-input',
      tip: 'Waiting for user input or auth',
    },
    {
      label: 'Completed',
      value: completed,
      subtitle: 'today',
      color: 'text-text',
      tip: 'Successfully finished sessions',
    },
    {
      label: 'Failed',
      value: failed,
      subtitle: 'errors',
      color: 'text-status-error',
      tip: 'Sessions that exited with errors',
    },
  ];

  return (
    <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
      {kpis.map((k) => (
        <Tooltip key={k.label} text={k.tip}>
          <div className="rounded-xl border border-border-subtle bg-[rgba(255,255,255,0.025)] p-5 transition-colors hover:bg-surface-hover/30">
            <p className="text-[11px] font-medium uppercase tracking-[0.06em] text-text-dim">
              {k.label}
            </p>
            <div className="mt-2 flex items-baseline gap-2">
              <span
                className={`text-[32px] font-extrabold -tracking-[0.04em] tabular-nums leading-none ${k.color}`}
              >
                {k.value}
              </span>
              <span className="text-xs text-text-faint">{k.subtitle}</span>
            </div>
          </div>
        </Tooltip>
      ))}
    </div>
  );
}
