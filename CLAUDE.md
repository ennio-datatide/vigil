# CLAUDE.md -- Instructions for Claude Code sessions

## Build and Run

```bash
npm install              # Install all workspace dependencies
npm run build            # Build all workspaces (Turborepo)
npm run dev              # Start dev servers (Fastify + Next.js)
npm test                 # Run all tests across workspaces
```

## Project Structure

Turborepo monorepo with npm workspaces:

- `apps/server` -- Fastify 5 backend (port 4000)
- `apps/web` -- Next.js 15 frontend (port 3000)
- `packages/shared` -- Shared Zod schemas and types
- `cli` -- Commander.js CLI (`praefectus` binary)

## Key Conventions

- **ESM everywhere** -- All packages use `"type": "module"` with `.js` extensions in imports
- **TypeScript strict** -- `strict: true` in all tsconfig files
- **Vitest** -- Test framework for all workspaces
- **Drizzle ORM** -- Database layer with better-sqlite3 (SQLite)
- **Fastify 5** -- Server framework with plugin architecture
- **Zod** -- Runtime validation for all API inputs (shared schemas in `packages/shared`)
- **No Docker** -- Runs directly on macOS

## Test Commands

```bash
# All tests
npm test

# Server tests only
cd apps/server && npx vitest run

# Web tests only
cd apps/web && npx vitest run

# CLI tests only
cd cli && npx vitest run

# Watch mode (server)
cd apps/server && npx vitest
```

## Key Files

### Server
- `apps/server/src/app.ts` -- Fastify app builder (`buildApp()`)
- `apps/server/src/config.ts` -- Config resolution (`resolveConfig()`)
- `apps/server/src/db/schema.ts` -- Drizzle schema (sessions, events, projects, pipelines, notifications)
- `apps/server/src/db/client.ts` -- SQLite database client
- `apps/server/src/routes/` -- REST endpoints (sessions, projects, events, notifications, skills, pipelines, settings, fs)
- `apps/server/src/services/` -- Business logic (session-manager, agent-spawner, pipeline-service, event-bus, skill-manager, worktree-manager, pty-manager, output-manager, notifier, settings-service, recovery, cleanup)
- `apps/server/src/ws/` -- WebSocket routes (dashboard, terminal)
- `apps/server/src/hooks/` -- Claude Code hook scripts

### Shared
- `packages/shared/src/index.ts` -- Zod schemas: `HookPayload`, `CreateSessionInput`, session types, event types

### CLI
- `cli/src/index.ts` -- Entry point with Commander.js program
- `cli/src/commands/` -- Command implementations (up, down, start, ls, auth, status, cleanup)

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
- `GET /api/skills` -- List skills
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

- Tests use `buildApp({ praefectusHome: '/tmp/pf-test-...' })` for isolated in-memory SQLite
- Routes are registered manually in tests via `app.register()`
- Use `app.inject()` for HTTP testing (no actual server needed)
- E2E tests in `apps/server/src/e2e/` test full API lifecycle
