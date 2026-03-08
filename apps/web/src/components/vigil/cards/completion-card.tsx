'use client';

import type { EmbeddedCard } from '@/lib/types';

interface CompletionCardProps {
  card: EmbeddedCard;
}

export function CompletionCard({ card }: CompletionCardProps) {
  return (
    <div className="rounded-xl border border-border-subtle border-l-4 border-l-status-working bg-surface px-4 py-3">
      <span className="text-xs font-semibold text-status-working">Completed</span>
      {card.summary && <p className="mt-1.5 text-sm text-text">{card.summary}</p>}
    </div>
  );
}
