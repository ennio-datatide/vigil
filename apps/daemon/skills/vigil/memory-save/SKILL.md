---
name: memory-save
description: Save a new memory for a project. Use when the user asks you to remember something, or when you learn an important fact, preference, lesson, or decision about a project. Memories persist across sessions and are searchable.
---

# Memory Save

Saves a new memory for a project via the Vigil daemon API.

## Usage

```bash
./scripts/save.sh --project-path <path> --content <text> --type <type>
```

## Arguments

- `--project-path <path>` (required) — Absolute path to the project directory
- `--content <text>` (required) — The memory content to persist
- `--type <type>` (required) — Type of memory (e.g. "lesson", "fact", "preference", "decision")

## Output

Returns JSON with the saved memory's ID and details.
