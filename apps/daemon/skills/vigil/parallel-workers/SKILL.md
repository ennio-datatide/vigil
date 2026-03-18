---
name: parallel-workers
description: Spawn multiple worker sessions in parallel for independent subtasks. Use when a task has 2+ parts that don't depend on each other (e.g., "research X AND build Y", "check weather in 3 cities"). Returns aggregated results from all workers.
---

# Parallel Workers

Spawns multiple Claude Code worker sessions concurrently and waits for all to complete. Each worker runs independently in its own worktree.

## Usage

```bash
./scripts/parallel.sh --project-path <path> --task "first task" --task "second task" [--task "third task" ...]
```

## Arguments

- `--project-path <path>` (required) -- Absolute path to the project directory
- `--task <text>` (required, repeatable) -- Task prompt for a worker. Specify multiple times for multiple workers.

## When to Use

- The user's request has 2+ independent parts: "research X AND build Y"
- Batch operations: "check weather in 3 cities", "summarize these 5 articles"
- Tasks where one subtask doesn't need the output of another

## When NOT to Use

- Tasks with sequential dependencies (use spawn-worker or execute-pipeline instead)
- Single tasks (use spawn-worker instead)
- Tasks requiring iteration/quality refinement (use evaluate-and-improve instead)

## Output

Returns aggregated JSON with results from all workers:

```json
{
  "results": [
    {"session_id": "abc", "status": "completed", "output": "..."},
    {"session_id": "def", "status": "completed", "output": "..."},
    {"session_id": "ghi", "status": "failed", "output": "...", "error": "..."}
  ]
}
```

## The needs_input Flow

If ANY worker hits `needs_input`, the script returns immediately with that worker's info so you can ask the user. The other workers continue running in the background -- use session-recall to check their status later.

When a result has `"status": "needs_input"`:
1. Read the `question` field
2. Ask the user the question in the chat
3. Use the reply-to-worker skill with the session_id and the user's answer
4. Use session-recall to check status of all workers once resolved

## Exit Codes

- `0` -- All business states (completed, failed, needs_input, timeout)
- `1` -- Script errors (missing args, daemon unreachable, parse failures)
