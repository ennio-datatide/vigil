# Vigil Skills Architecture & Rebrand

**Date:** 2026-03-18
**Status:** Proposed

## Problem

Praefectus currently uses MCP tools (`pf mcp-serve` subprocess) to give Vigil its capabilities — `spawn_worker`, `memory_recall`, `session_recall`, etc. This requires a custom MCP server implementation (`mcp.rs`), MCP config files, the `--tools ""` flag to disable built-in tools, and the `rmcp` crate dependency. The strategy prompt is a hardcoded markdown file embedded via `--append-system-prompt-file`.

This is inflexible: adding new capabilities requires modifying Rust code, recompiling, and redeploying. The Agent Skills specification (agentskills.io) provides a standard, open format for giving agents capabilities via markdown instruction documents with bundled scripts — no custom server needed.

Additionally, "Praefectus" is being rebranded to "Vigil" for a cleaner identity.

## Goal

1. Replace all MCP tools with Agent Skills (SKILL.md + scripts that curl the daemon REST API)
2. Delete the MCP server entirely
3. Rebrand Praefectus → Vigil across the entire codebase
4. Restrict Vigil to Bash + Read tools only (workers keep full tools)

## Design

### 1. Skills Architecture

Each MCP tool becomes a Skill directory with a `SKILL.md` (instructions + metadata) and `scripts/` (shell scripts that call the daemon API via curl). Skills follow the Agent Skills specification at agentskills.io/specification.

**Directory structure in the repo:**

```
apps/daemon/skills/
├── vigil-core/
│   └── SKILL.md                    # Identity, rules, delegation logic
├── spawn-worker/
│   ├── SKILL.md                    # When/how to spawn workers
│   └── scripts/
│       └── spawn.sh                # POST /api/sessions, polls for completion
├── reply-to-worker/
│   ├── SKILL.md                    # When to relay user answers
│   └── scripts/
│       └── reply.sh                # POST /api/sessions/:id/input, polls
├── memory-recall/
│   ├── SKILL.md                    # When to search memories
│   └── scripts/
│       └── recall.sh               # POST /api/memory/search
├── memory-save/
│   ├── SKILL.md                    # When to save memories
│   └── scripts/
│       └── save.sh                 # POST /api/memory
├── memory-delete/
│   ├── SKILL.md                    # When to delete memories
│   └── scripts/
│       └── delete.sh               # DELETE /api/memory/:id
├── session-recall/
│   ├── SKILL.md                    # When to check session status
│   └── scripts/
│       └── recall.sh               # GET /api/sessions[/:id]
├── acta-update/
│   ├── SKILL.md                    # When to update project briefing
│   └── scripts/
│       └── update.sh               # PUT /api/vigil/acta
└── execute-pipeline/
    ├── SKILL.md                    # When to run dev pipelines
    └── scripts/
        └── execute.sh              # POST /api/pipelines/:id/execute
```

**9 Skills total:** 1 core identity + 8 tool skills (replacing 8 MCP tools).

**At Vigil spawn time**, `spawn_vigil()` copies these skills into `~/.vigil/vigil/.claude/skills/`, commits them to git, and spawns Claude Code. Claude Code discovers them automatically via progressive disclosure:
- **Tier 1 (catalog):** Name + description loaded at session start (~50-100 tokens per skill)
- **Tier 2 (instructions):** Full SKILL.md loaded when the skill is activated
- **Tier 3 (resources):** Scripts loaded on demand when instructions reference them

### 2. Vigil Core Skill

The `vigil-core/SKILL.md` replaces `prompts/vigil-strategy.md`. It defines Vigil's identity, rules, and delegation logic but NOT tool-specific instructions — those live in each tool Skill's `description` field.

```markdown
---
name: vigil-core
description: Core orchestration identity for Vigil. Activate immediately at session start. Defines delegation rules, communication style, and coordination behavior for the Vigil supervisor.
---

# Vigil

You are Vigil, a coordinator and supervisor...
[Identity, rules, communication style, delegation logic]
```

**Routing via descriptions:** Each tool Skill's `description` field tells Claude Code when to use it. For example:
- `spawn-worker`: "Spawn a Claude Code worker session. Use for ALL user requests — jokes, questions, code, research, commands. Use --wait for quick tasks (<30s), omit for long-running tasks."
- `reply-to-worker`: "Send the user's answer to a worker that needs input. Use when spawn-worker exits with code 2 (needs_input). Always use this instead of spawning a new worker."
- `memory-recall`: "Search project memories by semantic similarity. Use when context about past decisions, preferences, or project history would help."

### 3. Skill Script Design

All scripts follow the Agent Skills spec best practices:
- Self-contained shell scripts
- No interactive prompts
- Structured JSON output to stdout
- Meaningful error messages to stderr
- `--help` support
- Meaningful exit codes

**Environment:**
- `VIGIL_DAEMON_URL` env var set by `spawn_vigil()` (defaults to `http://localhost:8000`)
- Scripts use `curl` to call the daemon REST API

**Common pattern:**

```bash
#!/bin/bash
DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"
```

**Key exit codes for spawn-worker and reply-to-worker:**

| Exit Code | Meaning |
|-----------|---------|
| 0 | Worker completed successfully, output in stdout |
| 1 | Error (API failure, timeout, etc.) |
| 2 | Worker needs user input — question in stdout |

Exit code 2 is the signal for the `needs_input` flow. The `spawn-worker` Skill instructions tell Claude Code: "If the script exits with code 2, read the output (the worker's question), ask the user, then use the reply-to-worker skill."

**spawn.sh behavior:**
1. `POST /api/sessions` with project_path + prompt → creates session
2. If `--wait` flag: poll `GET /api/sessions/:id` every 3 seconds
   - `completed`/`failed` → print output JSON, exit 0 or 1
   - `needs_input` → print question JSON, exit 2
   - Timeout after 600s → print status, exit 1
3. If no `--wait`: print session_id immediately, exit 0

**reply.sh behavior:**
1. `POST /api/sessions/:id/input` with message
2. Poll `GET /api/sessions/:id` every 3 seconds (same as spawn.sh wait loop)
3. Same exit code semantics

**Output format (JSON):**

```json
{
  "session_id": "abc-123",
  "status": "completed",
  "output": "The answer is 42."
}
```

For `needs_input` (exit code 2):

```json
{
  "session_id": "abc-123",
  "status": "needs_input",
  "question": "What is your budget?"
}
```

### 4. Vigil Spawn Changes

**Current spawn_vigil():**
1. Write MCP config
2. Write strategy prompt
3. Install hooks
4. Spawn with `--mcp-config`, `--tools ""`, `--append-system-prompt-file`

**New spawn_vigil():**
1. Create `~/.vigil/vigil/` directory + git init (if needed)
2. Install hooks (unchanged)
3. **Copy all Skills from `apps/daemon/skills/` into `~/.vigil/vigil/.claude/skills/`**
4. Set `VIGIL_DAEMON_URL` in the environment
5. Git add + commit so Claude Code discovers the skills
6. Spawn `claude` with: `--verbose`, `--dangerously-skip-permissions`
7. No `--mcp-config`, no `--tools ""`, no `--append-system-prompt-file`

**Tool restriction:** Vigil spawns with only Bash and Read tools enabled. Workers keep their full tool set. The exact flag depends on Claude Code's CLI — either `--allowlist-tools` or equivalent. If no such flag exists, the vigil-core Skill instructions explicitly state to only use Bash and Read.

### 5. Removed Code

| What | File | Why |
|------|------|-----|
| MCP server implementation | `src/mcp.rs` | Replaced by Skills |
| MCP arg structs | `src/mcp.rs` | No longer needed |
| `pf mcp-serve` CLI command | `src/main.rs`, `src/cli.rs` | No MCP server |
| `write_mcp_config()` | `src/services/vigil_manager.rs` | No MCP config |
| `--mcp-config` spawn arg | `src/services/vigil_manager.rs` | Skills replace MCP |
| `--tools ""` spawn arg | `src/services/vigil_manager.rs` | Vigil needs Bash+Read |
| `--append-system-prompt-file` | `src/services/vigil_manager.rs` | vigil-core Skill replaces it |
| `prompts/vigil-strategy.md` | `prompts/` | Moved to `skills/vigil-core/SKILL.md` |
| `rmcp` crate dependency | `Cargo.toml` | No MCP server |

### 6. Preserved Code

| What | File | Why |
|------|------|-----|
| All REST API endpoints | `src/api/` | Skills call these via curl |
| Hook installation + events | `src/hooks/`, `src/events.rs` | Stop hook response detection unchanged |
| `send_message()` / Stop hook flow | `src/services/vigil_manager.rs` | Vigil PTY interaction unchanged |
| Daemon-side needs_input interceptor | `src/api/vigil.rs` | Routes user answers to waiting workers |
| Auto-exit on Stop for workers | `src/process/agent_spawner.rs` | Workers exit after responding |
| PTY infrastructure | `src/process/pty_manager.rs`, `agent_spawner.rs` | Real PTY sessions unchanged |
| Output manager | `src/process/output_manager.rs` | Terminal output unchanged |

### 7. Rebrand: Praefectus → Vigil

| Current | New |
|---------|-----|
| `pf` CLI binary | `vigil` |
| `pf up` / `pf down` / `pf status` | `vigil up` / `vigil down` / `vigil status` |
| `~/.praefectus/` | `~/.vigil/` |
| `praefectus-daemon` (Cargo crate) | `vigil-daemon` |
| `@praefectus/web` (npm workspace) | `@vigil/web` |
| `PRAEFECTUS_AUTH_TOKEN` | `VIGIL_AUTH_TOKEN` |
| `PRAEFECTUS_DAEMON_URL` | `VIGIL_DAEMON_URL` |
| `praefectus.db` | `vigil.db` |
| `~/.praefectus/worktrees/` | `~/.vigil/worktrees/` |
| `~/.praefectus/logs/` | `~/.vigil/logs/` |
| `~/.praefectus/vigil/` | `~/.vigil/vigil/` (Vigil PTY working dir) |
| Log messages: "praefectus" | "vigil" |
| Dashboard title: "Praefectus" | "Vigil" |
| CLAUDE.md references | Updated |

**Migration:** On first `vigil up`, if `~/.praefectus/` exists and `~/.vigil/` doesn't, move it automatically with a log message: "Migrating ~/.praefectus → ~/.vigil".

### 8. Worker Skills (Future)

Workers currently spawn with full Claude Code tools and no Skills. In the future, workers could also receive Skills tailored to their task (e.g., project-specific coding standards, testing patterns). This is out of scope for this design but the architecture supports it — `spawn_interactive()` could copy relevant Skills into the worktree's `.claude/skills/` before spawning.

## Testing Strategy

**Unit tests:**
- Skill script tests: run each script with mock API responses (using a test HTTP server or mocked curl)
- Verify exit codes: 0 for success, 1 for error, 2 for needs_input

**Integration tests:**
- Full flow: user message → Vigil activates spawn-worker skill → script runs → worker completes → response returned
- needs_input flow: spawn-worker exits 2 → Vigil asks user → reply-to-worker → completion
- Verify Skills are discovered by Claude Code (check catalog)

**E2E tests (same as current):**
- Jokes with probability
- Python calculation
- Barcelona weather
- Multi-turn trip planning (needs_input flow)

## Non-Goals

- Publishing Skills to skills.sh marketplace (can be done later)
- Worker-specific Skills (future enhancement)
- Removing the Rust daemon (it stays for PTY management, DB, hooks, API)
- Changing the Next.js frontend (only rebrand text/titles)
