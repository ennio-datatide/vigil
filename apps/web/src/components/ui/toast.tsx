'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { useToastStore } from '@/lib/stores/toast-store';

const TYPE_COLORS = {
  success: 'border-l-status-working',
  error: 'border-l-status-error',
  info: 'border-l-accent',
} as const;

export function ToastContainer() {
  const toasts = useToastStore((s) => s.toasts);
  const remove = useToastStore((s) => s.remove);

  return (
    <div className="fixed bottom-20 right-4 z-100 flex flex-col gap-2 md:bottom-6">
      <AnimatePresence>
        {toasts.map((t) => (
          <motion.button
            key={t.id}
            initial={{ opacity: 0, y: 20, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, x: 40, scale: 0.95 }}
            transition={{ type: 'spring', stiffness: 400, damping: 30 }}
            onClick={() => remove(t.id)}
            className={`glass cursor-pointer rounded-lg border-l-4 px-4 py-3 text-sm text-text shadow-lg ${TYPE_COLORS[t.type]}`}
          >
            {t.message}
          </motion.button>
        ))}
      </AnimatePresence>
    </div>
  );
}
