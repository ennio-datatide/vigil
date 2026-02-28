import { Tooltip } from '@/components/ui/tooltip';

const CONFIG: Record<string, { label: string; dotClass: string; pillClass: string }> = {
  queued: {
    label: 'Queued',
    dotClass: 'bg-status-queued',
    pillClass: 'bg-status-queued/10 text-status-queued',
  },
  running: {
    label: 'Running',
    dotClass: 'bg-status-working',
    pillClass: 'bg-status-working/10 text-status-working',
  },
  needs_input: {
    label: 'Needs Input',
    dotClass: 'bg-status-needs-input',
    pillClass: 'bg-status-needs-input/10 text-status-needs-input',
  },
  auth_required: {
    label: 'Auth Required',
    dotClass: 'bg-status-auth',
    pillClass: 'bg-status-auth/10 text-status-auth',
  },
  completed: {
    label: 'Completed',
    dotClass: 'bg-status-completed',
    pillClass: 'bg-status-completed/10 text-status-completed',
  },
  failed: {
    label: 'Failed',
    dotClass: 'bg-status-error',
    pillClass: 'bg-status-error/10 text-status-error',
  },
  cancelled: {
    label: 'Cancelled',
    dotClass: 'bg-text-muted',
    pillClass: 'bg-text-muted/10 text-text-muted',
  },
  interrupted: {
    label: 'Interrupted',
    dotClass: 'bg-status-needs-input',
    pillClass: 'bg-status-needs-input/10 text-status-needs-input',
  },
};

export function StatusBadge({ status }: { status: string }) {
  const cfg = CONFIG[status] ?? { label: status, dotClass: 'bg-text-muted', pillClass: 'bg-text-muted/10 text-text-muted' };

  return (
    <Tooltip text={cfg.label}>
      <span className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${cfg.pillClass}`}>
        <span
          className={`h-1.5 w-1.5 rounded-full ${cfg.dotClass} ${
            status === 'running' ? 'animate-pulse shadow-[0_0_6px_1px] shadow-status-working/50' : ''
          }`}
        />
        {cfg.label}
      </span>
    </Tooltip>
  );
}
