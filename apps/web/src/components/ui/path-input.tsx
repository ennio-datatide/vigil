'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useDirsQuery, useProjectsQuery } from '@/lib/api';

export function PathInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
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

  const select = useCallback(
    (path: string) => {
      // Append / so the user can keep drilling down
      onChange(path.endsWith('/') ? path : `${path}/`);
      setHighlightIdx(-1);
      inputRef.current?.focus();
    },
    [onChange],
  );

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
  useEffect(() => {
    setHighlightIdx(-1);
  }, []);

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
              {s.label !== s.path && <span className="ml-2 text-xs text-text-muted">{s.path}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
