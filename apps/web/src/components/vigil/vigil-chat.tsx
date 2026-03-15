'use client';

import { useRouter } from 'next/navigation';
import { useEffect, useRef, useState } from 'react';
import { useProjectsQuery, useVigilChat, useVigilHistoryQuery } from '@/lib/api';
import { useVigilStore } from '@/lib/stores/vigil-store';
import { ChatMessage } from './chat-message';

export function VigilChat({ onSessionClick }: { onSessionClick?: () => void }) {
  const router = useRouter();
  const [input, setInput] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const {
    messages,
    isProcessing,
    projectPath,
    activities,
    setMessages,
    addMessage,
    setProcessing,
    setProjectPath,
    clearActivities,
  } = useVigilStore();
  const { data } = useVigilHistoryQuery();
  const chatMutation = useVigilChat();
  const { data: projects } = useProjectsQuery();

  useEffect(() => {
    if (data?.messages) {
      setMessages(data.messages);
    }
  }, [data, setMessages]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll on message/processing/activity changes
  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages, isProcessing, activities]);

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
    clearActivities();
    setProcessing(true);

    try {
      const result = await chatMutation.mutateAsync({ message: trimmed, projectPath });
      const vigilMessage = {
        id: Date.now() + 1,
        role: 'vigil' as const,
        content: result.response,
        sessionId: result.sessionId ?? undefined,
        embeddedCards: null,
        createdAt: Date.now(),
      };
      addMessage(vigilMessage);
    } catch (err) {
      const isBusy = err instanceof Error && err.message.includes('503');
      const errorMessage = {
        id: Date.now() + 1,
        role: 'vigil' as const,
        content: isBusy
          ? "I'm currently processing another request. Please wait a moment."
          : `Failed to reach Vigil. ${err instanceof Error ? err.message : 'Unknown error.'}`,
        embeddedCards: null,
        createdAt: Date.now(),
      };
      addMessage(errorMessage);
    } finally {
      setProcessing(false);
      clearActivities();
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
              <div className="space-y-1.5 py-2">
                {activities.map((activity) => (
                  <div key={activity.id} className="flex items-center gap-2">
                    <span className="h-1.5 w-1.5 rounded-full bg-accent/50" />
                    {activity.sessionId ? (
                      <button
                        type="button"
                        className="text-xs text-text-muted transition-colors hover:text-accent"
                        onClick={() => router.push(`/dashboard/sessions/${activity.sessionId}`)}
                      >
                        {activity.text}
                      </button>
                    ) : (
                      <span className="text-xs text-text-muted">{activity.text}</span>
                    )}
                  </div>
                ))}
                <div className="flex items-center gap-2">
                  <span className="h-2 w-2 animate-pulse rounded-full bg-accent" />
                  <span className="text-xs text-text-muted">
                    {activities.length > 0 ? 'Vigil is working...' : 'Vigil is thinking...'}
                  </span>
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      <div className="flex-shrink-0 border-t border-border-subtle px-5 py-3">
        {projects && projects.length > 0 && (
          <div className="mb-2">
            <select
              value={projectPath ?? ''}
              onChange={(e) => setProjectPath(e.target.value || null)}
              className="w-full rounded-lg bg-surface-alt px-3 py-1.5 text-xs text-text-muted focus-accent"
            >
              <option value="">No project context</option>
              {projects.map((p) => (
                <option key={p.path} value={p.path}>
                  {p.name || p.path.split('/').pop()}
                </option>
              ))}
            </select>
          </div>
        )}
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
