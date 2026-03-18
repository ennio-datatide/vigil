---
name: evaluate-and-improve
description: Evaluate a worker's output and iterate for improvement. Use for quality-critical tasks like code generation, writing, research, or API design. Spawns an evaluator, checks quality score, and if below threshold spawns a refinement worker.
---

# Evaluate and Improve

Implements the evaluator-optimizer pattern: evaluate a completed worker's output against specific criteria, and if quality is insufficient, automatically spawn a refinement worker to improve it.

## Usage

```bash
./scripts/evaluate.sh --project-path <path> --session-id <id> --criteria <text>
```

## Arguments

- `--project-path <path>` (required) -- Absolute path to the project directory
- `--session-id <id>` (required) -- Session ID of the completed worker to evaluate
- `--criteria <text>` (required) -- What to evaluate (e.g., "code correctness, test coverage, readability")

## When to Use

- Quality-critical output: production code, technical writing, API designs
- User explicitly asks for high quality or review
- After spawn-worker completes a complex task worth validating
- When you want a second opinion on worker output

## When NOT to Use

- Trivial tasks (jokes, simple lookups, quick math)
- Tasks where speed matters more than quality
- Already-evaluated output (avoid evaluation loops)

## How It Works

1. Fetches the original worker's output from `/api/sessions/:id`
2. Spawns an **evaluator worker** that scores the output (1-10) against your criteria
3. If quality score >= 7: returns the original output as-is with the evaluation
4. If quality score < 7: spawns a **refinement worker** with the original output + evaluator feedback
5. Returns the final output with quality metadata

## Output

### High quality (no refinement needed)

```json
{
  "session_id": "original-id",
  "status": "completed",
  "output": "...",
  "evaluation": {
    "quality": 8,
    "issues": [],
    "suggestions": ["Minor: consider adding examples"]
  },
  "refined": false
}
```

### Refined output

```json
{
  "session_id": "refinement-id",
  "original_session_id": "original-id",
  "status": "completed",
  "output": "...",
  "evaluation": {
    "quality": 5,
    "issues": ["Missing error handling", "No tests"],
    "suggestions": ["Add try/catch blocks", "Add unit tests"]
  },
  "refined": true
}
```

## The needs_input Flow

If the evaluator or refinement worker hits `needs_input`:
1. Read the `question` field
2. Ask the user the question in the chat
3. Use the reply-to-worker skill with the session_id and the user's answer

## Exit Codes

- `0` -- All business states (completed, failed, needs_input, timeout)
- `1` -- Script errors (missing args, daemon unreachable, parse failures)
