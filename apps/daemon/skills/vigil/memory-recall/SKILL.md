---
name: memory-recall
description: Search project memories by semantic similarity. Use when you need to recall previously saved information about a project — lessons learned, preferences, facts, decisions. Returns the most relevant memories matching the query.
---

# Memory Recall

Searches project memories by semantic similarity via the Vigil daemon API.

## Usage

```bash
./scripts/recall.sh --project-path <path> --query <text> [--limit <n>]
```

## Arguments

- `--project-path <path>` (required) — Absolute path to the project directory
- `--query <text>` (required) — Natural-language search query
- `--limit <n>` — Maximum number of results (default: server-side default)

## Output

Returns a JSON array of matching memories with content, type, and relevance score.
