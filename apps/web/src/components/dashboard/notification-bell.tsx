'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import { useMarkNotificationRead, useNotificationsQuery } from '@/lib/api';

export function NotificationBell() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const { data: notifications = [] } = useNotificationsQuery(false);
  const markRead = useMarkNotificationRead();

  const unread = notifications.filter((n) => !n.readAt).length;

  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    if (open) document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="btn-press relative rounded-lg p-2 text-text-muted hover:bg-surface-hover hover:text-text transition-colors"
        aria-label="Notifications"
      >
        <svg
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
          <path d="M13.73 21a2 2 0 0 1-3.46 0" />
        </svg>
        {unread > 0 && (
          <motion.span
            key={unread}
            initial={{ scale: 0.5 }}
            animate={{ scale: 1 }}
            className="absolute -right-0.5 -top-0.5 flex h-4 min-w-[16px] items-center justify-center rounded-full bg-accent px-1 text-[10px] font-bold text-white"
          >
            {unread}
          </motion.span>
        )}
      </button>

      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: -8, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -8, scale: 0.95 }}
            transition={{ type: 'spring', stiffness: 400, damping: 30 }}
            className="glass absolute right-0 top-full z-50 mt-2 w-72 rounded-xl p-2 shadow-2xl md:left-0 md:right-auto"
          >
            {notifications.length === 0 ? (
              <p className="px-3 py-6 text-center text-xs text-text-muted">No notifications</p>
            ) : (
              <div className="max-h-60 space-y-0.5 overflow-y-auto">
                {notifications.map((n) => (
                  <button
                    type="button"
                    key={n.id}
                    onClick={() => {
                      if (!n.readAt) markRead.mutate(n.id);
                    }}
                    className={`w-full rounded-lg px-3 py-2 text-left text-xs transition-colors hover:bg-surface-hover ${
                      n.readAt ? 'text-text-muted' : 'text-text'
                    }`}
                  >
                    <span className="flex items-start gap-2">
                      {!n.readAt && (
                        <span className="mt-1 h-1.5 w-1.5 shrink-0 rounded-full bg-accent" />
                      )}
                      <span className={!n.readAt ? '' : 'pl-3.5'}>{n.message}</span>
                    </span>
                  </button>
                ))}
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
