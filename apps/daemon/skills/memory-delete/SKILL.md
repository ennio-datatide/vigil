---
name: memory-delete
description: Delete a specific memory by its ID. Use when the user asks to forget something or when a previously saved memory is no longer accurate or relevant.
---

# Memory Delete

Deletes a memory by ID via the Vigil daemon API.

## Usage

```bash
./scripts/delete.sh --memory-id <id>
```

## Arguments

- `--memory-id <id>` (required) — ID of the memory to delete

## Output

Returns confirmation that the memory was deleted.
