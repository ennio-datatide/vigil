'use client';

import { useQuery } from '@tanstack/react-query';
import type { Session } from '@/lib/types';
import { StatusBadge } from '@/components/dashboard/status-badge';
import Link from 'next/link';
import { useState } from 'react';

export default function HistoryPage() {
  const [search, setSearch] = useState('');
  const { data: sessions, isLoading } = useQuery({
    queryKey: ['sessions-history'],
    queryFn: async () => {
      const res = await fetch('/api/sessions');
      if (!res.ok) return [];
      const data = await res.json();
      return Array.isArray(data) ? data as Session[] : [];
    },
  });

  const filtered = sessions
    ?.filter((s) => ['completed', 'failed', 'cancelled'].includes(s.status))
    ?.filter((s) => !search || s.prompt.toLowerCase().includes(search.toLowerCase()))
    ?? [];

  return (
    <div className="p-4">
      <h2 className="mb-4 text-xl font-semibold tracking-tight">Session History</h2>
      <input
        type="text"
        placeholder="Search by prompt..."
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        className="mb-4 w-full glass rounded-lg border border-border-subtle bg-bg p-2 text-sm text-text focus-accent"
      />
      {isLoading ? (
        <p className="text-text-faint">Loading...</p>
      ) : filtered.length === 0 ? (
        <p className="text-text-faint">No completed sessions yet.</p>
      ) : (
        <div className="space-y-2">
          {filtered.map((session) => (
            <Link
              key={session.id}
              href={`/dashboard/sessions/${session.id}`}
              className="block glass rounded-xl p-4 hover:bg-surface-hover/50 transition-colors"
            >
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">{session.agentType}</span>
                <StatusBadge status={session.status} />
              </div>
              <p className="mt-1 text-sm text-text-muted line-clamp-1">{session.prompt}</p>
              <div className="mt-1 text-xs text-text-faint">
                {session.endedAt ? new Date(session.endedAt).toLocaleString() : '--'}
                {' | '}{session.projectPath.split('/').pop()}
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
