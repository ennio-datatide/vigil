---
name: acta-update
description: Update the project acta (briefing document) with new content. The acta summarizes project context, architecture decisions, and key information for future sessions. Use when significant project decisions are made or context should be preserved.
---

# Acta Update

Updates the project acta (briefing document) via the Vigil daemon API.

## Usage

```bash
./scripts/update.sh --project-path <path> --content <text>
```

## Arguments

- `--project-path <path>` (required) — Absolute path to the project directory
- `--content <text>` (required) — Updated acta content

## Output

Returns JSON confirmation of the update.
