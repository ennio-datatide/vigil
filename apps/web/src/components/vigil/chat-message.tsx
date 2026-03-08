'use client';

import type { VigilMessage } from '@/lib/types';
import { ActaCard } from './cards/acta-card';
import { BlockerCard } from './cards/blocker-card';
import { CompletionCard } from './cards/completion-card';
import { StatusCard } from './cards/status-card';

interface ChatMessageProps {
  message: VigilMessage;
}

export function ChatMessage({ message }: ChatMessageProps) {
  const isUser = message.role === 'user';

  return (
    <div className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}>
      <div className="flex max-w-[80%] flex-col gap-2">
        <div
          className={`rounded-xl px-4 py-3 text-sm leading-relaxed ${
            isUser
              ? 'bg-accent/15 text-text'
              : 'border border-border-subtle bg-surface-alt text-text'
          }`}
        >
          <p className="whitespace-pre-wrap">{message.content}</p>
        </div>

        {message.embeddedCards?.map((card, i) => {
          const key = `${message.id}-card-${i}`;
          switch (card.type) {
            case 'blocker':
              return <BlockerCard key={key} card={card} />;
            case 'status':
              return <StatusCard key={key} card={card} />;
            case 'completion':
              return <CompletionCard key={key} card={card} />;
            case 'acta':
              return <ActaCard key={key} card={card} />;
            default:
              return null;
          }
        })}
      </div>
    </div>
  );
}
