'use client';

import type { EmbeddedCard } from '@/lib/types';

interface StatusCardProps {
  card: EmbeddedCard;
}

export function StatusCard({ card }: StatusCardProps) {
  return (
    <div className="rounded-xl border border-border-subtle bg-surface px-4 py-3">
      <div className="flex items-center gap-2">
        <span className="h-2 w-2 rounded-full bg-status-working" />
        <p className="text-sm text-text">{card.summary}</p>
      </div>
    </div>
  );
}
