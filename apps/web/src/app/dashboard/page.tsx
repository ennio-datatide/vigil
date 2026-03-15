'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useState } from 'react';
import { TerminalPanel } from '@/components/dashboard/terminal-panel';
import { SessionMonitor } from '@/components/vigil/session-monitor';
import { VigilChat } from '@/components/vigil/vigil-chat';
import { useDashboardWs } from '@/lib/hooks/use-dashboard-ws';
import { useSessionStore } from '@/lib/stores/session-store';
import { useTerminalStore } from '@/lib/stores/terminal-store';

export default function DashboardPage() {
  useDashboardWs();

  const sessionCount = useSessionStore((s) => Object.keys(s.sessions).length);
  const hasActiveSessions = useSessionStore((s) =>
    Object.values(s.sessions).some((session) =>
      ['queued', 'running', 'needs_input', 'auth_required'].includes(session.status),
    ),
  );

  const [panelOpen, setPanelOpen] = useState(false);
  const { activeSessionId, panelMode, closePanel } = useTerminalStore();

  // Auto-open session monitor when active sessions appear, but let user close manually.
  useEffect(() => {
    if (hasActiveSessions) setPanelOpen(true);
  }, [hasActiveSessions]);

  const showSessionMonitor = panelOpen && sessionCount > 0 && panelMode === 'closed';

  // Fullscreen terminal mode — takes entire viewport
  if (panelMode === 'fullscreen' && activeSessionId) {
    return (
      <div className="h-full w-full relative bg-background">
        <TerminalPanel sessionId={activeSessionId} />
        <button
          type="button"
          onClick={closePanel}
          className="absolute top-3 left-3 z-10 flex items-center gap-2 px-3 py-1.5 rounded-lg bg-surface/80 backdrop-blur border border-border text-sm text-text-muted hover:text-text transition-colors"
        >
          <svg
            className="w-4 h-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <line x1="19" y1="12" x2="5" y2="12" />
            <polyline points="12 19 5 12 12 5" />
          </svg>
          Back to Vigil
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-full">
      {/* Vigil chat — shrinks when terminal panel is open */}
      <div
        className={`flex-1 transition-all duration-300 ${
          panelMode === 'panel' && activeSessionId
            ? 'md:w-1/2 md:flex-none'
            : showSessionMonitor
              ? 'md:w-[55%] md:flex-none'
              : ''
        }`}
      >
        <VigilChat onSessionClick={() => setPanelOpen(true)} />
      </div>

      {/* Toggle button when session monitor is closed but sessions exist */}
      {!showSessionMonitor && panelMode === 'closed' && sessionCount > 0 && (
        <button
          type="button"
          onClick={() => setPanelOpen(true)}
          className="fixed right-4 top-4 z-20 hidden items-center gap-1.5 rounded-lg border border-white/10 bg-surface-alt px-3 py-1.5 text-xs text-text-muted transition-colors hover:text-text md:flex"
        >
          <span className="h-1.5 w-1.5 rounded-full bg-accent" />
          {sessionCount} session{sessionCount !== 1 ? 's' : ''}
        </button>
      )}

      {/* Terminal panel — slides in from right */}
      <AnimatePresence>
        {panelMode === 'panel' && activeSessionId && (
          <motion.div
            key="terminal-panel"
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: '50%', opacity: 1 }}
            exit={{ width: 0, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 300, damping: 30 }}
            className="hidden border-l border-border-subtle overflow-hidden md:block"
          >
            <TerminalPanel sessionId={activeSessionId} />
          </motion.div>
        )}
      </AnimatePresence>

      {/* Session monitor — only when terminal panel is closed */}
      <AnimatePresence>
        {showSessionMonitor && (
          <motion.div
            key="session-monitor"
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: '45%', opacity: 1 }}
            exit={{ width: 0, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 300, damping: 30 }}
            className="hidden border-l border-border-subtle overflow-hidden md:block"
          >
            <SessionMonitor onClose={() => setPanelOpen(false)} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
