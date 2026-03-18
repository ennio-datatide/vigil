---
name: reply-to-worker
description: Send a reply to a worker that needs user input, then wait for it to complete. Use this after spawn-worker returns status "needs_input". Pass the session_id from the spawn-worker output and the user's answer. If the worker asks another question, it returns needs_input again — repeat the cycle.
---

# Reply to Worker

Sends a message to a worker session that is waiting for user input, then polls until the worker completes or asks another question.

## Usage

```bash
./scripts/reply.sh --session-id <id> --message <text>
```

## Arguments

- `--session-id <id>` (required) — Session ID of the worker (from spawn-worker output)
- `--message <text>` (required) — The user's answer to relay to the worker

## Behavior

1. POSTs the message to the worker's input endpoint
2. Polls every 3 seconds until the worker reaches a terminal state

Output format is identical to spawn-worker with `--wait`:

| Status | JSON |
|--------|------|
| Completed | `{"session_id":"...","status":"completed","output":"..."}` |
| Failed | `{"session_id":"...","status":"failed","output":"...","error":"..."}` |
| Cancelled | `{"session_id":"...","status":"cancelled","output":"..."}` |
| Interrupted | `{"session_id":"...","status":"interrupted","output":"..."}` |
| Needs input | `{"session_id":"...","status":"needs_input","question":"..."}` |
| Timeout | `{"session_id":"...","status":"timeout","message":"Worker still running after 600s. Use session-recall to check status."}` |

## The Multi-Turn Flow

If the output has `"status":"needs_input"` again, the worker is asking another question. Read the `question` field, ask the user, and run reply-to-worker again with the same session_id and the new answer. Repeat until the worker completes.
