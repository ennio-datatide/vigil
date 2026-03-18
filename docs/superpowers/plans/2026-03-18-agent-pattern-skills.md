# Agent Pattern Skills — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create 11 pattern Skills for workers, 2 Vigil orchestration Skills, reorganize skills directory, and wire pattern Skills into worker spawning.

**Architecture:** Pattern Skills are pure SKILL.md instruction documents (no scripts) with Python examples. Vigil Skills have scripts. Existing skills move from `skills/` to `skills/vigil/`. `copy_dir_recursive` becomes shared. Workers get patterns auto-installed.

**Tech Stack:** Agent Skills spec (SKILL.md), bash scripts, Rust

**Spec:** `docs/superpowers/specs/2026-03-18-agent-pattern-skills-design.md`

---

## Group A: 11 Pattern Skills (all parallelizable — pure markdown)

### Task 1: agent-loop + tool-calling + prompt-chaining Skills

Create 3 foundational pattern Skills in `apps/daemon/skills/patterns/`:

**agent-loop/SKILL.md** — The fundamental LLM + tools + loop pattern. Include: minimal Python agent loop (~60 lines) using the Anthropic SDK, tool dispatch dict, stop condition checking, context window management. Source: anthropic-cookbook patterns/agents, claude-quickstarts loop.py.

**tool-calling/SKILL.md** — JSON schema definition, function execution, result passing. Include: defining tools as dicts, parsing tool_use blocks, executing locally, appending results. Works with any LLM API. Source: openai-cookbook function calling.

**prompt-chaining/SKILL.md** — Sequential LLM calls with output interpolation. Include: MinimalChainable pattern (~50 lines), validation gates between steps, error handling. Source: MinimalChainable, anthropic-cookbook.

Each SKILL.md follows the template: When to Use, The Pattern (complete code), Example, Common Pitfalls, Key Insight.

Commit: `feat: add agent-loop, tool-calling, prompt-chaining pattern Skills`

### Task 2: react-agent + orchestrator-workers + evaluator-optimizer Skills

**react-agent/SKILL.md** — Reason + Act + Observe cycle. Include: structured thinking prompt, action parsing, observation loop. Source: minimal-agent, ai-agents-from-scratch.

**orchestrator-workers/SKILL.md** — Central LLM delegating to specialized workers. Include: task decomposition prompt, worker dispatch, result synthesis. Source: anthropic-cookbook, swarm.

**evaluator-optimizer/SKILL.md** — Generate → evaluate → improve loop. Include: evaluation criteria prompt, quality scoring, feedback incorporation, iteration limits. Source: anthropic-cookbook.

Commit: `feat: add react-agent, orchestrator-workers, evaluator-optimizer pattern Skills`

### Task 3: parallelization + multi-agent-handoffs + multi-agent-debate Skills

**parallelization/SKILL.md** — Simultaneous LLM calls. Include: asyncio.gather pattern, fan-out/fan-in, voting/aggregation strategies. Source: anthropic-cookbook.

**multi-agent-handoffs/SKILL.md** — Swarm-style agent switching. Include: Agent class with instructions+functions, handoff via function return, context transfer. Source: openai/swarm.

**multi-agent-debate/SKILL.md** — Multi-round debate for consensus. Include: 3-agent debate loop, response sharing, refinement rounds. Source: llm_multiagent_debate.

Commit: `feat: add parallelization, multi-agent-handoffs, multi-agent-debate pattern Skills`

### Task 4: dag-workflows + unix-pipe-agents Skills

**dag-workflows/SKILL.md** — Directed graph with shared state. Include: Node class with prep/exec/post, edge routing, shared store. PocketFlow's 100-line pattern. Source: PocketFlow.

**unix-pipe-agents/SKILL.md** — Prompts as files, compose via pipes. Include: pattern directory structure, pipe composition examples, helper tools. Source: fabric.

Commit: `feat: add dag-workflows, unix-pipe-agents pattern Skills`

---

## Group B: 2 Vigil Orchestration Skills (parallelizable with A)

### Task 5: parallel-workers Skill + script

Create `apps/daemon/skills/vigil/parallel-workers/SKILL.md` and `scripts/parallel.sh`.

**SKILL.md:** Description for routing — "Spawn multiple worker sessions in parallel for independent subtasks." Instructions for when to use, how to interpret aggregated results, needs_input handling.

**parallel.sh:** Accepts `--project-path` and multiple `--task "prompt"` args. Spawns all via concurrent background curl POSTs to `/api/sessions`. Polls all session IDs every 3s. Returns aggregated JSON. If any hits needs_input, returns immediately with that worker's info.

Commit: `feat: add parallel-workers Vigil Skill`

### Task 6: evaluate-and-improve Skill + script

Create `apps/daemon/skills/vigil/evaluate-and-improve/SKILL.md` and `scripts/evaluate.sh`.

**SKILL.md:** Description — "Evaluate a worker's output and iterate for improvement." Instructions for when to use (quality-critical tasks), quality threshold (< 7 triggers refinement).

**evaluate.sh:** Accepts `--project-path`, `--session-id`, `--criteria`. GETs worker output, spawns evaluator worker, parses quality score, optionally spawns refinement worker. Returns final output with quality score.

Commit: `feat: add evaluate-and-improve Vigil Skill`

---

## Group C: Wiring (depends on A + B completing)

### Task 7: Reorganize skills directory + share copy_dir_recursive

1. Move existing skills: `skills/*` → `skills/vigil/*` (git mv)
2. Move `copy_dir_recursive` from `vigil_manager.rs` to a shared location (new `src/utils.rs` or inline in both files)
3. Update `vigil_manager.rs` source path: `skills/` → `skills/vigil/`
4. Update `agent_spawner.rs`: add pattern Skills installation in `spawn_interactive()` — copy `skills/patterns/` into worktree `.claude/skills/`
5. Update `vigil-core/SKILL.md` with routing decision tree (parallel-workers, evaluate-and-improve, execute-pipeline, spawn-worker)

Build: `cargo build && cargo clippy -- -D warnings`

Commit: `feat: reorganize skills directory and install patterns in workers`

### Task 8: Build, verify, push

1. `cargo build` + `cargo test` + `cargo clippy -- -D warnings`
2. `npx biome check --write .` + `npm run build`
3. Verify all 22 Skills exist (11 patterns + 11 vigil)
4. Commit any fixes
