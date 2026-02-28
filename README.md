# Praefectus

Self-hosted mission control for orchestrating [Claude Code](https://claude.ai/code) agents across git worktrees.

Configure tasks, and the system handles agent spawning in isolated worktrees with the right skills, monitors progress via a real-time dashboard, and alerts you when agents need input.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Install

### Quick Install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/ennio-datatide/praefectus/main/install.sh | bash
```

### Homebrew

```bash
brew tap ennio-datatide/praefectus
brew install praefectus
```

### npm

```bash
npm install -g praefectus
```

### Prerequisites

- **Node.js 22+** and npm
- **tmux** — agent sessions run inside tmux
- **Claude Code CLI** (`claude`) — installed and authenticated
- **macOS or Linux** — see [Windows](#windows) for WSL instructions

## Quick Start

```bash
# Start the server and web dashboard
praefectus up

# Open the dashboard
open http://localhost:3000

# Spawn an agent session
praefectus start /path/to/project "Implement the login page"

# List active sessions
praefectus ls

# Stop everything
praefectus down
```

## Features

- **Agent Orchestration** — spawn multiple Claude Code agents with different roles (implementer, reviewer, fixer)
- **Git Worktree Isolation** — each agent works in its own worktree to avoid conflicts
- **Real-time Dashboard** — web UI with live session monitoring via WebSocket
- **Mobile Terminal** — attach to agent tmux sessions from your phone via xterm.js
- **Skill System** — define agent behavior via skill files (`.claude/skills/`)
- **Event Hooks** — Claude Code hook integration for real-time event streaming
- **Telegram Notifications** — alerts when agents need input or complete tasks
- **Session Recovery** — automatically recovers interrupted sessions
- **Pipeline Workflows** — configurable multi-step agent workflows with visual editor
- **Project Management** — register projects, track sessions across repos

## CLI Reference

| Command | Description |
|---------|-------------|
| `praefectus up [--daemon]` | Start the server and web dashboard |
| `praefectus down` | Stop the server |
| `praefectus start <project> <prompt>` | Spawn a new agent session |
| `praefectus ls [--all]` | List sessions (active by default) |
| `praefectus status` | Show server status |
| `praefectus auth [claude\|codex]` | Manage authentication |
| `praefectus cleanup` | Remove old worktrees from completed sessions |

## Architecture

```
praefectus/
  apps/
    server/    Fastify 5 backend (SQLite, node-pty, WebSocket)
    web/       Next.js 15 dashboard (React 19, Tailwind, xterm.js)
  packages/
    shared/    Shared Zod schemas and TypeScript types
  cli/         CLI tool (praefectus command)
```

- **Server** — Fastify 5 with better-sqlite3 + Drizzle ORM, node-pty for terminal management, WebSocket for real-time updates
- **Web** — Next.js 15 with React 19, Zustand for state, TanStack Query for data fetching, xterm.js for terminal rendering
- **Shared** — Zod schemas for API payloads and session types
- **CLI** — Commander.js CLI for managing the server and sessions

## Development

### Setup

```bash
git clone https://github.com/ennio-datatide/praefectus.git
cd praefectus
npm install
```

### Build & Run

```bash
npm run dev          # Start dev servers (Fastify + Next.js)
npm run build        # Build all workspaces via Turborepo
npm test             # Run all tests
```

### Test

```bash
npm test                                    # All tests
cd apps/server && npx vitest run            # Server tests only
cd cli && npx vitest run                    # CLI tests only
```

### Project Structure

```
apps/server/src/
  app.ts              Fastify app builder
  config.ts           Configuration resolution
  index.ts            Server entry point
  db/                 Drizzle schema + SQLite client
  routes/             REST API (sessions, projects, events, notifications, skills, pipelines, settings)
  services/           Business logic (session-manager, agent-spawner, pipeline-service, etc.)
  ws/                 WebSocket handlers (dashboard, terminal)
  hooks/              Claude Code hook scripts

apps/web/src/
  app/                Next.js app router pages
  components/         React components
  lib/                API client, stores, hooks

packages/shared/src/
  index.ts            Shared Zod schemas and types

cli/src/
  index.ts            CLI entry point
  commands/           Command implementations
```

## Windows

Praefectus relies on tmux and node-pty, which require a Unix environment. On Windows, use **WSL 2**:

1. [Install WSL 2](https://learn.microsoft.com/en-us/windows/wsl/install)
2. Open your WSL terminal
3. Follow the Linux installation instructions above

Native Windows is not supported.

## License

[MIT](LICENSE)
