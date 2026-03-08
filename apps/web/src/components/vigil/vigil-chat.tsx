'use client';

import { useEffect, useRef, useState } from 'react';
import { useVigilChat, useVigilHistoryQuery } from '@/lib/api';
import { useVigilStore } from '@/lib/stores/vigil-store';
import { ChatMessage } from './chat-message';

export function VigilChat() {
  const [input, setInput] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const { messages, isProcessing, setMessages, addMessage, setProcessing } = useVigilStore();
  const { data } = useVigilHistoryQuery();
  const chatMutation = useVigilChat();

  useEffect(() => {
    if (data?.messages) {
      setMessages(data.messages);
    }
  }, [data, setMessages]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll on message/processing changes
  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages, isProcessing]);

  function resetTextarea() {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  }

  async function handleSend() {
    const trimmed = input.trim();
    if (!trimmed || isProcessing) return;

    const userMessage = {
      id: Date.now(),
      role: 'user' as const,
      content: trimmed,
      embeddedCards: null,
      createdAt: Date.now(),
    };

    addMessage(userMessage);
    setInput('');
    resetTextarea();
    setProcessing(true);

    try {
      const result = await chatMutation.mutateAsync(trimmed);
      const vigilMessage = {
        id: Date.now() + 1,
        role: 'vigil' as const,
        content: result.response,
        embeddedCards: null,
        createdAt: Date.now(),
      };
      addMessage(vigilMessage);
    } finally {
      setProcessing(false);
    }
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function handleTextareaInput(e: React.ChangeEvent<HTMLTextAreaElement>) {
    setInput(e.target.value);
    const el = e.target;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex-shrink-0 border-b border-border-subtle px-5 py-4">
        <h2 className="text-sm font-semibold text-text">Vigil</h2>
        <p className="text-xs text-text-muted">AI Orchestrator</p>
      </div>

      <div ref={scrollRef} className="flex-1 overflow-y-auto px-5 py-4">
        {messages.length === 0 && !isProcessing ? (
          <div className="flex h-full items-center justify-center">
            <p className="max-w-64 text-center text-sm text-text-muted">
              Start a conversation with Vigil to orchestrate your coding sessions.
            </p>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            {messages.map((msg) => (
              <ChatMessage key={msg.id} message={msg} />
            ))}
            {isProcessing && (
              <div className="flex items-center gap-2 py-2">
                <span className="h-2 w-2 animate-pulse rounded-full bg-accent" />
                <span className="text-xs text-text-muted">Vigil is thinking...</span>
              </div>
            )}
          </div>
        )}
      </div>

      <div className="flex-shrink-0 border-t border-border-subtle px-5 py-3">
        <div className="flex items-end gap-2">
          <textarea
            ref={textareaRef}
            value={input}
            onChange={handleTextareaInput}
            onKeyDown={handleKeyDown}
            placeholder="Message Vigil..."
            rows={1}
            className="flex-1 resize-none rounded-xl bg-surface-alt px-4 py-2.5 text-sm text-text placeholder:text-text-muted focus-accent"
          />
          <button
            type="button"
            onClick={handleSend}
            disabled={!input.trim() || isProcessing}
            className="btn-press flex-shrink-0 rounded-xl bg-accent px-4 py-2.5 text-sm font-medium text-white transition-opacity disabled:opacity-40"
          >
            Send
          </button>
        </div>
      </div>
    </div>
  );
}
