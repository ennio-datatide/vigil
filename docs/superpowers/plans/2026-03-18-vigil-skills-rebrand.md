# Vigil Skills & Rebrand — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace MCP tools with Agent Skills, delete the MCP server, and rebrand Praefectus → Vigil.

**Architecture:** 9 Skills (SKILL.md + bash scripts) replace 8 MCP tools + strategy prompt. Scripts call the daemon REST API via curl. Vigil spawns with Skills in `.claude/skills/` instead of MCP config. Full rebrand of binary names, paths, package names, and UI text.

**Tech Stack:** Agent Skills spec (agentskills.io), bash/curl scripts, Rust (axum), Next.js

**Spec:** `docs/superpowers/specs/2026-03-18-vigil-skills-rebrand-design.md`

---

## File Structure

### New Files (Skills)

| File | Responsibility |
|------|---------------|
| `apps/daemon/skills/vigil-core/SKILL.md` | Vigil identity, rules, delegation logic |
| `apps/daemon/skills/spawn-worker/SKILL.md` | When/how to spawn workers |
| `apps/daemon/skills/spawn-worker/scripts/spawn.sh` | POST /api/sessions, poll for completion |
| `apps/daemon/skills/reply-to-worker/SKILL.md` | When to relay user answers |
| `apps/daemon/skills/reply-to-worker/scripts/reply.sh` | POST /api/sessions/:id/input, poll |
| `apps/daemon/skills/memory-recall/SKILL.md` | When to search memories |
| `apps/daemon/skills/memory-recall/scripts/recall.sh` | POST /api/memory/search |
| `apps/daemon/skills/memory-save/SKILL.md` | When to save memories |
| `apps/daemon/skills/memory-save/scripts/save.sh` | POST /api/memory |
| `apps/daemon/skills/memory-delete/SKILL.md` | When to delete memories |
| `apps/daemon/skills/memory-delete/scripts/delete.sh` | DELETE /api/memory/:id |
| `apps/daemon/skills/session-recall/SKILL.md` | When to check sessions |
| `apps/daemon/skills/session-recall/scripts/recall.sh` | GET /api/sessions |
| `apps/daemon/skills/acta-update/SKILL.md` | When to update project briefing |
| `apps/daemon/skills/acta-update/scripts/update.sh` | PUT /api/vigil/acta |
| `apps/daemon/skills/execute-pipeline/SKILL.md` | When to run dev pipelines |
| `apps/daemon/skills/execute-pipeline/scripts/execute.sh` | POST /api/pipelines/:id/execute |

### Modified Files

| File | Change |
|------|--------|
| `apps/daemon/Cargo.toml` | Rename crate, remove rmcp + schemars, rename binaries |
| `apps/daemon/src/config.rs` | `praefectus_home` → `vigil_home`, paths → `.vigil` |
| `apps/daemon/src/cli.rs` | Command name → `vigil`, home dir → `.vigil` |
| `apps/daemon/src/main.rs` | `praefectus_daemon` → `vigil_daemon`, remove McpServe |
| `apps/daemon/src/lib.rs` | Log messages, filter name |
| `apps/daemon/src/services/vigil_manager.rs` | Remove MCP config, install Skills, bootstrap message |
| `apps/daemon/src/hooks/installer.rs` | Comment text only |
| `apps/daemon/src/process/agent_spawner.rs` | Branch name prefix |
| `apps/web/package.json` | `@praefectus/web` → `@vigil/web` |
| `packages/shared/package.json` | `@praefectus/shared` → `@vigil/shared` |
| `apps/web/src/lib/types.ts` | Import from `@vigil/shared` |
| `apps/web/src/app/layout.tsx` | Title → Vigil |
| `apps/web/src/app/manifest.ts` | Name → Vigil |
| `apps/web/src/app/opengraph-image.tsx` | Text → Vigil |
| `apps/web/src/app/dashboard/layout.tsx` | Logo component import |
| `apps/web/src/components/ui/praefectus-logo.tsx` | Rename to `vigil-logo.tsx`, text → Vigil |
| `apps/web/src/lib/auth-token.ts` | Storage key |
| `apps/web/src/app/dashboard/auth/page.tsx` | CLI commands |
| `apps/web/src/app/dashboard/settings/page.tsx` | CLI commands, text |
| `CLAUDE.md` | All references |
| `.github/workflows/ci.yml` | Email domain |

### Deleted Files

| File | Why |
|------|-----|
| `apps/daemon/src/mcp.rs` | Replaced by Skills |
| `apps/daemon/prompts/vigil-strategy.md` | Moved to `skills/vigil-core/SKILL.md` |

---

## Chunk 1: Create All Skills (Parallelizable)

### Task 1: Create vigil-core Skill

**Files:**
- Create: `apps/daemon/skills/vigil-core/SKILL.md`

- [ ] **Step 1: Write the SKILL.md**

The vigil-core Skill contains Vigil's identity, rules, and delegation logic. Port the content from `apps/daemon/prompts/vigil-strategy.md` into the Agent Skills format. Key changes:
- Add YAML frontmatter with `name: vigil-core` and `description`
- Remove MCP-specific language (references to MCP tools)
- Replace tool references with Skill names (e.g., "use the spawn-worker skill")
- Add instruction: "You may ONLY use Bash and Read tools. Never use Write, Edit, Grep, Glob, WebFetch, or any other tool."
- Keep all delegation rules, communication style, examples
- Update CLI references from `pf` to `vigil`

Read `apps/daemon/prompts/vigil-strategy.md` first, then create the new Skill.

- [ ] **Step 2: Commit**

```bash
git add apps/daemon/skills/vigil-core/
git commit -m "feat: add vigil-core Skill (replaces strategy prompt)"
```

---

### Task 2: Create spawn-worker Skill + script

**Files:**
- Create: `apps/daemon/skills/spawn-worker/SKILL.md`
- Create: `apps/daemon/skills/spawn-worker/scripts/spawn.sh`

- [ ] **Step 1: Write SKILL.md**

```yaml
---
name: spawn-worker
description: Spawn a Claude Code worker session to handle a user request. Use for ALL user tasks — jokes, questions, code, research, commands, web searches. Use --wait for quick tasks (<30s), omit for long-running tasks. If the output JSON has status "needs_input", ask the user the question and use the reply-to-worker skill.
---
```

Body: instructions for when to use --wait vs not, how to interpret JSON output, the needs_input flow, examples.

- [ ] **Step 2: Write spawn.sh**

```bash
#!/bin/bash
set -euo pipefail
DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"
# Parse args: --project-path, --prompt, --wait, --skill, --help
# POST /api/sessions → get session_id
# If --wait: poll GET /api/sessions/:id every 3s
#   Output JSON for each terminal state
# If no --wait: print {"session_id":"...","status":"queued"}, exit 0
```

Full implementation with --help, arg parsing, polling loop, JSON output for all states (completed, failed, cancelled, interrupted, needs_input, timeout). Exit 0 for all business states, exit 1 for script errors only.

- [ ] **Step 3: Make script executable and commit**

```bash
chmod +x apps/daemon/skills/spawn-worker/scripts/spawn.sh
git add apps/daemon/skills/spawn-worker/
git commit -m "feat: add spawn-worker Skill with polling script"
```

---

### Task 3: Create reply-to-worker Skill + script

**Files:**
- Create: `apps/daemon/skills/reply-to-worker/SKILL.md`
- Create: `apps/daemon/skills/reply-to-worker/scripts/reply.sh`

- [ ] **Step 1: Write SKILL.md and reply.sh**

Same pattern as spawn-worker. SKILL.md describes when to use (after spawn-worker returns needs_input). Script POSTs to `/api/sessions/:id/input` then polls for completion with same JSON output format.

- [ ] **Step 2: Commit**

---

### Task 4: Create memory Skills (recall, save, delete)

**Files:**
- Create: `apps/daemon/skills/memory-recall/SKILL.md`
- Create: `apps/daemon/skills/memory-recall/scripts/recall.sh`
- Create: `apps/daemon/skills/memory-save/SKILL.md`
- Create: `apps/daemon/skills/memory-save/scripts/save.sh`
- Create: `apps/daemon/skills/memory-delete/SKILL.md`
- Create: `apps/daemon/skills/memory-delete/scripts/delete.sh`

- [ ] **Step 1: Write all 3 memory Skills**

Each has a SKILL.md with appropriate description and a script that curls the relevant API endpoint. Simple request/response (no polling needed).

- [ ] **Step 2: Commit**

---

### Task 5: Create session-recall, acta-update, execute-pipeline Skills

**Files:**
- Create: `apps/daemon/skills/session-recall/SKILL.md` + `scripts/recall.sh`
- Create: `apps/daemon/skills/acta-update/SKILL.md` + `scripts/update.sh`
- Create: `apps/daemon/skills/execute-pipeline/SKILL.md` + `scripts/execute.sh`

- [ ] **Step 1: Write all 3 Skills**

Each has a SKILL.md with appropriate description and a script. session-recall is a GET, acta-update is a PUT, execute-pipeline is a POST. All simple request/response.

- [ ] **Step 2: Commit**

---

## Chunk 2: Rebrand Praefectus → Vigil (Parallelizable with Chunk 1)

### Task 6: Rebrand Rust daemon

**Files:**
- Modify: `apps/daemon/Cargo.toml`
- Modify: `apps/daemon/src/config.rs`
- Modify: `apps/daemon/src/cli.rs`
- Modify: `apps/daemon/src/main.rs`
- Modify: `apps/daemon/src/lib.rs`
- Modify: `apps/daemon/src/hooks/installer.rs`
- Modify: `apps/daemon/src/process/agent_spawner.rs`
- Modify: `apps/daemon/src/services/vigil_manager.rs`

- [ ] **Step 1: Rename in Cargo.toml**

```toml
[package]
name = "vigil-daemon"
description = "Vigil daemon — AI session orchestrator"

[[bin]]
name = "vigil"
path = "src/main.rs"
```

Remove the `pf` binary alias. Remove `rmcp` and `schemars` dependencies.

- [ ] **Step 2: Rename in config.rs**

- `praefectus_home` → `vigil_home` (field name and all references)
- `".praefectus"` → `".vigil"` (home directory path)
- `"praefectus.db"` → `"vigil.db"` (database filename)
- `PRAEFECTUS_AUTH_TOKEN` → `VIGIL_AUTH_TOKEN`
- `PRAEFECTUS_DASHBOARD_URL` → `VIGIL_DASHBOARD_URL`

- [ ] **Step 3: Rename in cli.rs**

- `#[command(name = "praefectus")]` → `#[command(name = "vigil")]`
- `".praefectus"` → `".vigil"` in home dir function
- All doc comments

- [ ] **Step 4: Rename in main.rs**

- `use praefectus_daemon::` → `use vigil_daemon::`
- Remove `Command::McpServe` match arm

- [ ] **Step 5: Rename in lib.rs**

- Log messages: "starting praefectus daemon" → "starting vigil daemon"
- Filter: `praefectus_daemon=info` → `vigil_daemon=info`
- "praefectus daemon stopped" → "vigil daemon stopped"

- [ ] **Step 6: Rename in other files**

- `hooks/installer.rs`: comment "forwards to praefectus server" → "forwards to vigil server"
- `agent_spawner.rs`: branch name `praefectus/{session_id}` → `vigil/{session_id}`
- `vigil_manager.rs`: `config.praefectus_home` → `config.vigil_home`

- [ ] **Step 7: Build and fix any compilation errors**

Run: `cd apps/daemon && cargo build 2>&1`
Fix all errors from the rename. The crate name change means `praefectus_daemon::` becomes `vigil_daemon::` everywhere.

- [ ] **Step 8: Run clippy**

Run: `cargo clippy -- -D warnings`

- [ ] **Step 9: Commit**

```bash
git add apps/daemon/
git commit -m "feat: rebrand Rust daemon — praefectus → vigil"
```

---

### Task 7: Rebrand frontend + shared packages

**Files:**
- Modify: `packages/shared/package.json`
- Modify: `apps/web/package.json`
- Modify: `apps/web/src/lib/types.ts`
- Modify: `apps/web/src/app/layout.tsx`
- Modify: `apps/web/src/app/manifest.ts`
- Modify: `apps/web/src/app/opengraph-image.tsx`
- Modify: `apps/web/src/app/dashboard/layout.tsx`
- Rename: `apps/web/src/components/ui/praefectus-logo.tsx` → `vigil-logo.tsx`
- Modify: `apps/web/src/lib/auth-token.ts`
- Modify: `apps/web/src/app/dashboard/auth/page.tsx`
- Modify: `apps/web/src/app/dashboard/settings/page.tsx`
- Modify: `CLAUDE.md`
- Modify: `.github/workflows/ci.yml`
- Modify: `package.json` (root)

- [ ] **Step 1: Rename npm packages**

- `packages/shared/package.json`: `@praefectus/shared` → `@vigil/shared`
- `apps/web/package.json`: `@praefectus/web` → `@vigil/web`, dep `@praefectus/shared` → `@vigil/shared`
- `apps/web/src/lib/types.ts`: import from `@vigil/shared`

- [ ] **Step 2: Rename UI text**

- `layout.tsx`: all "Praefectus" → "Vigil" in metadata
- `manifest.ts`: name/short_name → "Vigil"
- `opengraph-image.tsx`: alt text and display text → "Vigil"
- `auth/page.tsx`: `praefectus auth` → `vigil auth`
- `settings/page.tsx`: "Praefectus daemon" → "Vigil daemon", CLI commands

- [ ] **Step 3: Rename logo component**

- Rename file `praefectus-logo.tsx` → `vigil-logo.tsx`
- Rename component `PraefectusLogo` → `VigilLogo`
- Change display text "Praefectus" → "Vigil"
- Update import in `dashboard/layout.tsx`

- [ ] **Step 4: Rename storage key and other references**

- `auth-token.ts`: `praefectus-api-token` → `vigil-api-token`
- `CLAUDE.md`: all references
- `.github/workflows/ci.yml`: email `ci@praefectus.dev` → `ci@vigil.dev`

- [ ] **Step 5: Run biome and build**

```bash
npx biome check --write .
npm run build
```

- [ ] **Step 6: Commit**

```bash
git add .
git commit -m "feat: rebrand frontend and docs — Praefectus → Vigil"
```

---

## Chunk 3: Wire Skills into Vigil Spawn + Delete MCP

### Task 8: Update vigil_manager to install Skills instead of MCP

**Files:**
- Modify: `apps/daemon/src/services/vigil_manager.rs`

- [ ] **Step 1: Remove MCP config writing**

Delete `write_mcp_config()` function and its call in `spawn_vigil()`.

- [ ] **Step 2: Add Skills installation**

In `spawn_vigil()`, after creating the vigil directory and installing hooks:

```rust
// Copy Skills from the bundled skills directory into the vigil session
let skills_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
let skills_dst = self.vigil_dir.join(".claude").join("skills");
// Copy each skill directory
```

Use `fs::create_dir_all` + recursive copy for each skill subdirectory. The skills are included at compile time via the crate's directory structure.

- [ ] **Step 3: Remove strategy prompt writing**

Delete the `include_str!("../../prompts/vigil-strategy.md")` and file write. The vigil-core Skill replaces it.

- [ ] **Step 4: Remove MCP-related spawn args**

In `spawn_vigil_pty()`, remove:
- `--mcp-config` arg
- `--tools ""` arg
- `--append-system-prompt-file` arg

- [ ] **Step 5: Add bootstrap message**

After the trust-prompt auto-accept (2s delay), add a 5s delayed message:
```rust
// Send bootstrap to activate vigil-core Skill
tokio::time::sleep(Duration::from_secs(5)).await;
pty_mgr.write(&sid, b"Activate the vigil-core skill and begin your session.\r").await;
```

- [ ] **Step 6: Set VIGIL_DAEMON_URL in PTY environment**

In `spawn_vigil_pty()`, add `cmd.env("VIGIL_DAEMON_URL", &daemon_url)` so skill scripts can reach the daemon.

- [ ] **Step 7: Add stale file cleanup**

In `spawn_vigil()`, after creating the directory:
```rust
// Clean up stale MCP-era files
let _ = std::fs::remove_file(self.vigil_dir.join("mcp-config.json"));
let _ = std::fs::remove_file(self.vigil_dir.join("strategy.md"));
```

- [ ] **Step 8: Commit**

```bash
git add apps/daemon/src/services/vigil_manager.rs
git commit -m "feat: install Skills in Vigil spawn, remove MCP config"
```

---

### Task 9: Delete MCP server and strategy prompt

**Files:**
- Delete: `apps/daemon/src/mcp.rs`
- Delete: `apps/daemon/prompts/vigil-strategy.md`
- Modify: `apps/daemon/src/lib.rs` (remove `mod mcp` declaration)
- Modify: `apps/daemon/src/main.rs` (remove McpServe command)
- Modify: `apps/daemon/src/cli.rs` (remove McpServe variant)

- [ ] **Step 1: Delete files**

```bash
rm apps/daemon/src/mcp.rs
rm apps/daemon/prompts/vigil-strategy.md
```

- [ ] **Step 2: Remove module declaration and CLI command**

In `lib.rs`: remove `pub(crate) mod mcp;` (or `pub mod mcp;`)
In `cli.rs`: remove the `McpServe` variant from the `Command` enum
In `main.rs`: remove the `Command::McpServe` match arm and `praefectus_daemon::mcp::run_mcp_server` call

- [ ] **Step 3: Remove rmcp and schemars from Cargo.toml**

(If not already done in Task 6)

- [ ] **Step 4: Build and fix**

```bash
cargo build
cargo clippy -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add apps/daemon/
git commit -m "feat: delete MCP server and strategy prompt"
```

---

### Task 10: Add migration logic for ~/.praefectus → ~/.vigil

**Files:**
- Modify: `apps/daemon/src/config.rs`

- [ ] **Step 1: Add migration check**

In `Config::resolve()`, before creating `~/.vigil/`:
```rust
let old_home = home.join(".praefectus");
let new_home = home.join(".vigil");
if old_home.exists() && !new_home.exists() {
    tracing::info!("Migrating ~/.praefectus → ~/.vigil");
    std::fs::rename(&old_home, &new_home)?;
}
```

- [ ] **Step 2: Commit**

```bash
git add apps/daemon/src/config.rs
git commit -m "feat: auto-migrate ~/.praefectus → ~/.vigil on first run"
```

---

## Chunk 4: Final Build, Test, and Verification

### Task 11: Full build and test

- [ ] **Step 1: Build Rust daemon**

Run: `cd apps/daemon && cargo build`

- [ ] **Step 2: Run Rust tests**

Run: `cargo test`

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings`

- [ ] **Step 4: Build frontend**

Run: `npm run build`

- [ ] **Step 5: Run biome**

Run: `npx biome check --write .`

- [ ] **Step 6: Verify no dead references**

Search for remaining `praefectus`, `pf `, `mcp-serve`, `rmcp`, `schemars` references.

- [ ] **Step 7: Install new binary**

Run: `cargo install --path .` (installs `vigil` binary)

- [ ] **Step 8: Test migration**

```bash
vigil down 2>/dev/null  # stop old daemon
vigil up                # should migrate ~/.praefectus → ~/.vigil
```

- [ ] **Step 9: E2E test**

Send a test message via curl to verify Skills work:
```bash
curl -s -X POST http://localhost:8000/api/vigil/chat \
  -H "Content-Type: application/json" \
  -d '{"message":"what is 2+2"}' --max-time 120
```

- [ ] **Step 10: Commit any fixes**

```bash
git add -A
git commit -m "chore: final cleanup for Vigil Skills & rebrand"
```
