'use client';

import { useRouter } from 'next/navigation';
import { useState } from 'react';
import { useVigilChat } from '@/lib/api';
import type { EmbeddedCard } from '@/lib/types';

interface BlockerCardProps {
  card: EmbeddedCard;
}

export function BlockerCard({ card }: BlockerCardProps) {
  const [reply, setReply] = useState('');
  const router = useRouter();
  const chatMutation = useVigilChat();

  async function handleSubmit() {
    const trimmed = reply.trim();
    if (!trimmed) return;

    const context = card.sessionId
      ? `[Re: blocker on session ${card.sessionId}] ${trimmed}`
      : trimmed;

    await chatMutation.mutateAsync(context);
    setReply('');
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleSubmit();
    }
  }

  return (
    <div className="rounded-xl border border-border-subtle border-l-4 border-l-status-needs-input bg-surface px-4 py-3">
      <span className="text-xs font-semibold text-status-needs-input">Needs Input</span>
      {card.question && <p className="mt-1.5 text-sm text-text">{card.question}</p>}
      <div className="mt-3 flex items-center gap-2">
        <input
          type="text"
          value={reply}
          onChange={(e) => setReply(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type a reply..."
          className="flex-1 rounded-lg bg-surface-alt px-3 py-1.5 text-sm text-text placeholder:text-text-muted focus-accent"
        />
        <button
          type="button"
          onClick={handleSubmit}
          disabled={!reply.trim() || chatMutation.isPending}
          className="btn-press rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white transition-opacity disabled:opacity-40"
        >
          Reply
        </button>
        {card.sessionId && (
          <button
            type="button"
            onClick={() => router.push(`/dashboard/sessions/${card.sessionId}`)}
            className="btn-press rounded-lg border border-border px-3 py-1.5 text-xs font-medium text-text-secondary transition-colors hover:bg-surface-hover"
          >
            Open terminal
          </button>
        )}
      </div>
    </div>
  );
}
