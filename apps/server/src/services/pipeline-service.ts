import type { Pipeline, PipelineEdge, PipelineStep } from '@praefectus/shared';
import { eq } from 'drizzle-orm';
import { nanoid } from 'nanoid';
import type { Db } from '../db/client.js';
import { pipelines } from '../db/schema.js';

interface CreatePipelineInput {
  name: string;
  description?: string;
  steps: PipelineStep[];
  edges: PipelineEdge[];
  isDefault?: boolean;
}

interface UpdatePipelineInput {
  name?: string;
  description?: string;
  steps?: PipelineStep[];
  edges?: PipelineEdge[];
  isDefault?: boolean;
}

function rowToPipeline(row: typeof pipelines.$inferSelect): Pipeline {
  let steps: PipelineStep[] = [];
  let edges: PipelineEdge[] = [];
  try {
    steps = JSON.parse(row.steps) as PipelineStep[];
  } catch {
    /* corrupted steps — default to empty */
  }
  try {
    edges = JSON.parse(row.edges) as PipelineEdge[];
  } catch {
    /* corrupted edges — default to empty */
  }

  return {
    id: row.id,
    name: row.name,
    description: row.description ?? '',
    steps,
    edges,
    isDefault: row.isDefault === 1,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
  };
}

const DEFAULT_STEPS: PipelineStep[] = [
  {
    id: 'step-brainstorm',
    skill: 'brainstorming',
    label: 'Brainstorm',
    agent: 'claude',
    prompt: 'Run the brainstorming skill to explore the problem space.',
    position: { x: 0, y: 0 },
  },
  {
    id: 'step-worktree',
    skill: 'using-git-worktrees',
    label: 'Setup Worktree',
    agent: 'claude',
    prompt: 'Set up a git worktree for isolated development.',
    position: { x: 250, y: 0 },
  },
  {
    id: 'step-plan',
    skill: 'writing-plans',
    label: 'Write Plan',
    agent: 'claude',
    prompt: 'Write a detailed implementation plan.',
    position: { x: 500, y: 0 },
  },
  {
    id: 'step-implement',
    skill: 'subagent-driven-development',
    label: 'Implement',
    agent: 'claude',
    prompt: 'Implement the plan using subagent-driven development.',
    position: { x: 750, y: 0 },
  },
  {
    id: 'step-review',
    skill: 'requesting-code-review',
    label: 'Code Review',
    agent: 'claude',
    prompt: 'Request a code review of the implementation.',
    position: { x: 1000, y: 0 },
  },
  {
    id: 'step-finish',
    skill: 'finishing-a-development-branch',
    label: 'Finish Branch',
    agent: 'claude',
    prompt: 'Finish the development branch and prepare for merge.',
    position: { x: 1250, y: 0 },
  },
];

const DEFAULT_EDGES: PipelineEdge[] = [
  { source: 'step-brainstorm', target: 'step-worktree' },
  { source: 'step-worktree', target: 'step-plan' },
  { source: 'step-plan', target: 'step-implement' },
  { source: 'step-implement', target: 'step-review' },
  { source: 'step-review', target: 'step-finish' },
];

export class PipelineService {
  constructor(private db: Db) {}

  list(): Pipeline[] {
    const rows = this.db.select().from(pipelines).all();
    return rows.map(rowToPipeline);
  }

  get(id: string): Pipeline | null {
    const row = this.db.select().from(pipelines).where(eq(pipelines.id, id)).get();
    return row ? rowToPipeline(row) : null;
  }

  create(input: CreatePipelineInput): Pipeline {
    const now = Date.now();
    const id = nanoid(12);

    const row = this.db
      .insert(pipelines)
      .values({
        id,
        name: input.name,
        description: input.description ?? '',
        steps: JSON.stringify(input.steps),
        edges: JSON.stringify(input.edges),
        isDefault: input.isDefault ? 1 : 0,
        createdAt: now,
        updatedAt: now,
      })
      .returning()
      .get();

    return rowToPipeline(row);
  }

  update(id: string, input: UpdatePipelineInput): Pipeline {
    const existing = this.get(id);
    if (!existing) {
      throw new Error(`Pipeline not found: ${id}`);
    }

    const now = Date.now();
    const setValues: Record<string, unknown> = { updatedAt: now };

    if (input.name !== undefined) setValues.name = input.name;
    if (input.description !== undefined) setValues.description = input.description;
    if (input.steps !== undefined) setValues.steps = JSON.stringify(input.steps);
    if (input.edges !== undefined) setValues.edges = JSON.stringify(input.edges);
    if (input.isDefault !== undefined) setValues.isDefault = input.isDefault ? 1 : 0;

    this.db.update(pipelines).set(setValues).where(eq(pipelines.id, id)).run();

    const result = this.get(id);
    if (!result) throw new Error(`Pipeline ${id} not found after update`);
    return result;
  }

  delete(id: string): void {
    this.db.delete(pipelines).where(eq(pipelines.id, id)).run();
  }

  getDefault(): Pipeline | null {
    const row = this.db.select().from(pipelines).where(eq(pipelines.isDefault, 1)).get();
    return row ? rowToPipeline(row) : null;
  }

  seedDefault(): Pipeline {
    const existing = this.getDefault();
    if (existing) return existing;

    return this.create({
      name: 'Superpowers Workflow',
      description:
        'Default workflow based on the superpowers skill set: brainstorm, worktree, plan, implement, review, finish.',
      steps: DEFAULT_STEPS,
      edges: DEFAULT_EDGES,
      isDefault: true,
    });
  }

  getNextStep(pipelineId: string, currentStepIndex: number): PipelineStep | null {
    const pipeline = this.get(pipelineId);
    if (!pipeline) return null;

    const currentStep = pipeline.steps[currentStepIndex];
    if (!currentStep) return null;

    // Follow edges from the current step
    const edge = pipeline.edges.find((e) => e.source === currentStep.id);
    if (!edge) return null;

    return pipeline.steps.find((s) => s.id === edge.target) ?? null;
  }
}
