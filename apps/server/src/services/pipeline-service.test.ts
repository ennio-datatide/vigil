import type { PipelineEdge, PipelineStep } from '@praefectus/shared';
import { beforeEach, describe, expect, it } from 'vitest';
import { createDb, type Db, initializeSchema } from '../db/client.js';
import { PipelineService } from './pipeline-service.js';

function makeTestDb(): Db {
  const { sqlite, db } = createDb(':memory:');
  initializeSchema(sqlite);
  return db;
}

function makeStep(overrides: Partial<PipelineStep> = {}): PipelineStep {
  return {
    id: 'step-1',
    skill: 'brainstorming',
    label: 'Brainstorm',
    agent: 'claude',
    prompt: 'Run brainstorming',
    position: { x: 0, y: 0 },
    ...overrides,
  };
}

describe('PipelineService', () => {
  let db: Db;
  let service: PipelineService;

  beforeEach(() => {
    db = makeTestDb();
    service = new PipelineService(db);
  });

  it('list() returns empty array initially', () => {
    expect(service.list()).toEqual([]);
  });

  it('create() stores and returns pipeline', () => {
    const steps: PipelineStep[] = [makeStep()];
    const edges: PipelineEdge[] = [];

    const pipeline = service.create({
      name: 'Test Pipeline',
      steps,
      edges,
    });

    expect(pipeline.id).toBeTruthy();
    expect(pipeline.name).toBe('Test Pipeline');
    expect(pipeline.description).toBe('');
    expect(pipeline.steps).toEqual(steps);
    expect(pipeline.edges).toEqual(edges);
    expect(pipeline.isDefault).toBe(false);
    expect(pipeline.createdAt).toBeGreaterThan(0);
    expect(pipeline.updatedAt).toBeGreaterThan(0);
  });

  it('get(id) retrieves pipeline with parsed steps/edges', () => {
    const steps: PipelineStep[] = [
      makeStep({ id: 's1', skill: 'brainstorming' }),
      makeStep({ id: 's2', skill: 'writing-plans', position: { x: 200, y: 0 } }),
    ];
    const edges: PipelineEdge[] = [{ source: 's1', target: 's2' }];

    const created = service.create({ name: 'My Pipeline', steps, edges });
    const fetched = service.get(created.id);

    expect(fetched).not.toBeNull();
    expect(fetched?.steps).toEqual(steps);
    expect(fetched?.edges).toEqual(edges);
    expect(fetched?.name).toBe('My Pipeline');
  });

  it('get() returns null for nonexistent id', () => {
    expect(service.get('nope')).toBeNull();
  });

  it('update(id, data) updates and bumps updatedAt', async () => {
    const pipeline = service.create({
      name: 'Original',
      steps: [makeStep()],
      edges: [],
    });

    // Small delay to ensure timestamp difference
    await new Promise((r) => setTimeout(r, 10));

    const updated = service.update(pipeline.id, { name: 'Renamed' });

    expect(updated.name).toBe('Renamed');
    expect(updated.updatedAt).toBeGreaterThanOrEqual(pipeline.updatedAt);
    expect(updated.steps).toEqual(pipeline.steps); // unchanged
  });

  it('update() throws for nonexistent id', () => {
    expect(() => service.update('nope', { name: 'x' })).toThrow();
  });

  it('delete(id) removes pipeline', () => {
    const pipeline = service.create({
      name: 'To Delete',
      steps: [makeStep()],
      edges: [],
    });

    service.delete(pipeline.id);
    expect(service.get(pipeline.id)).toBeNull();
    expect(service.list()).toHaveLength(0);
  });

  it('getDefault() returns the default pipeline', () => {
    service.create({ name: 'Not Default', steps: [makeStep()], edges: [] });
    service.create({
      name: 'Default One',
      steps: [makeStep()],
      edges: [],
      isDefault: true,
    });

    const def = service.getDefault();
    expect(def).not.toBeNull();
    expect(def?.name).toBe('Default One');
    expect(def?.isDefault).toBe(true);
  });

  it('getDefault() returns null when no default exists', () => {
    service.create({ name: 'Not Default', steps: [makeStep()], edges: [] });
    expect(service.getDefault()).toBeNull();
  });

  it('seedDefault() creates default pipeline if none exists', () => {
    const pipeline = service.seedDefault();

    expect(pipeline.name).toBe('Superpowers Workflow');
    expect(pipeline.isDefault).toBe(true);
    expect(pipeline.steps.length).toBe(6);
    expect(pipeline.edges.length).toBe(5); // 6 steps, 5 edges connecting them

    // Verify skills match superpowers order
    const skills = pipeline.steps.map((s) => s.skill);
    expect(skills).toEqual([
      'brainstorming',
      'using-git-worktrees',
      'writing-plans',
      'subagent-driven-development',
      'requesting-code-review',
      'finishing-a-development-branch',
    ]);
  });

  it('seedDefault() returns existing default if one exists', () => {
    const first = service.seedDefault();
    const second = service.seedDefault();

    expect(first.id).toBe(second.id);
    expect(service.list()).toHaveLength(1);
  });

  it('getNextStep() returns next step following edges', () => {
    const steps: PipelineStep[] = [
      makeStep({ id: 's1', skill: 'brainstorming', position: { x: 0, y: 0 } }),
      makeStep({ id: 's2', skill: 'writing-plans', position: { x: 200, y: 0 } }),
      makeStep({ id: 's3', skill: 'implementing', position: { x: 400, y: 0 } }),
    ];
    const edges: PipelineEdge[] = [
      { source: 's1', target: 's2' },
      { source: 's2', target: 's3' },
    ];

    const pipeline = service.create({ name: 'Chain', steps, edges });

    const next1 = service.getNextStep(pipeline.id, 0);
    expect(next1).not.toBeNull();
    expect(next1?.id).toBe('s2');

    const next2 = service.getNextStep(pipeline.id, 1);
    expect(next2).not.toBeNull();
    expect(next2?.id).toBe('s3');
  });

  it('getNextStep() returns null for last step', () => {
    const steps: PipelineStep[] = [
      makeStep({ id: 's1', skill: 'brainstorming', position: { x: 0, y: 0 } }),
      makeStep({ id: 's2', skill: 'writing-plans', position: { x: 200, y: 0 } }),
    ];
    const edges: PipelineEdge[] = [{ source: 's1', target: 's2' }];

    const pipeline = service.create({ name: 'Short', steps, edges });

    expect(service.getNextStep(pipeline.id, 1)).toBeNull();
  });

  it('getNextStep() returns null for nonexistent pipeline', () => {
    expect(service.getNextStep('nope', 0)).toBeNull();
  });

  it('list() returns all pipelines', () => {
    service.create({ name: 'A', steps: [makeStep()], edges: [] });
    service.create({ name: 'B', steps: [makeStep()], edges: [] });

    const all = service.list();
    expect(all).toHaveLength(2);
    expect(all.map((p) => p.name).sort()).toEqual(['A', 'B']);
  });
});
