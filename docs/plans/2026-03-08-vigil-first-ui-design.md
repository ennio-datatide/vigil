# Vigil-First UI Redesign

## Overview

Shift the Praefectus dashboard from a session-centric grid to a conversational orchestrator. Vigil becomes the primary interface — the user talks to Vigil, Vigil manages sessions, and a monitoring panel shows what's happening. The old TypeScript backend (`apps/server/`, `cli/`) is fully removed and replaced by the Rust daemon.

## Core Interaction Model

```
User ↔ Vigil Chat ↔ Session Orchestration ↔ Terminal (escape hatch)
```

1. **Chat with Vigil** — give tasks, ask questions, get updates
2. **Vigil spawns/manages sessions** — autonomously, using existing spawn/session APIs
3. **Vigil surfaces blockers** — "Session X needs input" appears as an inline card with reply field
4. **Vigil reports results** — summarizes outcomes, decommissions finished sessions
5. **Escalation** — if user doesn't respond to a blocker within 2 minutes, Vigil sends a Telegram notification

The user rarely needs to touch sessions directly. Terminal access is preserved as an escape hatch for debugging and direct control.

## Architecture

### Single Global Vigil

The current backend has per-project Vigils. This redesign unifies them into a **single global orchestrator** that holds context for all active projects and routes tasks to the right project. One chat, one Vigil.

### Layout: Two Modes

**Idle mode** (no active sessions):
- Vigil chat is full-width
- User gives tasks, reviews acta, asks questions

**Active mode** (sessions running):
- Left (~55%): Vigil chat
- Right (~45%): Session monitoring panel, slides in from the right
- Panel collapses back when all sessions finish

### Sidebar

Thin icon strip (no labels by default):
- **Vigil** (message icon) — home, default view
- **History** (clock) — past sessions, same as current
- **Projects** (folder) — register/unregister, shows per-project memory count + acta status
- **Pipelines** (nodes) — same visual editor
- **Settings** (gear) — Telegram config, memory decay settings

## Vigil Chat Panel

### Message Types

**User messages:** Right-aligned bubbles, standard text with markdown support.

**Vigil messages:** Left-aligned bubbles with markdown rendering. Can contain embedded cards:

- **Blocker card** — yellow border. Shows session name + the pending question. Has:
  - Inline text input for quick reply
  - "Open terminal" button (escape hatch)
  - Vigil passes the reply to the session via `POST /api/sessions/{id}/resume`

- **Status card** — neutral border. "Spawned 3 workers for feature X" with mini status dots per child session.

- **Completion card** — green border. "Session X finished." with collapsible summary/diff.

- **Acta card** — collapsible markdown block showing the project briefing.

### Input

- Text input at bottom with send button
- Multi-line: Shift+Enter
- Typing indicator when Vigil is processing

### Persistence

- Chat history persists across page reloads (stored in redb or a new SQLite table)
- Scrollable history

## Session Monitor Panel (Right Side)

### Header

Compact KPI strip: "3 active · 1 blocked · 14 completed"

### Session List

Scrollable, sorted by status priority:
1. Blocked (needs_input, auth_required) — top, yellow/orange accent
2. Running — green dot
3. Queued — blue dot

Each row:
- Status dot
- Truncated prompt
- Project name
- Duration (monospace)
- Parent sessions show child count badge

### Hierarchy

Sessions with `parentId` are nested under their parent — indented with a collapsible tree. Click a parent to expand/collapse children.

### Click Behavior

Click any session → terminal opens in a full-screen overlay.

### Adaptive Animation

- Panel slides in from right when first session spawns
- Collapses when last session finishes
- Framer Motion spring animation

## Terminal Overlay

- **Triggers:** Click session in monitor panel, or "Open terminal" on a blocker card
- Full-screen overlay with back button → returns to Vigil chat
- Same xterm.js + WebGL terminal as current implementation
- Header: session status, prompt, git metadata badges

## Vigil Escalation via Telegram

When a session enters `needs_input` or `auth_required`:

1. Vigil starts a **2-minute timer**
2. If the user responds in chat within 2 minutes → timer cancelled, no Telegram
3. If timer expires → Vigil sends a Telegram notification:
   - Session name/prompt
   - The question needing an answer
   - Deep link: `https://<dashboard_url>/dashboard?blocker=<sessionId>`
4. Uses existing Telegram integration (bot token + chat ID from settings)

Backend: cancellable `tokio::time::sleep` task per blocker. Checks resolution status before sending.

## Backend Changes

### Unify Vigil (Global Orchestrator)

- Remove `projectPath` requirement from `POST /api/vigil/chat`
- Single Vigil instance manages all projects
- Vigil internally routes to correct project context based on conversation

### Chat History Persistence

- New endpoint: `GET /api/vigil/history` — returns past messages
- Storage: redb or new `vigil_messages` SQLite table
- Fields: `id`, `role` (user/vigil), `content`, `embedded_cards`, `created_at`

### Blocker Escalation Timer

- New service: `EscalationService` — manages per-blocker timers
- On `StatusChanged { newStatus: needs_input }` → start 2-min timer
- On session resume → cancel timer
- On timer expiry → send Telegram via existing notifier

### Vigil Session Spawning

- Vigil calls existing `POST /api/sessions` and `POST /api/sessions/{id}/spawn` internally
- No new endpoints — Vigil uses rig-core tools that call the session APIs

## Frontend Changes

### Removed

- Session grid page (current `/dashboard`)
- New Session modal/FAB
- Standalone KPI bar component
- `apps/server/` — entire TypeScript Fastify backend
- `cli/` — TypeScript CLI (replaced by Rust `apps/daemon` CLI)

### Added

- `VigilChat` component — main chat interface with message bubbles + embedded cards
- `BlockerCard` component — inline blocker with reply input + terminal button
- `StatusCard`, `CompletionCard`, `ActaCard` — embedded chat cards
- `SessionMonitor` component — right-side adaptive panel
- `SessionTree` component — hierarchical session list with parent/child nesting
- `TerminalOverlay` component — full-screen terminal with back navigation
- `useVigilWs` hook — WebSocket integration for Vigil events
- `useEscalationTimer` — client-side timer display (countdown before Telegram fires)

### Modified

- `layout.tsx` — new sidebar items, remove FAB, adaptive layout
- `use-dashboard-ws.ts` — handle new events (child_spawned, child_completed, memory_updated, acta_refreshed, status_changed)
- `session-store.ts` — add parent/child relationships, tree structure helpers
- Projects page — show memory count + acta status per project
- Settings page — add escalation timeout config (default 2 min)
- `api.ts` — add Vigil chat, history, memory endpoints

### Kept As-Is

- History page (minor: add sub-session info to rows)
- Pipeline editor
- Terminal implementation (xterm.js + WebGL)
- Design system (colors, glassmorphism, Framer Motion)
- Auth page

## Data Flow

```
User types in Vigil chat
  → POST /api/vigil/chat { message }
  → Vigil processes (LLM + tools)
  → Response includes text + optional embedded cards
  → WebSocket pushes real-time updates:
     - session_update (session state changes)
     - child_spawned / child_completed (hierarchy updates)
     - status_changed (triggers blocker cards + escalation timers)
     - memory_updated / acta_refreshed (knowledge base changes)
  → UI renders chat messages + updates session monitor panel
```

## Migration: Remove Old Backend

### Delete

- `apps/server/` — entire directory (Fastify backend)
- `cli/` — entire directory (Commander.js CLI)
- Root-level configs referencing old workspaces

### Move

- `apps/daemon/` becomes the sole backend
- Update `package.json` workspaces to remove `apps/server` and `cli`
- Update Turborepo config (`turbo.json`) accordingly

### Keep

- `apps/web/` — Next.js frontend (modified per this design)
- `packages/shared/` — updated with generated API types from Rust OpenAPI spec
- `docs/` — plans and documentation
