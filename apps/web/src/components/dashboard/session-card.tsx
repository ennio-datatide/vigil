'use client';

import { motion } from 'framer-motion';
import Link from 'next/link';
import { useState } from 'react';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { useCancelSession, useRemoveSession } from '@/lib/api';
import { useToast } from '@/lib/stores/toast-store';
import type { Session } from '@/lib/types';
import { StatusBadge } from './status-badge';

function formatDuration(startedAt: number | null, endedAt?: number | null): string {
  if (!startedAt) return '--';
  const end = endedAt ?? Date.now();
  const seconds = Math.floor((end - startedAt) / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

const GLOW_MAP: Record<string, string> = {
  running: 'glow-running-shimmer',
  needs_input: 'glow-needs-input',
  auth_required: 'glow-auth',
  queued: 'glow-queued',
  failed: 'glow-error',
  completed: 'glow-completed',
  cancelled: 'glow-completed',
  interrupted: 'glow-needs-input',
};

export function SessionCard({ session }: { session: Session }) {
  const [confirmCancel, setConfirmCancel] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const cancelMutation = useCancelSession();
  const removeMutation = useRemoveSession();
  const toast = useToast();

  const isActive = ['queued', 'running', 'needs_input'].includes(session.status);
  const isDone = ['completed', 'failed', 'cancelled', 'interrupted'].includes(session.status);
  const glowClass = GLOW_MAP[session.status] ?? '';

  return (
    <>
      <motion.div
        layout
        initial={{ opacity: 0, y: 16 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.95 }}
        whileHover={{ y: -2 }}
        transition={{ type: 'spring', stiffness: 400, damping: 30 }}
        className={`group glass relative rounded-xl border-l-2 border-transparent ${glowClass} transition-shadow`}
      >
        <div className="absolute right-2 top-2 z-10 flex gap-1 opacity-0 transition-opacity group-hover:opacity-100">
          {isActive && (
            <button
              type="button"
              onClick={() => setConfirmCancel(true)}
              className="btn-press rounded-md bg-status-error/10 px-2 py-1 text-xs font-medium text-status-error hover:bg-status-error/20"
            >
              Cancel
            </button>
          )}
          {isDone && (
            <button
              type="button"
              onClick={() => setConfirmRemove(true)}
              className="btn-press flex h-5 w-5 items-center justify-center rounded-full bg-status-error/80 text-[10px] text-white hover:bg-status-error"
              aria-label="Remove session"
            >
              &times;
            </button>
          )}
        </div>

        <Link href={`/dashboard/sessions/${session.id}`} className="block p-4">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-text-muted">
              {session.agentType === 'claude' ? 'Claude' : 'Codex'}
              {session.role ? ` / ${session.role}` : ''}
            </span>
            <StatusBadge status={session.status} />
          </div>

          <p className="mt-2 line-clamp-2 text-sm leading-relaxed text-text">{session.prompt}</p>

          <div className="mt-3 flex items-center justify-between text-xs text-text-faint">
            <span className="truncate font-mono">{session.projectPath.split('/').pop()}</span>
            <span className="shrink-0 tabular-nums">
              {formatDuration(session.startedAt, session.endedAt)}
            </span>
          </div>
        </Link>
      </motion.div>

      <ConfirmDialog
        open={confirmCancel}
        title="Cancel Session"
        message="This will stop the agent. The session record will be kept."
        confirmLabel="Cancel Session"
        variant="danger"
        onConfirm={() => {
          cancelMutation.mutate(session.id, {
            onSuccess: () => toast.success('Session cancelled'),
            onError: () => toast.error('Failed to cancel session'),
          });
          setConfirmCancel(false);
        }}
        onCancel={() => setConfirmCancel(false)}
      />
      <ConfirmDialog
        open={confirmRemove}
        title="Remove Session"
        message="This will permanently delete this session and its terminal history."
        confirmLabel="Remove"
        variant="danger"
        onConfirm={() => {
          removeMutation.mutate(session.id, {
            onSuccess: () => toast.success('Session removed'),
            onError: () => toast.error('Failed to remove session'),
          });
          setConfirmRemove(false);
        }}
        onCancel={() => setConfirmRemove(false)}
      />
    </>
  );
}
