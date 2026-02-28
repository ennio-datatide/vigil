'use client';

import { useState } from 'react';
import { useProjectsQuery } from '@/lib/api';
import { useMutation, useQueryClient } from '@tanstack/react-query';

export default function ProjectsPage() {
  const { data: projects, isLoading } = useProjectsQuery();
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const [path, setPath] = useState('');

  const addProject = useMutation({
    mutationFn: async (input: { path: string; name: string }) => {
      const res = await fetch('/api/projects', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(input),
      });
      if (!res.ok) throw new Error('Failed to add project');
      return res.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      setName('');
      setPath('');
    },
  });

  const removeProject = useMutation({
    mutationFn: async (projectPath: string) => {
      const res = await fetch(`/api/projects/${encodeURIComponent(projectPath)}`, {
        method: 'DELETE',
      });
      if (!res.ok) throw new Error('Failed to remove project');
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
    },
  });

  return (
    <div className="p-4">
      <h2 className="mb-4 text-xl font-semibold tracking-tight">Projects</h2>

      {/* Add project form */}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          addProject.mutate({ path, name });
        }}
        className="mb-6 glass rounded-xl p-6 flex gap-2"
      >
        <input
          type="text"
          placeholder="Project name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="flex-1 rounded-lg border border-border-subtle bg-bg px-3 py-2 text-sm focus-accent transition-colors"
          required
        />
        <input
          type="text"
          placeholder="/path/to/project"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          className="flex-1 rounded-lg border border-border-subtle bg-bg px-3 py-2 text-sm focus-accent transition-colors"
          required
        />
        <button
          type="submit"
          disabled={addProject.isPending}
          className="min-h-[44px] btn-press rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-hover transition-colors disabled:opacity-50"
        >
          Add
        </button>
      </form>

      {/* Project list */}
      {isLoading ? (
        <p className="text-text-faint">Loading...</p>
      ) : !projects || projects.length === 0 ? (
        <p className="text-text-faint">No projects registered. Add one above.</p>
      ) : (
        <div className="space-y-2">
          {projects.map((project) => (
            <div
              key={project.path}
              className="flex items-center justify-between glass rounded-xl p-4 hover:bg-surface-hover/50 transition-colors"
            >
              <div>
                <span className="text-sm font-medium">{project.name}</span>
                <span className="ml-2 text-xs text-text-muted">{project.path}</span>
              </div>
              <button
                onClick={() => removeProject.mutate(project.path)}
                disabled={removeProject.isPending}
                className="min-h-[44px] btn-press rounded-lg px-3 py-2 text-xs text-status-error hover:bg-status-error/10 transition-colors disabled:opacity-50"
              >
                Remove
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
