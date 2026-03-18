# CLAUDE.md -- Instructions for Claude Code sessions

## Build and Run

```bash
npm install              # Install all workspace dependencies
npm run build            # Build all workspaces (Turborepo)
npm run dev              # Start dev servers (Vigil daemon + Next.js)
npm test                 # Run all tests across workspaces
```

## Project Structure

Turborepo monorepo with npm workspaces:

- `apps/daemon` -- Rust daemon (port 8000)
- `apps/web` -- Next.js frontend (port 3000)
- `packages/shared` -- Shared Zod schemas and types

## Key Conventions

- **ESM everywhere** -- All packages use `"type": "module"` with `.js` extensions in imports
- **TypeScript strict** -- `strict: true` in all tsconfig files
- **Vitest** -- Test framework for frontend workspaces
- **Rust (axum)** -- Backend daemon with SQLite (sqlx)
- **Zod** -- Runtime validation for all API inputs (shared schemas in `packages/shared`)
- **No Docker** -- Runs directly on macOS

## Test Commands

```bash
# All tests
npm test

# Daemon tests only
cd apps/daemon && cargo test

# Web tests only
cd apps/web && npx vitest run
```

## Key Files

### Daemon
- `apps/daemon/src/lib.rs` -- Library crate root
- `apps/daemon/src/config.rs` -- Config resolution (`Config::resolve()`)
- `apps/daemon/src/db/` -- SQLite schema and client
- `apps/daemon/src/api/` -- REST endpoints (sessions, projects, events, notifications, pipelines, settings, vigil)
- `apps/daemon/src/services/` -- Business logic (session-manager, agent-spawner, pipeline-service, event-bus, vigil-manager, notifier, cleanup, recovery)
- `apps/daemon/src/hooks/` -- Claude Code hook installation
- `apps/daemon/src/process/` -- PTY management, output capture

### Shared
- `packages/shared/src/index.ts` -- Zod schemas: `HookPayload`, `CreateSessionInput`, session types, event types

## API Endpoints

- `GET /health` -- Health check
- `GET /api/sessions` -- List sessions
- `GET /api/sessions/:id` -- Get session
- `POST /api/sessions` -- Create session (queued)
- `DELETE /api/sessions/:id` -- Cancel session
- `GET /api/projects` -- List projects
- `POST /api/projects` -- Register project
- `DELETE /api/projects/:path` -- Unregister project
- `POST /events` -- Hook event ingestion
- `GET /api/notifications` -- List notifications
- `PATCH /api/notifications/:id/read` -- Mark notification read
- `GET /api/pipelines` -- List pipelines
- `GET /api/pipelines/:id` -- Get pipeline
- `POST /api/pipelines` -- Create pipeline
- `PUT /api/pipelines/:id` -- Update pipeline
- `DELETE /api/pipelines/:id` -- Delete pipeline
- `GET /api/settings/telegram` -- Get Telegram settings
- `PUT /api/settings/telegram` -- Save Telegram settings
- `WS /ws/dashboard` -- Real-time session updates
- `WS /ws/terminal/:sessionId` -- Terminal proxy

## Testing Patterns

- Daemon tests use isolated temp directories with in-memory SQLite
- Use `Config::for_testing(base)` for test configs
- E2E tests in `apps/daemon/src/e2e/` test full API lifecycle
- **Bug fixes MUST include a regression test** -- see `.claude/skills/bug-driven-testing.md`

## Code Quality

- **MANDATORY PRE-COMMIT CHECKS** -- Before EVERY git commit, you MUST run: `npx biome check --write .` then `npm run build` then `npm test`. NO EXCEPTIONS. See `.claude/skills/pre-commit-checks.md`
- **Biome** -- Linter and formatter. Always run `npx biome check --write .` before committing.
- **SOLID principles, clean code, TDD** -- see `.claude/skills/code-quality-standards.md`
- **Bug-driven testing** -- every bug fix starts with a failing test. See `.claude/skills/bug-driven-testing.md`
