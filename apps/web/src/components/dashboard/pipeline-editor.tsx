'use client';

import {
  addEdge,
  Background,
  BackgroundVariant,
  type Connection,
  Controls,
  type Edge,
  type Node,
  ReactFlow,
  useEdgesState,
  useNodesState,
} from '@xyflow/react';
import { useCallback, useMemo, useState } from 'react';
import '@xyflow/react/dist/style.css';
import { nanoid } from 'nanoid';
import type { Pipeline, PipelineEdge, PipelineStep } from '@/lib/types';
import { PipelineStepNode } from './pipeline-step-node';
import { StepConfigPanel } from './step-config-panel';

interface PipelineEditorProps {
  pipeline: Pipeline;
  onSave: (steps: PipelineStep[], edges: PipelineEdge[]) => void;
  saving?: boolean;
}

function stepsToNodes(steps: PipelineStep[]): Node[] {
  return steps.map((step) => ({
    id: step.id,
    type: 'pipelineStep',
    position: step.position,
    data: step,
  }));
}

function edgesToFlowEdges(edges: PipelineEdge[]): Edge[] {
  return edges.map((edge) => ({
    id: `${edge.source}-${edge.target}`,
    source: edge.source,
    target: edge.target,
    animated: true,
    style: { stroke: 'var(--color-text-muted)', strokeWidth: 2 },
  }));
}

function nodesToSteps(nodes: Node[]): PipelineStep[] {
  return nodes.map((node) => ({
    ...(node.data as PipelineStep),
    position: node.position,
  }));
}

function flowEdgesToEdges(edges: Edge[]): PipelineEdge[] {
  return edges.map((edge) => ({
    source: edge.source,
    target: edge.target,
  }));
}

export function PipelineEditor({ pipeline, onSave, saving }: PipelineEditorProps) {
  const nodeTypes = useMemo(() => ({ pipelineStep: PipelineStepNode }), []);

  const [nodes, setNodes, onNodesChange] = useNodesState(stepsToNodes(pipeline.steps));
  const [edges, setEdges, onEdgesChange] = useEdgesState(edgesToFlowEdges(pipeline.edges));
  const [selectedStepId, setSelectedStepId] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);

  const selectedStep = useMemo(() => {
    if (!selectedStepId) return null;
    const node = nodes.find((n) => n.id === selectedStepId);
    return node ? (node.data as PipelineStep) : null;
  }, [selectedStepId, nodes]);

  const onConnect = useCallback(
    (connection: Connection) => {
      // Prevent self-connections
      if (connection.source === connection.target) return;
      setEdges((eds) =>
        addEdge(
          {
            ...connection,
            animated: true,
            style: { stroke: 'var(--color-text-muted)', strokeWidth: 2 },
          },
          eds,
        ),
      );
      setDirty(true);
    },
    [setEdges],
  );

  const handleNodeClick = useCallback((_: React.MouseEvent, node: Node) => {
    setSelectedStepId(node.id);
  }, []);

  const handlePaneClick = useCallback(() => {
    setSelectedStepId(null);
  }, []);

  const handleNodesChange: typeof onNodesChange = useCallback(
    (changes) => {
      onNodesChange(changes);
      // Mark dirty if position changed
      if (changes.some((c) => c.type === 'position' && 'position' in c)) {
        setDirty(true);
      }
    },
    [onNodesChange],
  );

  const handleEdgesChange: typeof onEdgesChange = useCallback(
    (changes) => {
      onEdgesChange(changes);
      if (changes.some((c) => c.type === 'remove')) {
        setDirty(true);
      }
    },
    [onEdgesChange],
  );

  const addStep = useCallback(() => {
    const id = nanoid(8);
    const newStep: PipelineStep = {
      id,
      skill: 'new-skill',
      label: 'New Step',
      agent: 'claude',
      prompt: 'Describe what this step should do.',
      position: { x: 300, y: nodes.length * 100 },
    };
    const newNode: Node = {
      id,
      type: 'pipelineStep',
      position: newStep.position,
      data: newStep,
    };
    setNodes((nds) => [...nds, newNode]);
    setSelectedStepId(id);
    setDirty(true);
  }, [nodes.length, setNodes]);

  const updateStep = useCallback(
    (updatedStep: PipelineStep) => {
      setNodes((nds) =>
        nds.map((n) => (n.id === updatedStep.id ? { ...n, data: updatedStep } : n)),
      );
      setDirty(true);
    },
    [setNodes],
  );

  const deleteStep = useCallback(
    (stepId: string) => {
      setNodes((nds) => nds.filter((n) => n.id !== stepId));
      setEdges((eds) => eds.filter((e) => e.source !== stepId && e.target !== stepId));
      setSelectedStepId(null);
      setDirty(true);
    },
    [setNodes, setEdges],
  );

  const handleSave = useCallback(() => {
    onSave(nodesToSteps(nodes), flowEdgesToEdges(edges));
    setDirty(false);
  }, [nodes, edges, onSave]);

  return (
    <div className="flex h-full">
      <div className="flex-1 flex flex-col">
        {/* Toolbar */}
        <div className="glass-strong rounded-xl flex items-center gap-3 border-b border-border px-4 py-2">
          <button
            type="button"
            onClick={addStep}
            className="btn-press rounded-lg bg-accent/15 text-accent hover:bg-accent/25 px-3 py-1.5 text-xs"
          >
            + Add Step
          </button>
          <div className="flex-1" />
          {dirty && <span className="text-xs text-accent">Unsaved changes</span>}
          <button
            type="button"
            onClick={handleSave}
            disabled={saving || !dirty}
            className="btn-press rounded-lg bg-accent text-white hover:bg-accent-hover disabled:opacity-50 px-4 py-1.5 text-xs font-medium"
          >
            {saving ? 'Saving...' : 'Save'}
          </button>
        </div>

        {/* Canvas */}
        <div className="flex-1">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            onNodesChange={handleNodesChange}
            onEdgesChange={handleEdgesChange}
            onConnect={onConnect}
            onNodeClick={handleNodeClick}
            onPaneClick={handlePaneClick}
            nodeTypes={nodeTypes}
            fitView
            deleteKeyCode="Backspace"
            proOptions={{ hideAttribution: true }}
          >
            <Background
              variant={BackgroundVariant.Dots}
              gap={20}
              size={1}
              color="var(--color-border)"
            />
            <Controls
              showInteractive={false}
              className="!bg-surface !border-border !shadow-sm [&>button]:!bg-surface [&>button]:!border-border [&>button]:!fill-text-muted [&>button:hover]:!bg-surface-hover"
            />
          </ReactFlow>
        </div>
      </div>

      {/* Side panel */}
      {selectedStep && (
        <StepConfigPanel
          step={selectedStep}
          onUpdate={updateStep}
          onDelete={deleteStep}
          onClose={() => setSelectedStepId(null)}
        />
      )}
    </div>
  );
}
