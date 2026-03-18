# Agent Pattern Skills & Vigil Orchestration Enhancements

**Date:** 2026-03-18
**Status:** Proposed

## Problem

Vigil will be used heavily to build AI solutions. Workers currently have no guidance on agentic design patterns — they rely on the LLM's training knowledge. The agent frameworks research document (`docs/agent frameworks`) identifies 11 proven patterns from 14 repos, but this knowledge isn't available as structured, activatable Skills.

Additionally, Vigil's own orchestration is limited to sequential worker spawning. It lacks parallelization (spawn multiple workers for independent subtasks) and evaluator-optimizer (iterative quality refinement).

## Goal

1. Create 11 pattern Skills that teach workers how to build agentic AI systems
2. Create 2 new Vigil orchestration Skills (parallel-workers, evaluate-and-improve)
3. Reorganize skills directory: `skills/vigil/` for Vigil's own Skills, `skills/patterns/` for worker Skills
4. Auto-install pattern Skills into every worker's worktree
5. Update vigil-core with routing logic for the new orchestration patterns

## Design

### 1. Pattern Skills for Workers (11 Skills)

Instructional Skills — no scripts, just SKILL.md with optional references. Each teaches one agentic pattern with working code examples.

| Skill | Pattern | Description |
|-------|---------|-------------|
| `agent-loop` | Agent Loop | The fundamental pattern: LLM + tools + while loop. 50-100 lines. Covers stop conditions, tool dispatch, context management. |
| `tool-calling` | Tool Use | Defining JSON schemas, function execution, result passing. Works with OpenAI, Anthropic, and local LLMs. |
| `prompt-chaining` | Prompt Chaining | Sequential LLM calls where each uses prior output. String interpolation, validation gates, error handling. |
| `react-agent` | ReAct | Reason + Act + Observe cycle. Structured thinking with explicit reasoning steps before actions. |
| `orchestrator-workers` | Orchestrator-Workers | Central LLM delegating subtasks to specialized worker LLMs. Task decomposition, result synthesis. |
| `evaluator-optimizer` | Evaluator-Optimizer | Generate → evaluate → improve → repeat. Quality thresholds, iteration limits, structured feedback. |
| `parallelization` | Parallelization | Simultaneous LLM calls with result aggregation. Fan-out/fan-in, voting, sectioning patterns. |
| `multi-agent-handoffs` | Handoffs | Swarm-style agent switching via function returns. Conversation routing, context transfer. |
| `multi-agent-debate` | Debate | Multiple agents argue, share reasoning, converge. Round-based with judge or consensus. |
| `dag-workflows` | DAG Execution | Directed graph of nodes with prep/exec/post lifecycle. Shared state store. PocketFlow-style. |
| `unix-pipe-agents` | Pipe Composition | Prompts as files, compose via Unix pipes. Fabric-style patterns directory. |

**SKILL.md structure for each:**

```markdown
---
name: <pattern-name>
description: <when to use this pattern — specific enough for Claude Code to match>
---

# <Pattern Name>

## When to Use
- [Specific scenarios]
- [NOT for these cases — use X instead]

## The Pattern (Minimal Implementation)
[Complete working code, 50-100 lines, one file]

## Example
[Concrete example with real-world use case]

## Common Pitfalls
[What goes wrong and how to avoid it]

## Key Insight
[The one sentence takeaway from the source repo]
```

**Language:** All code examples use **Python** as the primary language (most universal for AI/ML work, all source repos use Python). Each Skill includes a note that the pattern applies to any language.

**Source mapping** — each Skill draws from specific repos:

| Skill | Primary Source |
|-------|---------------|
| `agent-loop` | anthropic-cookbook patterns/agents, claude-quickstarts loop.py |
| `tool-calling` | openai-cookbook function calling notebooks |
| `prompt-chaining` | MinimalChainable, anthropic-cookbook |
| `react-agent` | minimal-agent, ai-agents-from-scratch |
| `orchestrator-workers` | anthropic-cookbook, swarm |
| `evaluator-optimizer` | anthropic-cookbook |
| `parallelization` | anthropic-cookbook |
| `multi-agent-handoffs` | openai/swarm |
| `multi-agent-debate` | llm_multiagent_debate |
| `dag-workflows` | PocketFlow |
| `unix-pipe-agents` | fabric |

### 2. Vigil Orchestration Enhancements (2 New Skills)

#### parallel-workers Skill

A Vigil-only Skill with a script that spawns multiple workers concurrently.

```
skills/vigil/parallel-workers/
├── SKILL.md
└── scripts/
    └── parallel.sh
```

**SKILL.md description:** "Spawn multiple worker sessions in parallel for independent subtasks. Use when a task has 2+ parts that don't depend on each other. Returns aggregated results from all workers."

**parallel.sh behavior:**
1. Accepts `--project-path` and multiple `--task "prompt"` args
2. POSTs to `/api/sessions` for each task concurrently (background curl jobs)
3. Collects all session IDs
4. Polls all sessions every 3 seconds until all reach terminal state
5. For each completed session, GETs `/api/sessions/:id` which includes the `output` field (populated by the get_session handler from OutputManager)
6. Returns aggregated JSON: `{"results": [{"session_id":"...","status":"...","output":"..."}, ...]}`
7. If any worker hits `needs_input`, returns immediately with that worker's info so Vigil can ask the user. Other workers continue running in the background — Vigil can check them later via session-recall.

NOTE: The `GET /api/sessions/:id` endpoint already includes an `output` field populated from `OutputManager::get_buffer()` or `read_log()` (see `api/sessions.rs` get_session handler). This is the same field used by the existing spawn-worker script.

#### evaluate-and-improve Skill

A Vigil-only Skill with a script that implements the evaluator-optimizer loop.

```
skills/vigil/evaluate-and-improve/
├── SKILL.md
└── scripts/
    └── evaluate.sh
```

**SKILL.md description:** "Evaluate a worker's output and optionally iterate for improvement. Use for quality-critical tasks like code generation, writing, or research. Spawns an evaluator worker, checks quality, and if needed spawns a refinement worker."

**evaluate.sh behavior:**
1. Accepts `--project-path`, `--session-id` (completed worker), `--criteria` (what to evaluate)
2. GETs the worker's output from `/api/sessions/:id` (the `output` field from OutputManager)
3. Spawns an evaluator worker via `POST /api/sessions` with prompt: "Evaluate this output against these criteria: [criteria]. Output JSON: {quality: 1-10, issues: [...], suggestions: [...]}"
4. Polls evaluator until complete, parses the quality JSON from output
5. If quality < 7: spawns a refinement worker with the original output + evaluator feedback
6. Returns final output JSON with quality score

NOTE: Both parallel.sh and evaluate.sh use the same `GET /api/sessions/:id` response format with the `output` field, consistent with spawn.sh.

### 3. Directory Reorganization

Move existing Skills from `skills/` to `skills/vigil/`:

```
apps/daemon/skills/
├── vigil/                        # Vigil's own Skills
│   ├── vigil-core/               # (moved from skills/vigil-core/)
│   ├── spawn-worker/             # (moved from skills/spawn-worker/)
│   ├── reply-to-worker/          # (moved)
│   ├── parallel-workers/         # NEW
│   ├── evaluate-and-improve/     # NEW
│   ├── memory-recall/            # (moved)
│   ├── memory-save/              # (moved)
│   ├── memory-delete/            # (moved)
│   ├── session-recall/           # (moved)
│   ├── acta-update/              # (moved)
│   └── execute-pipeline/         # (moved)
└── patterns/                     # Worker pattern Skills (ALL NEW)
    ├── agent-loop/
    ├── tool-calling/
    ├── prompt-chaining/
    ├── react-agent/
    ├── orchestrator-workers/
    ├── evaluator-optimizer/
    ├── parallelization/
    ├── multi-agent-handoffs/
    ├── multi-agent-debate/
    ├── dag-workflows/
    └── unix-pipe-agents/
```

### 4. Installation Changes

**vigil_manager.rs `spawn_vigil()`:**
- Currently copies from `skills/` → change to copy from `skills/vigil/`
- No change to the copy logic, just the source path

**agent_spawner.rs `spawn_interactive()`:**
- Add new step: copy `skills/patterns/` into the worktree's `.claude/skills/`
- The `copy_dir_recursive` helper currently lives as a private function in `vigil_manager.rs`. Move it to a shared utility (e.g., `src/utils.rs` or make it `pub(crate)`) so both `vigil_manager.rs` and `agent_spawner.rs` can use it.
- Git add + commit the skills so Claude Code discovers them

**vigil_manager.rs `spawn_vigil()`:**
- Update the source path from `skills/` to `skills/vigil/` in the `copy_dir_recursive` call. The source path uses `env!("CARGO_MANIFEST_DIR")` at compile time — verify this resolves correctly after the directory move.

### 5. vigil-core Updates

Add routing instructions to `vigil-core/SKILL.md`:

```markdown
## Routing Decision Tree

When receiving a user request, choose the best approach:

1. **Independent subtasks** (e.g., "research X AND build Y", "check weather in 3 cities")
   → Use the `parallel-workers` skill

2. **Quality-critical output** (e.g., "write production code", "draft a proposal", "design an API")
   → Use `spawn-worker` first, then `evaluate-and-improve` on the result

3. **Multi-step development** (e.g., "implement this feature", "refactor the auth module")
   → Use `execute-pipeline`

4. **Everything else** (questions, lookups, simple tasks)
   → Use `spawn-worker`
```

## Testing Strategy

**Pattern Skills:** No automated tests — these are instructional documents. Verify by reading each SKILL.md for completeness and accuracy.

**parallel-workers:** Test with 3 concurrent trivial tasks ("what is 1+1", "what is 2+2", "what is 3+3"). All should complete and return aggregated results.

**evaluate-and-improve:** Test by asking for code, then evaluating it. The evaluator should produce a quality score and the loop should iterate if needed.

**Integration:** Send "research the weather in 3 cities simultaneously" to Vigil and verify it uses parallel-workers.

## Non-Goals

- Publishing Skills to skills.sh marketplace (future)
- Skill versioning or dependency management
- Worker-to-worker communication (workers are independent)
- Custom LLM provider configuration in pattern Skills (patterns are provider-agnostic in the examples, workers use Claude Code's default model)
