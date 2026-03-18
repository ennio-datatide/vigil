---
name: execute-pipeline
description: Execute a multi-step development pipeline (brainstorm -> design -> code -> review) for coding tasks. Non-blocking — starts the pipeline and returns immediately. Use for writing code, implementing features, refactoring, designing systems, or any multi-step development workflow.
---

# Execute Pipeline

Starts a multi-step development pipeline via the Vigil daemon API.

## Usage

```bash
./scripts/execute.sh --project-path <path> --prompt <text> [--pipeline-id <id>]
```

## Arguments

- `--project-path <path>` (required) — Absolute path to the project directory
- `--prompt <text>` (required) — The user's request / instructions for the pipeline
- `--pipeline-id <id>` — Pipeline ID to execute (default: uses the default pipeline)

## Behavior

Non-blocking. Returns immediately with the execution ID. The user can watch progress in the session monitor.

## Output

Returns JSON with the pipeline execution ID:
```json
{"execution_id":"...","status":"started"}
```
