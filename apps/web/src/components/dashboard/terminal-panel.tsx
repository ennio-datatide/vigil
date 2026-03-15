'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { useRef, useState } from 'react';
import { useRestartSession } from '@/lib/api';
import { useTerminal } from '@/lib/hooks/use-terminal';
import { useSessionStore } from '@/lib/stores/session-store';
import { useTerminalStore } from '@/lib/stores/terminal-store';
import { useToast } from '@/lib/stores/toast-store';

import '@xterm/xterm/css/xterm.css';

export function TerminalPanel({ sessionId }: { sessionId: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const { connected, ptyAlive, sendInput } = useTerminal(sessionId, containerRef);
  const [mobileInput, setMobileInput] = useState('');
  const [inputOpen, setInputOpen] = useState(false);
  const restartMutation = useRestartSession();
  const toast = useToast();
  const { panelMode, toggleFullscreen, closePanel, setPanelMode } = useTerminalStore();
  const session = useSessionStore((s) => s.sessions[sessionId]);

  const handleMobileSend = () => {
    if (!mobileInput.trim()) return;
    const text = mobileInput;
    setMobileInput('');
    sendInput(text);
    setTimeout(() => sendInput('\r'), 500);
  };

  const handleRestart = () => {
    restartMutation.mutate(sessionId, {
      onSuccess: () => toast.success('Session restarting...'),
      onError: () => toast.error('Failed to restart session'),
    });
  };

  return (
    <div className="flex h-full flex-col">
      {/* Terminal header */}
      <div className="glass-strong flex shrink-0 items-center justify-between px-4 py-2">
        <span className="flex items-center gap-2">
          <span
            className={`h-2 w-2 rounded-full transition-colors ${
              connected
                ? ptyAlive
                  ? 'bg-status-working shadow-[0_0_6px_1px] shadow-status-working/50'
                  : 'bg-status-needs-input'
                : 'bg-status-needs-input animate-pulse'
            }`}
          />
          <span className="text-xs text-text-muted">
            {!connected ? 'Connecting...' : !ptyAlive ? 'Read-only' : 'Connected'}
          </span>
          {session && (
            <span className="ml-2 max-w-[200px] truncate text-xs text-text-muted/70">
              {session.role || session.prompt?.slice(0, 40)}
            </span>
          )}
        </span>
        <div className="flex items-center gap-1">
          {connected && !ptyAlive && (
            <button
              type="button"
              onClick={handleRestart}
              disabled={restartMutation.isPending}
              className="btn-press rounded-lg bg-accent/15 px-3 py-1 text-xs font-medium text-accent hover:bg-accent/25 disabled:opacity-50 transition-colors"
            >
              {restartMutation.isPending ? 'Restarting...' : 'Restart'}
            </button>
          )}
          {panelMode === 'fullscreen' ? (
            <button
              type="button"
              onClick={() => setPanelMode('panel')}
              className="p-1 rounded hover:bg-white/[0.06] transition-colors"
              title="Minimize"
            >
              <svg
                className="w-4 h-4 text-text-muted"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <polyline points="4 14 10 14 10 20" />
                <polyline points="20 10 14 10 14 4" />
                <line x1="14" y1="10" x2="21" y2="3" />
                <line x1="3" y1="21" x2="10" y2="14" />
              </svg>
            </button>
          ) : (
            <button
              type="button"
              onClick={toggleFullscreen}
              className="p-1 rounded hover:bg-white/[0.06] transition-colors"
              title="Maximize"
            >
              <svg
                className="w-4 h-4 text-text-muted"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <polyline points="15 3 21 3 21 9" />
                <polyline points="9 21 3 21 3 15" />
                <line x1="21" y1="3" x2="14" y2="10" />
                <line x1="3" y1="21" x2="10" y2="14" />
              </svg>
            </button>
          )}
          <button
            type="button"
            onClick={closePanel}
            className="p-1 rounded hover:bg-white/[0.06] transition-colors"
            title="Close"
          >
            <svg
              className="w-4 h-4 text-text-muted"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>
      </div>

      {/* Terminal body */}
      <div className="relative min-h-0 flex-1">
        <div
          ref={containerRef}
          className="absolute inset-0 p-1"
          style={{ backgroundColor: 'hsl(228 25% 5%)' }}
        />

        {/* Mobile keyboard toggle */}
        {ptyAlive && (
          <button
            type="button"
            onClick={() => setInputOpen((o) => !o)}
            className={`absolute bottom-3 right-3 z-10 flex h-10 w-10 items-center justify-center rounded-full shadow-lg md:hidden transition-colors ${
              inputOpen ? 'bg-accent text-white' : 'glass text-text-muted'
            }`}
            aria-label={inputOpen ? 'Hide keyboard' : 'Show keyboard'}
          >
            <svg
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              {inputOpen ? (
                <>
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </>
              ) : (
                <>
                  <rect x="2" y="4" width="20" height="16" rx="2" />
                  <line x1="6" y1="8" x2="6" y2="8" />
                  <line x1="10" y1="8" x2="10" y2="8" />
                  <line x1="14" y1="8" x2="14" y2="8" />
                  <line x1="18" y1="8" x2="18" y2="8" />
                  <line x1="6" y1="12" x2="6" y2="12" />
                  <line x1="10" y1="12" x2="10" y2="12" />
                  <line x1="14" y1="12" x2="14" y2="12" />
                  <line x1="18" y1="12" x2="18" y2="12" />
                  <line x1="8" y1="16" x2="16" y2="16" />
                </>
              )}
            </svg>
          </button>
        )}
      </div>

      {/* Mobile input panel */}
      <AnimatePresence>
        {inputOpen && ptyAlive && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 400, damping: 35 }}
            className="shrink-0 overflow-hidden md:hidden"
          >
            <div className="glass-strong border-t border-border-subtle p-3">
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  handleMobileSend();
                }}
                className="flex gap-2"
              >
                <input
                  type="text"
                  value={mobileInput}
                  onChange={(e) => setMobileInput(e.target.value)}
                  placeholder="Type command..."
                  autoComplete="off"
                  autoCorrect="off"
                  autoCapitalize="off"
                  spellCheck={false}
                  className="min-h-[44px] flex-1 rounded-lg border border-border-subtle bg-bg px-3 text-sm text-text font-mono focus-accent transition-colors"
                />
                <button
                  type="submit"
                  disabled={!connected}
                  className="btn-press min-h-[44px] rounded-lg bg-accent px-4 text-sm font-medium text-white disabled:opacity-50 transition-colors"
                >
                  Send
                </button>
              </form>
              <div className="mt-2 flex gap-1.5 overflow-x-auto">
                {[
                  { label: 'Tab', key: '\t' },
                  { label: 'Ctrl+C', key: '\x03' },
                  { label: 'Esc', key: '\x1b' },
                  { label: 'Up', key: '\x1b[A' },
                  { label: 'Down', key: '\x1b[B' },
                  { label: 'y', key: 'y' },
                  { label: 'n', key: 'n' },
                ].map(({ label, key }) => (
                  <button
                    key={label}
                    type="button"
                    onClick={() => sendInput(key)}
                    disabled={!connected}
                    className="btn-press shrink-0 rounded-lg border border-border-subtle bg-surface px-2.5 py-1 text-xs font-mono text-text-muted active:bg-surface-hover disabled:opacity-50 transition-colors"
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
