'use client';

import { useEffect, useState } from 'react';
import type { PipelineStep } from '@/lib/types';

interface StepConfigPanelProps {
  step: PipelineStep;
  onUpdate: (step: PipelineStep) => void;
  onDelete: (stepId: string) => void;
  onClose: () => void;
}

export function StepConfigPanel({ step, onUpdate, onDelete, onClose }: StepConfigPanelProps) {
  const [label, setLabel] = useState(step.label);
  const [skill, setSkill] = useState(step.skill);
  const [agent, setAgent] = useState(step.agent);
  const [prompt, setPrompt] = useState(step.prompt);
  const [confirmDelete, setConfirmDelete] = useState(false);

  // Sync local state when step changes
  useEffect(() => {
    setLabel(step.label);
    setSkill(step.skill);
    setAgent(step.agent);
    setPrompt(step.prompt);
    setConfirmDelete(false);
  }, [step.label, step.skill, step.agent, step.prompt]);

  const handleUpdate = () => {
    onUpdate({ ...step, label, skill, agent, prompt });
  };

  return (
    <div className="glass-strong h-full w-80 shrink-0 border-l border-border-subtle p-4 overflow-y-auto">
      <div className="mb-4 flex items-center justify-between">
        <h3 className="text-sm font-medium text-text-muted">Step Config</h3>
        <button
          type="button"
          onClick={onClose}
          className="rounded-lg p-1 text-text-muted hover:bg-surface-hover transition-colors text-lg leading-none"
        >
          &times;
        </button>
      </div>

      <div className="space-y-4">
        <div>
          <label className="mb-1 block text-sm font-medium text-text-muted">Label</label>
          <input
            type="text"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            onBlur={handleUpdate}
            className="w-full rounded-lg border border-border-subtle bg-bg p-2 text-sm text-text focus-accent transition-colors"
          />
        </div>

        <div>
          <label className="mb-1 block text-sm font-medium text-text-muted">Skill</label>
          <input
            type="text"
            value={skill}
            onChange={(e) => setSkill(e.target.value)}
            onBlur={handleUpdate}
            className="w-full rounded-lg border border-border-subtle bg-bg p-2 text-sm text-text focus-accent transition-colors"
          />
        </div>

        <div>
          <label className="mb-1 block text-sm font-medium text-text-muted">Agent</label>
          <div className="space-y-2">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="agent"
                checked={agent === 'claude'}
                onChange={() => {
                  setAgent('claude');
                  onUpdate({ ...step, label, skill, agent: 'claude', prompt });
                }}
                className="accent-accent"
              />
              <span className="text-sm text-text">Claude</span>
            </label>
            <label className="flex items-center gap-2 cursor-not-allowed opacity-50">
              <input type="radio" name="agent" disabled />
              <span className="text-sm text-text-muted">Codex</span>
              <span className="rounded bg-surface-hover px-1.5 py-0.5 text-[10px] text-text-muted">
                Coming Soon
              </span>
            </label>
          </div>
        </div>

        <div>
          <label className="mb-1 block text-sm font-medium text-text-muted">Prompt Template</label>
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            onBlur={handleUpdate}
            rows={4}
            className="w-full rounded-lg border border-border-subtle bg-bg p-2 text-sm text-text focus-accent transition-colors resize-y"
          />
        </div>

        <div className="pt-2 border-t border-border">
          {confirmDelete ? (
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => onDelete(step.id)}
                className="btn-press flex-1 rounded-lg bg-status-error/10 text-status-error hover:bg-status-error/20 px-3 py-2 text-sm"
              >
                Confirm Delete
              </button>
              <button
                type="button"
                onClick={() => setConfirmDelete(false)}
                className="rounded-md border border-border px-3 py-2 text-sm text-text-muted"
              >
                Cancel
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setConfirmDelete(true)}
              className="btn-press w-full rounded-lg bg-status-error/10 text-status-error hover:bg-status-error/20 px-3 py-2 text-sm"
            >
              Delete Step
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
