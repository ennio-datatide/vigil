'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { SessionMonitor } from '@/components/vigil/session-monitor';
import { VigilChat } from '@/components/vigil/vigil-chat';
import { useDashboardWs } from '@/lib/hooks/use-dashboard-ws';
import { useSessionStore } from '@/lib/stores/session-store';

export default function DashboardPage() {
  useDashboardWs();

  const sessions = useSessionStore((s) => Object.values(s.sessions));
  const hasActiveSessions = sessions.some((s) =>
    ['queued', 'running', 'needs_input', 'auth_required'].includes(s.status),
  );

  return (
    <div className="flex h-full">
      <div
        className={`flex-1 transition-all duration-300 ${hasActiveSessions ? 'md:w-[55%] md:flex-none' : ''}`}
      >
        <VigilChat />
      </div>

      <AnimatePresence>
        {hasActiveSessions && (
          <motion.div
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: '45%', opacity: 1 }}
            exit={{ width: 0, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 300, damping: 30 }}
            className="hidden border-l border-border-subtle overflow-hidden md:block"
          >
            <SessionMonitor />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
