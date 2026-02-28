'use client';

import { motion, AnimatePresence } from 'framer-motion';

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  variant?: 'default' | 'danger';
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel = 'Confirm',
  variant = 'default',
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="fixed inset-0 z-60 flex items-center justify-center bg-black/60 p-4"
          onClick={onCancel}
        >
          <motion.div
            initial={{ opacity: 0, scale: 0.95, y: 8 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: 8 }}
            transition={{ type: 'spring', stiffness: 400, damping: 30 }}
            onClick={(e) => e.stopPropagation()}
            className="glass w-full max-w-sm rounded-xl p-6 shadow-2xl"
          >
            <h3 className="text-base font-semibold">{title}</h3>
            <p className="mt-2 text-sm text-text-muted leading-relaxed">{message}</p>
            <div className="mt-5 flex justify-end gap-3">
              <button
                onClick={onCancel}
                className="btn-press rounded-lg px-4 py-2 text-sm text-text-muted hover:bg-surface-hover transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={onConfirm}
                className={`btn-press rounded-lg px-4 py-2 text-sm font-medium transition-colors ${
                  variant === 'danger'
                    ? 'bg-status-error/15 text-status-error hover:bg-status-error/25'
                    : 'bg-accent/15 text-accent hover:bg-accent/25'
                }`}
              >
                {confirmLabel}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
