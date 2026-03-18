---
name: session-recall
description: Check the status of worker sessions. Use to get details on a specific session by ID, or list all sessions optionally filtered by project path. Use this to check on long-running workers or verify completed work.
---

# Session Recall

Retrieves session information from the Vigil daemon API.

## Usage

```bash
# Get a specific session
./scripts/recall.sh --session-id <id>

# List all sessions
./scripts/recall.sh

# List sessions for a project
./scripts/recall.sh --project-path <path>
```

## Arguments

- `--session-id <id>` — Get a specific session by ID
- `--project-path <path>` — Filter sessions by project path (when listing)

## Output

Returns JSON with session details including status, output, project path, and timestamps.
