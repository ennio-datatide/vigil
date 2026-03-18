# Vigil Skills Architecture & Rebrand

**Date:** 2026-03-18
**Status:** Proposed

## Problem

Praefectus currently uses 8 MCP tools (`pf mcp-serve` subprocess) to give Vigil its capabilities — `spawn_worker`, `reply_to_worker`, `memory_recall`, `memory_save`, `memory_delete`, `session_recall`, `acta_update`, `execute_pipeline`. This requires a custom MCP server implementation (`mcp.rs`), MCP config files, the `--tools ""` flag to disable built-in tools, and the `rmcp` crate dependency. The strategy prompt is a hardcoded markdown file embedded via `--append-system-prompt-file`.

This is inflexible: adding new capabilities requires modifying Rust code, recompiling, and redeploying. The Agent Skills specification (agentskills.io) provides a standard, open format for giving agents capabilities via markdown instruction documents with bundled scripts — no custom server needed.

Additionally, "Praefectus" is being rebranded to "Vigil" for a cleaner identity.

## Goal

1. Replace all 8 MCP tools with Agent Skills (SKILL.md + scripts that curl the daemon REST API)
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

**At Vigil spawn time**, `spawn_vigil()` copies these skills into `~/.vigil/session/.claude/skills/`, commits them to git, and spawns Claude Code. Claude Code discovers them automatically via progressive disclosure:
- **Tier 1 (catalog):** Name + description loaded at session start (~50-100 tokens per skill)
- **Tier 2 (instructions):** Full SKILL.md loaded when the skill is activated
- **Tier 3 (resources):** Scripts loaded on demand when instructions reference them

### 2. Vigil Core Skill — Bootstrapping

The `vigil-core/SKILL.md` replaces `prompts/vigil-strategy.md`. It defines Vigil's identity, rules, and delegation logic.

**Activation guarantee:** Claude Code only activates skills when a task matches the description. To ensure `vigil-core` is activated at session start, `spawn_vigil()` sends a bootstrap message to the PTY after the trust prompt is accepted:

```
Activate the vigil-core skill and begin your session.
```

This triggers Claude Code to match the `vigil-core` description and load the full SKILL.md. The bootstrap message is sent via the existing delayed-PTY-write mechanism (same pattern as the trust prompt auto-accept).

**SKILL.md structure:**

```markdown
---
name: vigil-core
description: Core orchestration identity for Vigil. Activate this skill when starting a session or when asked to act as Vigil. Defines delegation rules, communication style, and coordination behavior.
---

# Vigil

You are Vigil, a coordinator and supervisor...
[Identity, rules, communication style, delegation logic]
[Only use Bash and Read tools — never Write, Edit, or other tools]
[Always delegate work via the spawn-worker skill]
```

**Routing via descriptions:** Each tool Skill's `description` field tells Claude Code when to use it. The catalog (all descriptions) is visible at session start, so Claude Code knows what's available without activating every skill.

### 3. Skill Script Design

All scripts follow the Agent Skills spec best practices:
- Self-contained shell scripts
- No interactive prompts
- Structured JSON output to stdout
- Meaningful error messages to stderr
- `--help` support

**Environment:**
- `VIGIL_DAEMON_URL` env var set by `spawn_vigil()` (defaults to `http://localhost:8000`)
- Scripts use `curl` to call the daemon REST API

**Common pattern:**

```bash
#!/bin/bash
DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"
```

**Status signaling via JSON stdout (not exit codes):**

All scripts exit 0 on success. The `status` field in the JSON output signals the result. This is more reliable than exit codes for agent consumption — Claude Code reliably reads JSON from stdout but exit code interpretation is fragile.

**spawn.sh output for each terminal state:**

| Session Status | JSON Output | Script Exit |
|----------------|-------------|-------------|
| `completed` | `{"session_id":"...","status":"completed","output":"..."}` | 0 |
| `failed` | `{"session_id":"...","status":"failed","output":"...","error":"..."}` | 0 |
| `cancelled` | `{"session_id":"...","status":"cancelled","output":"..."}` | 0 |
| `interrupted` | `{"session_id":"...","status":"interrupted","output":"..."}` | 0 |
| `needs_input` | `{"session_id":"...","status":"needs_input","question":"..."}` | 0 |
| Timeout (600s) | `{"session_id":"...","status":"timeout","message":"Worker still running after 600s. Use session-recall to check status."}` | 0 |
| API error | Error message to stderr | 1 |

Exit code 1 is reserved for actual script failures (network error, malformed response). All business-logic states (including needs_input) use exit 0 with structured JSON.

The `spawn-worker` SKILL.md instructions tell Claude Code: "If the output JSON has `status: needs_input`, read the `question` field, ask the user, then use the reply-to-worker skill with the session_id."

**spawn.sh behavior:**
1. `POST /api/sessions` with `--project-path` and `--prompt` → creates session
2. If `--wait` flag: poll `GET /api/sessions/:id` every 3 seconds
   - Terminal state → print JSON with output, exit 0
   - `needs_input` → print JSON with question, exit 0
   - Non-200 API response → retry silently (keep polling)
   - Timeout after 600s → print timeout JSON, exit 0
3. If no `--wait`: print `{"session_id":"...","status":"queued"}` immediately, exit 0

**reply.sh behavior:**
1. `POST /api/sessions/:id/input` with `--session-id` and `--message`
2. Poll `GET /api/sessions/:id` every 3 seconds (same loop as spawn.sh)
3. Same JSON output format and exit semantics

**spawn.sh optional flags:**
- `--project-path <path>` (required) — project directory
- `--prompt <text>` (required) — task for the worker
- `--wait` — block until completion (default: no wait)
- `--skill <name>` — optional skill to assign (forward-compatible, currently unused)

### 4. Vigil Spawn Changes

**Current spawn_vigil():**
1. Write MCP config
2. Write strategy prompt
3. Install hooks
4. Spawn with `--mcp-config`, `--tools ""`, `--append-system-prompt-file`

**New spawn_vigil():**
1. Create `~/.vigil/session/` directory + git init (if needed)
2. Install hooks (unchanged)
3. **Copy all Skills from `apps/daemon/skills/` into `~/.vigil/session/.claude/skills/`**
4. **Clean up stale files:** remove `mcp-config.json` and `strategy.md` if present (leftover from pre-Skills architecture)
5. Set `VIGIL_DAEMON_URL` in the PTY environment
6. Git add + commit so Claude Code discovers the skills
7. Spawn `claude` with: `--verbose`, `--dangerously-skip-permissions`
8. Auto-accept trust prompt (Enter after 2s, unchanged)
9. **Send bootstrap message** after 5s: `"Activate the vigil-core skill and begin your session.\r"`
10. No `--mcp-config`, no `--tools ""`, no `--append-system-prompt-file`

**Tool restriction:** The vigil-core Skill instructions explicitly state: "You may ONLY use Bash and Read tools. Never use Write, Edit, Grep, Glob, WebFetch, or any other tool." This is an instruction-level restriction. Claude Code does not currently support `--allowlist-tools` as a CLI flag, so instruction-only restriction is the approach. The risk is that Vigil could ignore this instruction — but the same risk existed with the MCP approach (Vigil could have used built-in tools despite `--tools ""`). In practice, the strategy prompt has been effective at controlling behavior.

### 5. Removed Code

| What | File | Why |
|------|------|-----|
| MCP server implementation | `src/mcp.rs` | Replaced by Skills |
| MCP arg structs | `src/mcp.rs` | No longer needed |
| `vigil mcp-serve` CLI command | `src/main.rs`, `src/cli.rs` | No MCP server |
| `write_mcp_config()` | `src/services/vigil_manager.rs` | No MCP config |
| `--mcp-config` spawn arg | `src/services/vigil_manager.rs` | Skills replace MCP |
| `--tools ""` spawn arg | `src/services/vigil_manager.rs` | Vigil needs Bash+Read |
| `--append-system-prompt-file` | `src/services/vigil_manager.rs` | vigil-core Skill replaces it |
| `prompts/vigil-strategy.md` | `prompts/` | Moved to `skills/vigil-core/SKILL.md` |
| `rmcp` crate dependency | `Cargo.toml` | No MCP server |
| `schemars` crate dependency | `Cargo.toml` | Only used by MCP arg structs |

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
| `praefectus_home` (Config field) | `vigil_home` |
| `PRAEFECTUS_AUTH_TOKEN` | `VIGIL_AUTH_TOKEN` |
| `PRAEFECTUS_DASHBOARD_URL` | `VIGIL_DASHBOARD_URL` |
| `PRAEFECTUS_DAEMON_URL` | `VIGIL_DAEMON_URL` |
| `praefectus.db` | `vigil.db` |
| `~/.praefectus/worktrees/` | `~/.vigil/worktrees/` |
| `~/.praefectus/logs/` | `~/.vigil/logs/` |
| `~/.praefectus/vigil/` | `~/.vigil/session/` (Vigil PTY working dir) |
| Log messages: "praefectus" | "vigil" |
| Dashboard title: "Praefectus" | "Vigil" |
| CLAUDE.md references | Updated |

**Migration:** On first `vigil up`, if `~/.praefectus/` exists and `~/.vigil/` doesn't:
1. Move `~/.praefectus/` → `~/.vigil/` with log message: "Migrating ~/.praefectus → ~/.vigil"
2. Remove stale files: `~/.vigil/session/mcp-config.json`, `~/.vigil/session/strategy.md`
3. Rename `vigil.db` if it was `praefectus.db`

### 8. Worker Skills (Future)

Workers currently spawn with full Claude Code tools and no Skills. In the future, workers could also receive Skills tailored to their task (e.g., project-specific coding standards, testing patterns). The `--skill` flag on `spawn.sh` is included for forward compatibility. This is out of scope for this design but the architecture supports it — `spawn_interactive()` could copy relevant Skills into the worktree's `.claude/skills/` before spawning.

## Testing Strategy

**Unit tests:**
- Skill script tests: run each script with mock API responses (using a test HTTP server or mocked curl)
- Verify JSON output format for all terminal states (completed, failed, cancelled, interrupted, needs_input, timeout)

**Integration tests:**
- Full flow: user message → Vigil activates spawn-worker skill → script runs → worker completes → response returned
- needs_input flow: spawn-worker returns `{"status":"needs_input"}` → Vigil asks user → reply-to-worker → completion
- Verify Skills are discovered by Claude Code (check catalog)
- Bootstrap message activates vigil-core skill

**E2E tests (same as current):**
- Jokes with probability
- Python calculation
- Barcelona weather
- Multi-turn trip planning (needs_input flow)

## Non-Goals

- Publishing Skills to skills.sh marketplace (can be done later)
- Worker-specific Skills (future enhancement beyond --skill flag)
- Removing the Rust daemon (it stays for PTY management, DB, hooks, API)
- Changing the Next.js frontend beyond rebrand text/titles
