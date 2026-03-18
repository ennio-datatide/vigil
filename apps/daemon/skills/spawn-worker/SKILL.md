---
name: spawn-worker
description: Spawn a Claude Code worker session to handle a user request. Use for ALL user tasks — jokes, questions, code, research, commands, web searches. Use --wait for quick tasks (<30s), omit for long-running tasks. If the output JSON has status "needs_input", ask the user the question and use the reply-to-worker skill.
---

# Spawn Worker

Spawns a new Claude Code worker session via the Vigil daemon API.

## Usage

```bash
./scripts/spawn.sh --project-path <path> --prompt <text> [--wait] [--skill <name>]
```

## Arguments

- `--project-path <path>` (required) — Absolute path to the project directory
- `--prompt <text>` (required) — Task instructions for the worker
- `--wait` — Block until the worker completes (default: return immediately)
- `--skill <name>` — Optional skill to assign to the worker

## Behavior

### Without --wait (default)
Returns immediately with the session ID:
```json
{"session_id":"abc123","status":"queued"}
```

### With --wait
Polls every 3 seconds until the worker reaches a terminal state. Output:

| Status | JSON |
|--------|------|
| Completed | `{"session_id":"...","status":"completed","output":"..."}` |
| Failed | `{"session_id":"...","status":"failed","output":"...","error":"..."}` |
| Cancelled | `{"session_id":"...","status":"cancelled","output":"..."}` |
| Interrupted | `{"session_id":"...","status":"interrupted","output":"..."}` |
| Needs input | `{"session_id":"...","status":"needs_input","question":"..."}` |
| Timeout | `{"session_id":"...","status":"timeout","message":"Worker still running after 600s. Use session-recall to check status."}` |

## The needs_input Flow

When the output JSON has `"status":"needs_input"`:
1. Read the `question` field — this is what the worker is asking
2. Ask the user the question in the chat
3. Wait for the user's answer
4. Use the reply-to-worker skill with the session_id and the user's answer

## When to Use --wait

- **Use --wait** for trivial tasks: jokes, simple math, quick lookups, single commands
- **Omit --wait** for anything that might take >30 seconds: research, code changes, web searches, refactoring

When in doubt, omit --wait. A quick "I'm on it" is better than silence.
