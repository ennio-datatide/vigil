'use client';

import { useState, useRef, useEffect, useCallback } from 'react';
import { useRouter, usePathname } from 'next/navigation';
import { AnimatePresence, motion } from 'framer-motion';
import { useCreateSession, useProjectsQuery, useDirsQuery, usePipelinesQuery } from '@/lib/api';
import { useToast } from '@/lib/stores/toast-store';

function PathInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [focused, setFocused] = useState(false);
  const [highlightIdx, setHighlightIdx] = useState(-1);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const { data: projects } = useProjectsQuery();

  // Debounce: only query after user stops typing for 200ms
  const [debouncedPrefix, setDebouncedPrefix] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedPrefix(value), 200);
    return () => clearTimeout(timer);
  }, [value]);

  const { data: dirsResult } = useDirsQuery(debouncedPrefix);
  const dirs = dirsResult?.dirs ?? [];

  // Build suggestion list: registered projects first (if input is short), then fs dirs
  const suggestions: { label: string; path: string }[] = [];

  if (value.length < 3 && projects && projects.length > 0) {
    for (const p of projects) {
      suggestions.push({ label: p.name, path: p.path });
    }
  }

  for (const d of dirs) {
    if (!suggestions.some((s) => s.path === d)) {
      const label = d.split('/').pop() || d;
      suggestions.push({ label, path: d });
    }
  }

  const showDropdown = focused && suggestions.length > 0;

  const select = useCallback((path: string) => {
    // Append / so the user can keep drilling down
    onChange(path.endsWith('/') ? path : path + '/');
    setHighlightIdx(-1);
    inputRef.current?.focus();
  }, [onChange]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!showDropdown) return;

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setHighlightIdx((i) => Math.min(i + 1, suggestions.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlightIdx((i) => Math.max(i - 1, 0));
    } else if (e.key === 'Tab' || e.key === 'Enter') {
      if (highlightIdx >= 0 && highlightIdx < suggestions.length) {
        e.preventDefault();
        select(suggestions[highlightIdx].path);
      }
    } else if (e.key === 'Escape') {
      setFocused(false);
    }
  };

  // Scroll highlighted item into view
  useEffect(() => {
    if (highlightIdx >= 0 && listRef.current) {
      const item = listRef.current.children[highlightIdx] as HTMLElement | undefined;
      item?.scrollIntoView({ block: 'nearest' });
    }
  }, [highlightIdx]);

  // Reset highlight when suggestions change
  useEffect(() => { setHighlightIdx(-1); }, [debouncedPrefix]);

  return (
    <div className="relative">
      <input
        ref={inputRef}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => setTimeout(() => setFocused(false), 150)}
        onKeyDown={handleKeyDown}
        placeholder="~/Code/my-project or start typing..."
        className="w-full rounded-lg border border-border-subtle bg-bg px-3 py-2.5 text-sm font-mono focus-accent transition-colors"
        autoComplete="off"
        required
      />
      {showDropdown && (
        <ul
          ref={listRef}
          className="absolute z-10 mt-1 max-h-48 w-full overflow-auto glass rounded-lg shadow-xl"
        >
          {suggestions.map((s, i) => (
            <li
              key={s.path}
              onMouseDown={() => select(s.path)}
              className={`cursor-pointer px-3 py-2 text-sm ${
                i === highlightIdx
                  ? 'bg-accent/20 text-text'
                  : 'text-text-muted hover:bg-surface-hover'
              }`}
            >
              <span className="font-medium text-text">{s.label}</span>
              {s.label !== s.path && (
                <span className="ml-2 text-xs text-text-muted">{s.path}</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Modal content — rendered by NewSession which manages open state */
function NewSessionModal({ onClose }: { onClose: () => void }) {
  const router = useRouter();
  const [projectPath, setProjectPath] = useState('');
  const [prompt, setPrompt] = useState('');
  const [skill, setSkill] = useState('');
  const [pipelineId, setPipelineId] = useState('');
  const [skipPermissions, setSkipPermissions] = useState(false);
  const createSession = useCreateSession();
  const { data: pipelines } = usePipelinesQuery();
  const toast = useToast();

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const cleanPath = projectPath.replace(/\/+$/, '');
    createSession.mutate(
      { projectPath: cleanPath, prompt, skill: skill || undefined, pipelineId: pipelineId || undefined, skipPermissions: skipPermissions || undefined },
      {
        onSuccess: (session) => {
          onClose();
          toast.success('Session started');
          router.push(`/dashboard/sessions/${session.id}`);
        },
        onError: () => toast.error('Failed to start session'),
      },
    );
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={onClose}
    >
      <motion.div
        initial={{ scale: 0.95, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        exit={{ scale: 0.95, opacity: 0 }}
        className="glass w-full max-w-md rounded-2xl p-6 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="mb-4 text-lg font-bold">New Session</h2>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="mb-1 block text-sm font-medium text-text-faint">Project path</label>
            <PathInput value={projectPath} onChange={setProjectPath} />
          </div>
          <div>
            <label className="mb-1 block text-sm font-medium text-text-faint">Prompt</label>
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="What should the agent do?"
              rows={3}
              className="w-full rounded-lg border border-border-subtle bg-bg px-3 py-2.5 text-sm focus-accent transition-colors"
              required
            />
          </div>
          <div>
            <label className="mb-1 block text-sm font-medium text-text-faint">Skill (optional)</label>
            <input
              type="text"
              value={skill}
              onChange={(e) => setSkill(e.target.value)}
              placeholder="e.g. test-driven-development"
              className="w-full rounded-lg border border-border-subtle bg-bg px-3 py-2.5 text-sm focus-accent transition-colors"
            />
          </div>
          {pipelines && pipelines.length > 0 && (
            <div>
              <label className="mb-1 block text-sm font-medium text-text-faint">Pipeline (optional)</label>
              <select
                value={pipelineId}
                onChange={(e) => setPipelineId(e.target.value)}
                className="w-full rounded-lg border border-border-subtle bg-bg px-3 py-2.5 text-sm focus-accent transition-colors"
              >
                <option value="">No pipeline</option>
                {pipelines.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} ({p.steps.length} steps){p.isDefault ? ' - Default' : ''}
                  </option>
                ))}
              </select>
            </div>
          )}
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={skipPermissions}
              onChange={(e) => setSkipPermissions(e.target.checked)}
              className="h-4 w-4 rounded border-border accent-accent"
            />
            <span className="text-sm text-text-muted">Auto-approve all tools</span>
          </label>
          <div className="flex gap-3">
            <button
              type="submit"
              disabled={createSession.isPending}
              className="btn-press w-full flex-1 rounded-lg bg-accent py-3 text-sm font-semibold text-white hover:bg-accent-hover transition-colors disabled:opacity-50"
            >
              {createSession.isPending ? 'Starting...' : 'Start Session'}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="btn-press rounded-lg px-4 py-2 text-sm text-text-muted hover:bg-surface-hover transition-colors"
            >
              Cancel
            </button>
          </div>
        </form>
      </motion.div>
    </motion.div>
  );
}

export function NewSession() {
  const [open, setOpen] = useState(false);
  const pathname = usePathname();
  // Hide mobile FAB on session detail pages where terminal input bar lives
  const hideFloatingButton = pathname.startsWith('/dashboard/sessions/');

  // Cmd+N / Ctrl+N keyboard shortcut
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'n') {
        e.preventDefault();
        setOpen(true);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);

  return (
    <>
      {/* Desktop: fixed button in sidebar area (w-56 = 14rem sidebar) */}
      <button
        onClick={() => setOpen(true)}
        className="btn-press glass fixed bottom-4 left-3 z-40 hidden w-[calc(14rem-1.5rem)] items-center gap-2 rounded-xl px-3 py-2 text-sm font-medium text-accent transition-colors hover:bg-accent/10 md:flex"
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
          <line x1="12" y1="5" x2="12" y2="19" />
          <line x1="5" y1="12" x2="19" y2="12" />
        </svg>
        New Session
        <kbd className="ml-auto rounded border border-border px-1.5 py-0.5 text-[10px] text-text-muted">&#8984;N</kbd>
      </button>

      {/* Mobile: FAB bottom-right — hidden on session pages to avoid overlapping terminal input */}
      <button
        onClick={() => setOpen(true)}
        className={`btn-press fixed bottom-20 right-4 z-50 h-14 w-14 items-center justify-center rounded-full bg-accent text-white shadow-lg shadow-accent/25 hover:bg-accent-hover transition-colors md:hidden ${hideFloatingButton ? 'hidden' : 'flex'}`}
        aria-label="New session"
      >
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
          <line x1="12" y1="5" x2="12" y2="19" />
          <line x1="5" y1="12" x2="19" y2="12" />
        </svg>
      </button>

      <AnimatePresence>
        {open && <NewSessionModal onClose={() => setOpen(false)} />}
      </AnimatePresence>
    </>
  );
}

