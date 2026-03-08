'use client';

import { useState } from 'react';
import type { EmbeddedCard } from '@/lib/types';

interface ActaCardProps {
  card: EmbeddedCard;
}

export function ActaCard({ card }: ActaCardProps) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="rounded-xl border border-border-subtle bg-surface px-4 py-3">
      <button
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        className="flex w-full items-center justify-between text-left"
      >
        <span className="text-xs font-semibold text-accent">Project Briefing (Acta)</span>
        <svg
          className={`h-4 w-4 text-text-muted transition-transform ${expanded ? 'rotate-180' : ''}`}
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          strokeWidth={2}
        >
          <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
        </svg>
      </button>
      {expanded && card.acta && (
        <p className="mt-3 whitespace-pre-wrap text-sm leading-relaxed text-text-secondary">
          {card.acta}
        </p>
      )}
    </div>
  );
}
