'use client';

import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import type { PipelineStep } from '@/lib/types';

type PipelineStepNodeData = PipelineStep & { selected?: boolean };

function PipelineStepNodeComponent({ data, selected }: NodeProps & { data: PipelineStepNodeData }) {
  const isCodex = data.agent === 'codex';

  return (
    <div
      className={`glass w-56 rounded-xl p-3 shadow-sm transition-all ${
        selected ? 'border-accent ring-2 ring-accent/30' : 'border-border-subtle'
      }`}
    >
      <Handle type="target" position={Position.Top} className="!bg-accent !w-2 !h-2" />

      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-sm font-medium text-text truncate">{data.label}</span>
        {isCodex ? (
          <span className="shrink-0 rounded-full bg-surface-alt px-2 py-0.5 text-[10px] text-text-muted">
            Codex - Soon
          </span>
        ) : (
          <span className="shrink-0 rounded-full bg-accent/15 px-2 py-0.5 text-[10px] text-accent">
            Claude
          </span>
        )}
      </div>

      <p className="text-xs text-text-muted line-clamp-2">{data.prompt}</p>

      <Handle type="source" position={Position.Bottom} className="!bg-accent !w-2 !h-2" />
    </div>
  );
}

export const PipelineStepNode = memo(PipelineStepNodeComponent);
