'use client';

import { useCallback, useState } from 'react';
import { PipelineEditor } from '@/components/dashboard/pipeline-editor';
import {
  useCreatePipeline,
  useDeletePipeline,
  usePipelinesQuery,
  useUpdatePipeline,
} from '@/lib/api';
import type { PipelineEdge, PipelineStep } from '@/lib/types';

export default function PipelinesPage() {
  const { data: pipelines, isLoading } = usePipelinesQuery();
  const updatePipeline = useUpdatePipeline();
  const createPipeline = useCreatePipeline();
  const deletePipeline = useDeletePipeline();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState(false);
  const [nameValue, setNameValue] = useState('');

  // Pick the selected pipeline or the first one
  const activePipeline = pipelines?.find((p) => p.id === selectedId) ?? pipelines?.[0] ?? null;

  const handleSave = useCallback(
    (steps: PipelineStep[], edges: PipelineEdge[]) => {
      if (!activePipeline) return;
      updatePipeline.mutate({ id: activePipeline.id, steps, edges });
    },
    [activePipeline, updatePipeline],
  );

  const handleCreatePipeline = useCallback(() => {
    createPipeline.mutate(
      {
        name: 'New Pipeline',
        steps: [
          {
            id: 'step-1',
            skill: 'new-skill',
            label: 'First Step',
            agent: 'claude',
            prompt: 'Describe what this step should do.',
            position: { x: 200, y: 100 },
          },
        ],
        edges: [],
      },
      {
        onSuccess: (pipeline) => setSelectedId(pipeline.id),
      },
    );
  }, [createPipeline]);

  const handleDeletePipeline = useCallback(() => {
    if (!activePipeline) return;
    if (pipelines && pipelines.length <= 1) return;
    deletePipeline.mutate(activePipeline.id, {
      onSuccess: () => setSelectedId(null),
    });
  }, [activePipeline, pipelines, deletePipeline]);

  const handleNameSave = useCallback(() => {
    if (!activePipeline || !nameValue.trim()) return;
    updatePipeline.mutate({ id: activePipeline.id, name: nameValue.trim() });
    setEditingName(false);
  }, [activePipeline, nameValue, updatePipeline]);

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <span className="text-text-muted text-sm">Loading pipelines...</span>
      </div>
    );
  }

  if (!activePipeline) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4">
        <p className="text-text-muted text-sm">No pipelines configured.</p>
        <button
          type="button"
          onClick={handleCreatePipeline}
          className="btn-press rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-hover"
        >
          Create Pipeline
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="flex items-center gap-3 border-b border-border px-4 py-3">
        {/* Pipeline selector */}
        {pipelines && pipelines.length > 1 && (
          <select
            value={activePipeline.id}
            onChange={(e) => setSelectedId(e.target.value)}
            className="glass rounded-lg border-border-subtle bg-bg px-2 py-1 text-sm text-text"
          >
            {pipelines.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} {p.isDefault ? '(Default)' : ''}
              </option>
            ))}
          </select>
        )}

        {/* Pipeline name */}
        {editingName ? (
          <input
            value={nameValue}
            onChange={(e) => setNameValue(e.target.value)}
            onBlur={handleNameSave}
            onKeyDown={(e) => e.key === 'Enter' && handleNameSave()}
            className="focus-accent rounded-lg border border-border-subtle bg-bg px-2 py-1 text-xl font-semibold tracking-tight text-text"
          />
        ) : (
          <button
            type="button"
            className="cursor-pointer text-xl font-semibold tracking-tight text-text hover:text-accent bg-transparent border-none p-0"
            onClick={() => {
              setNameValue(activePipeline.name);
              setEditingName(true);
            }}
          >
            {activePipeline.name}
          </button>
        )}

        {activePipeline.isDefault && (
          <span className="rounded-full bg-accent/15 px-2 py-0.5 text-[10px] font-medium text-accent">
            Default
          </span>
        )}

        <div className="flex-1" />

        <button
          type="button"
          onClick={handleCreatePipeline}
          disabled={createPipeline.isPending}
          className="btn-press rounded-lg bg-accent/15 text-accent hover:bg-accent/25 px-3 py-1.5 text-xs"
        >
          New Pipeline
        </button>

        {!activePipeline.isDefault && pipelines && pipelines.length > 1 && (
          <button
            type="button"
            onClick={handleDeletePipeline}
            disabled={deletePipeline.isPending}
            className="btn-press rounded-lg text-xs text-status-error hover:bg-status-error/10 px-3 py-1.5"
          >
            Delete
          </button>
        )}
      </div>

      {/* Editor */}
      <div className="min-h-0 flex-1">
        <PipelineEditor
          key={activePipeline.id}
          pipeline={activePipeline}
          onSave={handleSave}
          saving={updatePipeline.isPending}
        />
      </div>
    </div>
  );
}
