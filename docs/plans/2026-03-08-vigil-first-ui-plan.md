# Vigil-First UI Redesign — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rebuild the Praefectus dashboard around Vigil as the primary conversational interface, remove the old TypeScript backend, and add Telegram escalation for unanswered blockers.

**Architecture:** Vigil chat on the left (full-width when idle, 55% when sessions active), adaptive session monitor panel slides in from the right when sessions are running. Terminal access via overlay. Old `apps/server/` and `cli/` directories deleted entirely.

**Tech Stack:** Next.js 16, React 19, Tailwind v4, Framer Motion, Zustand, TanStack React Query, xterm.js 6, Rust/Axum backend (already built at `apps/daemon/`)

---

## Phase 0: Remove Old Backend & Update Workspace

### Task 0.1: Delete Old TypeScript Backend and CLI

**Files:**
- Delete: `apps/server/` (entire directory)
- Delete: `cli/` (entire directory)

**Step 1: Delete the directories**

```bash
rm -rf apps/server cli
```

**Step 2: Commit**

```bash
git add -A && git commit -m "chore: remove old TypeScript backend and CLI (replaced by Rust daemon)"
```

---

### Task 0.2: Update Workspace Configuration

**Files:**
- Modify: `package.json` (root)
- Modify: `turbo.json`

**Step 1: Update root package.json**

Remove `cli` from workspaces. Update scripts to remove references to old backend. Add daemon scripts.

```json
{
  "workspaces": ["apps/*", "packages/*"],
  "scripts": {
    "dev": "turbo dev",
    "dev:web": "cd apps/web && npm run dev",
    "dev:daemon": "cd apps/daemon && cargo run -- daemon",
    "build": "turbo build",
    "build:daemon": "cd apps/daemon && cargo build --release",
    "test": "turbo test",
    "test:daemon": "cd apps/daemon && cargo test",
    "lint:daemon": "cd apps/daemon && cargo clippy -- -D warnings",
    "format": "biome format --write .",
    "check": "biome check .",
    "lint": "turbo lint",
    "generate:types": "openapi-typescript apps/daemon/openapi.json -o packages/shared/src/api-types.ts"
  }
}
```

**Step 2: Verify build still works**

```bash
npm install && npm run build
```

Expected: Only `apps/web` and `packages/shared` build. No errors about missing `apps/server` or `cli`.

**Step 3: Commit**

```bash
git add package.json turbo.json package-lock.json && git commit -m "chore: update workspace config for Rust-only backend"
```

---

## Phase 1: Backend — Global Vigil, Chat History, Escalation

All backend changes are in the Rust daemon at `apps/daemon/`.

### Task 1.1: Add Chat History to Vigil

**Files:**
- Create: `apps/daemon/src/services/vigil_chat.rs`
- Modify: `apps/daemon/src/services.rs` (add module)
- Modify: `apps/daemon/src/db/migrations/` (new migration for vigil_messages table)

**Step 1: Write the failing test**

In `apps/daemon/src/services/vigil_chat.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn save_and_list_messages() {
        let (db, _dir) = test_db().await;
        let store = VigilChatStore::new(Arc::clone(&db));

        store.save_message("user", "Hello Vigil", None).await.unwrap();
        store.save_message("vigil", "Hello! How can I help?", None).await.unwrap();

        let messages = store.list_messages(50, 0).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "vigil");
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cd apps/daemon && cargo test vigil_chat -- --nocapture
```

Expected: FAIL — module doesn't exist.

**Step 3: Write implementation**

Create `vigil_chat.rs` with:

```rust
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::db::sqlite::SqliteDb;
use crate::error::{DbError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VigilMessage {
    pub id: i64,
    pub role: String,           // "user" or "vigil"
    pub content: String,
    pub embedded_cards: Option<String>, // JSON array of card data
    pub created_at: i64,        // Unix ms
}

pub(crate) struct VigilChatStore {
    db: Arc<SqliteDb>,
}

impl VigilChatStore {
    pub(crate) fn new(db: Arc<SqliteDb>) -> Self { Self { db } }

    pub(crate) async fn save_message(
        &self,
        role: &str,
        content: &str,
        embedded_cards: Option<&str>,
    ) -> Result<VigilMessage> {
        let now = chrono::Utc::now().timestamp_millis();
        let id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO vigil_messages (role, content, embedded_cards, created_at) \
             VALUES (?, ?, ?, ?) RETURNING id"
        )
        .bind(role).bind(content).bind(embedded_cards).bind(now)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(VigilMessage { id, role: role.to_owned(), content: content.to_owned(),
            embedded_cards: embedded_cards.map(String::from), created_at: now })
    }

    pub(crate) async fn list_messages(&self, limit: i64, offset: i64) -> Result<Vec<VigilMessage>> {
        let rows = sqlx::query(
            "SELECT id, role, content, embedded_cards, created_at \
             FROM vigil_messages ORDER BY id ASC LIMIT ? OFFSET ?"
        )
        .bind(limit).bind(offset)
        .fetch_all(self.db.pool())
        .await
        .map_err(DbError::from)?;

        Ok(rows.iter().map(|r| VigilMessage {
            id: r.get("id"),
            role: r.get("role"),
            content: r.get("content"),
            embedded_cards: r.get("embedded_cards"),
            created_at: r.get("created_at"),
        }).collect())
    }

    pub(crate) async fn clear(&self) -> Result<()> {
        sqlx::query("DELETE FROM vigil_messages")
            .execute(self.db.pool()).await.map_err(DbError::from)?;
        Ok(())
    }
}
```

Add SQL migration:

```sql
CREATE TABLE IF NOT EXISTS vigil_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    embedded_cards TEXT,
    created_at INTEGER NOT NULL
);
```

**Step 4: Run test to verify it passes**

```bash
cd apps/daemon && cargo test vigil_chat -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: add Vigil chat history persistence (vigil_chat store)"
```

---

### Task 1.2: Unify Vigil to Global (Remove Project Scoping from Chat)

**Files:**
- Modify: `apps/daemon/src/api/vigil.rs`
- Modify: `apps/daemon/src/services/vigil.rs`
- Modify: `apps/daemon/src/deps.rs`

**Step 1: Write the failing test**

In `apps/daemon/src/api/vigil.rs` tests, add:

```rust
#[tokio::test]
async fn chat_without_project_path() {
    let app = test_app().await;
    let resp = app.post("/api/vigil/chat")
        .json(&json!({ "message": "What projects am I working on?" }))
        .send().await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await;
    assert!(body["response"].is_string());
}

#[tokio::test]
async fn chat_history_persists() {
    let app = test_app().await;
    app.post("/api/vigil/chat")
        .json(&json!({ "message": "Hello" }))
        .send().await;

    let resp = app.get("/api/vigil/history").send().await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await;
    let messages = body["messages"].as_array().unwrap();
    assert!(messages.len() >= 2); // user + vigil response
}
```

**Step 2: Run test to verify it fails**

```bash
cd apps/daemon && cargo test chat_without_project -- --nocapture
```

**Step 3: Update ChatInput and add history endpoint**

In `api/vigil.rs`:
- Remove `project_path` from `ChatInput` (make it optional or remove entirely)
- Add `GET /api/vigil/history` handler that returns `VigilChatStore::list_messages()`
- Update `chat()` handler to save user message + response to `VigilChatStore`

In `deps.rs`:
- Add `vigil_chat_store: VigilChatStore` to `AppDeps`

**Step 4: Run test to verify it passes**

```bash
cd apps/daemon && cargo test vigil -- --nocapture
```

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: unify Vigil to global orchestrator, add chat history endpoint"
```

---

### Task 1.3: Blocker Escalation Timer Service

**Files:**
- Create: `apps/daemon/src/services/escalation.rs`
- Modify: `apps/daemon/src/services.rs` (add module)
- Modify: `apps/daemon/src/deps.rs` (add to AppDeps)

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn escalation_fires_after_timeout() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));
        let service = EscalationService::new(
            Arc::clone(&db),
            event_bus.clone(),
            Duration::from_millis(100), // 100ms for testing
        );

        service.start_timer("session-1").await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should have emitted an escalation event
        assert!(service.was_escalated("session-1").await);
    }

    #[tokio::test]
    async fn escalation_cancelled_on_resolve() {
        let (db, _dir) = test_db().await;
        let event_bus = Arc::new(EventBus::new(64));
        let service = EscalationService::new(
            Arc::clone(&db),
            event_bus.clone(),
            Duration::from_millis(200),
        );

        service.start_timer("session-1").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        service.cancel_timer("session-1").await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(!service.was_escalated("session-1").await);
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cd apps/daemon && cargo test escalation -- --nocapture
```

**Step 3: Write implementation**

```rust
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::db::sqlite::SqliteDb;
use crate::events::{AppEvent, EventBus};

const DEFAULT_ESCALATION_TIMEOUT: Duration = Duration::from_secs(120); // 2 minutes

pub(crate) struct EscalationService {
    db: Arc<SqliteDb>,
    event_bus: Arc<EventBus>,
    timeout: Duration,
    timers: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    escalated: Arc<Mutex<Vec<String>>>, // for testing
}

impl EscalationService {
    pub(crate) fn new(db: Arc<SqliteDb>, event_bus: Arc<EventBus>, timeout: Duration) -> Self {
        Self {
            db, event_bus, timeout,
            timers: Arc::new(Mutex::new(HashMap::new())),
            escalated: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn default_timeout(db: Arc<SqliteDb>, event_bus: Arc<EventBus>) -> Self {
        Self::new(db, event_bus, DEFAULT_ESCALATION_TIMEOUT)
    }

    /// Start escalation timer for a session entering needs_input/auth_required.
    pub(crate) async fn start_timer(&self, session_id: &str) {
        let sid = session_id.to_owned();
        let timeout = self.timeout;
        let db = Arc::clone(&self.db);
        let event_bus = Arc::clone(&self.event_bus);
        let escalated = Arc::clone(&self.escalated);

        let handle = tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            // Timer expired — send Telegram escalation
            escalated.lock().await.push(sid.clone());
            let _ = event_bus.emit(AppEvent::EscalationTriggered {
                session_id: sid,
            });
        });

        self.timers.lock().await.insert(session_id.to_owned(), handle);
    }

    /// Cancel escalation timer (user responded in time).
    pub(crate) async fn cancel_timer(&self, session_id: &str) {
        if let Some(handle) = self.timers.lock().await.remove(session_id) {
            handle.abort();
        }
    }

    /// Check if a session was escalated (for testing).
    pub(crate) async fn was_escalated(&self, session_id: &str) -> bool {
        self.escalated.lock().await.contains(&session_id.to_owned())
    }
}
```

Add `EscalationTriggered { session_id: String }` variant to `AppEvent` in `events.rs`.

Wire into `TelegramNotifier` — on `EscalationTriggered`, load session prompt + pending question, send Telegram with deep link.

**Step 4: Run tests**

```bash
cd apps/daemon && cargo test escalation -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: add blocker escalation timer service (2-min Telegram fallback)"
```

---

### Task 1.4: Wire Escalation into Session Status Flow

**Files:**
- Modify: `apps/daemon/src/services/session_manager.rs`
- Modify: `apps/daemon/src/services/notifier.rs`

**Step 1: Write the failing test**

In `session_manager.rs` tests:

```rust
#[tokio::test]
async fn needs_input_starts_escalation_timer() {
    // Create session, transition to needs_input
    // Verify escalation timer was started
}

#[tokio::test]
async fn resume_cancels_escalation_timer() {
    // Create session, transition to needs_input, then resume
    // Verify timer was cancelled
}
```

**Step 2: Run test to verify fails**

**Step 3: In SessionManager's StatusChanged handler:**
- When new_status is `NeedsInput` or `AuthRequired` → call `escalation.start_timer(session_id)`
- When status transitions FROM `NeedsInput`/`AuthRequired` to `Running` → call `escalation.cancel_timer(session_id)`

In `notifier.rs`:
- Handle `EscalationTriggered` event — load session, format Telegram message with deep link, send

**Step 4: Run tests**

```bash
cd apps/daemon && cargo test escalation -- --nocapture
cd apps/daemon && cargo test session_manager -- --nocapture
```

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: wire escalation timers into session status flow"
```

---

### Task 1.5: Update OpenAPI Spec

**Files:**
- Modify: `apps/daemon/openapi.json`

**Step 1: Add new endpoints to OpenAPI spec**

- `GET /api/vigil/history` — returns `{ messages: VigilMessage[] }`
- Update `POST /api/vigil/chat` — `projectPath` now optional
- Add `VigilMessage` schema

**Step 2: Regenerate TypeScript types**

```bash
npx openapi-typescript apps/daemon/openapi.json -o packages/shared/src/api-types.ts
```

**Step 3: Verify types compile**

```bash
cd packages/shared && npx tsc --noEmit
```

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: update OpenAPI spec with chat history and global Vigil endpoints"
```

---

## Phase 2: Frontend — Types and API Layer

### Task 2.1: Add New Types for Vigil Chat

**Files:**
- Modify: `apps/web/src/lib/types.ts`

**Step 1: Add Vigil-specific types**

```typescript
// Vigil chat types
export interface VigilMessage {
  id: number;
  role: 'user' | 'vigil';
  content: string;
  embeddedCards: EmbeddedCard[] | null;
  createdAt: number;
}

export type EmbeddedCardType = 'blocker' | 'status' | 'completion' | 'acta';

export interface EmbeddedCard {
  type: EmbeddedCardType;
  sessionId?: string;
  sessionPrompt?: string;
  question?: string;     // for blocker cards
  summary?: string;      // for completion cards
  childCount?: number;   // for status cards
  acta?: string;         // for acta cards
}

// Extended WS message types
export type WsMessageExtended = WsMessage
  | { type: 'child_spawned'; parentId: string; childId: string }
  | { type: 'child_completed'; parentId: string; childId: string; success: boolean }
  | { type: 'status_changed'; sessionId: string; oldStatus: string; newStatus: string }
  | { type: 'memory_updated'; memoryId: string }
  | { type: 'acta_refreshed'; projectPath: string }
  | { type: 'escalation_warning'; sessionId: string; secondsRemaining: number };
```

**Step 2: Commit**

```bash
git add apps/web/src/lib/types.ts && git commit -m "feat: add Vigil chat and extended WS message types"
```

---

### Task 2.2: Add Vigil API Hooks

**Files:**
- Modify: `apps/web/src/lib/api.ts`

**Step 1: Add hooks for Vigil chat and history**

```typescript
// Vigil hooks

export function useVigilHistoryQuery() {
  return useQuery({
    queryKey: ['vigil-history'],
    queryFn: () => fetchJson<{ messages: VigilMessage[] }>('/api/vigil/history'),
    refetchOnMount: 'always',
  });
}

export function useVigilChat() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (message: string) =>
      fetchJson<{ response: string }>('/api/vigil/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message }),
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['vigil-history'] }),
  });
}

export function useVigilStatusQuery() {
  return useQuery({
    queryKey: ['vigil-status'],
    queryFn: () => fetchJson<{ activeProjects: string[] }>('/api/vigil/status'),
    refetchInterval: 10_000,
  });
}

export function useSessionChildrenQuery(parentId: string) {
  return useQuery({
    queryKey: ['session-children', parentId],
    queryFn: () => fetchJson<Session[]>(`/api/sessions/${parentId}/children`),
    enabled: !!parentId,
  });
}
```

Import `VigilMessage` from `./types`.

**Step 2: Commit**

```bash
git add apps/web/src/lib/api.ts && git commit -m "feat: add Vigil chat, history, and children API hooks"
```

---

### Task 2.3: Add Vigil Chat Store

**Files:**
- Create: `apps/web/src/lib/stores/vigil-store.ts`

**Step 1: Create the store**

```typescript
import { create } from 'zustand';
import type { VigilMessage } from '../types';

interface VigilState {
  messages: VigilMessage[];
  isProcessing: boolean;
  setMessages: (messages: VigilMessage[]) => void;
  addMessage: (message: VigilMessage) => void;
  setProcessing: (processing: boolean) => void;
}

export const useVigilStore = create<VigilState>((set) => ({
  messages: [],
  isProcessing: false,
  setMessages: (messages) => set({ messages }),
  addMessage: (message) =>
    set((state) => ({ messages: [...state.messages, message] })),
  setProcessing: (processing) => set({ isProcessing: processing }),
}));
```

**Step 2: Commit**

```bash
git add apps/web/src/lib/stores/vigil-store.ts && git commit -m "feat: add Vigil chat Zustand store"
```

---

### Task 2.4: Extend Dashboard WebSocket Hook

**Files:**
- Modify: `apps/web/src/lib/hooks/use-dashboard-ws.ts`

**Step 1: Handle new WebSocket event types**

Update the `ws.onmessage` handler to process:
- `child_spawned` → update parent session's child count in store
- `child_completed` → update parent + child in store
- `status_changed` → update session status (triggers blocker card in Vigil chat)
- `memory_updated` → invalidate memory queries (optional)
- `acta_refreshed` → invalidate acta queries (optional)

Add a callback prop or event emitter so the Vigil chat component can react to `status_changed` events to display blocker cards.

**Step 2: Commit**

```bash
git add apps/web/src/lib/hooks/use-dashboard-ws.ts && git commit -m "feat: extend dashboard WS hook for child/status/memory events"
```

---

## Phase 3: Frontend — Layout Restructure

### Task 3.1: Redesign Dashboard Layout

**Files:**
- Modify: `apps/web/src/app/dashboard/layout.tsx`

**Step 1: Update NAV_ITEMS**

Replace the first nav item (Dashboard grid icon) with Vigil (message bubble icon):

```typescript
const NAV_ITEMS = [
  {
    href: '/dashboard',
    label: 'Vigil',
    icon: (/* message bubble SVG */),
    match: 'exact' as const,
  },
  // ... keep History, Projects, Pipelines, Settings
];
```

**Step 2: Remove NewSession component**

Remove `<NewSession />` from the layout. Users will start sessions via Vigil chat.

**Step 3: Commit**

```bash
git add apps/web/src/app/dashboard/layout.tsx && git commit -m "refactor: update sidebar nav — Vigil replaces Dashboard, remove NewSession FAB"
```

---

### Task 3.2: Rewrite Dashboard Main Page

**Files:**
- Modify: `apps/web/src/app/dashboard/page.tsx`

**Step 1: Replace session grid with Vigil chat + adaptive monitor**

The page becomes the Vigil-first layout:

```typescript
'use client';
import { useState } from 'react';
import { VigilChat } from '@/components/vigil/vigil-chat';
import { SessionMonitor } from '@/components/vigil/session-monitor';
import { useSessionStore } from '@/lib/stores/session-store';
import { useDashboardWs } from '@/lib/hooks/use-dashboard-ws';
import { AnimatePresence, motion } from 'framer-motion';

export default function DashboardPage() {
  useDashboardWs();

  const sessions = useSessionStore((s) => Object.values(s.sessions));
  const activeSessions = sessions.filter((s) =>
    ['queued', 'running', 'needs_input', 'auth_required'].includes(s.status)
  );
  const hasActiveSessions = activeSessions.length > 0;

  return (
    <div className="flex h-full">
      {/* Vigil Chat — full width when idle, 55% when sessions active */}
      <div className={`flex-1 transition-all duration-300 ${hasActiveSessions ? 'md:w-[55%] md:flex-none' : ''}`}>
        <VigilChat />
      </div>

      {/* Session Monitor — slides in from right */}
      <AnimatePresence>
        {hasActiveSessions && (
          <motion.div
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: '45%', opacity: 1 }}
            exit={{ width: 0, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 300, damping: 30 }}
            className="hidden md:block border-l border-border-subtle overflow-hidden"
          >
            <SessionMonitor />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
```

**Step 2: Commit**

```bash
git add apps/web/src/app/dashboard/page.tsx && git commit -m "feat: replace session grid with Vigil chat + adaptive session monitor"
```

---

## Phase 4: Frontend — Vigil Chat Component

### Task 4.1: Create VigilChat Component

**Files:**
- Create: `apps/web/src/components/vigil/vigil-chat.tsx`

**Step 1: Build the chat shell**

```typescript
'use client';
import { useEffect, useRef, useState } from 'react';
import { motion } from 'framer-motion';
import { useVigilHistoryQuery, useVigilChat } from '@/lib/api';
import { useVigilStore } from '@/lib/stores/vigil-store';
import { ChatMessage } from './chat-message';

export function VigilChat() {
  const [input, setInput] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);
  const { data: history } = useVigilHistoryQuery();
  const chatMutation = useVigilChat();
  const { messages, setMessages, addMessage, isProcessing, setProcessing } = useVigilStore();

  // Sync history on load
  useEffect(() => {
    if (history?.messages) setMessages(history.messages);
  }, [history, setMessages]);

  // Auto-scroll on new messages
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' });
  }, [messages]);

  const handleSend = async () => {
    if (!input.trim() || isProcessing) return;
    const text = input.trim();
    setInput('');

    // Optimistic: add user message
    addMessage({
      id: Date.now(),
      role: 'user',
      content: text,
      embeddedCards: null,
      createdAt: Date.now(),
    });

    setProcessing(true);
    try {
      const { response } = await chatMutation.mutateAsync(text);
      addMessage({
        id: Date.now() + 1,
        role: 'vigil',
        content: response,
        embeddedCards: null,
        createdAt: Date.now(),
      });
    } finally {
      setProcessing(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="shrink-0 border-b border-border-subtle px-6 py-4">
        <h1 className="text-lg font-semibold text-text">Vigil</h1>
        <p className="text-xs text-text-muted">AI Orchestrator</p>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
        {messages.length === 0 && (
          <div className="flex h-full items-center justify-center text-text-muted text-sm">
            Start a conversation with Vigil to orchestrate your coding sessions.
          </div>
        )}
        {messages.map((msg) => (
          <ChatMessage key={msg.id} message={msg} />
        ))}
        {isProcessing && (
          <div className="flex items-center gap-2 text-text-muted text-sm">
            <span className="inline-block h-2 w-2 rounded-full bg-accent animate-pulse" />
            Vigil is thinking...
          </div>
        )}
      </div>

      {/* Input */}
      <div className="shrink-0 border-t border-border-subtle px-6 py-4">
        <div className="flex items-end gap-3">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                handleSend();
              }
            }}
            placeholder="Talk to Vigil..."
            rows={1}
            className="flex-1 resize-none rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text placeholder:text-text-muted focus-accent"
          />
          <button
            type="button"
            onClick={handleSend}
            disabled={!input.trim() || isProcessing}
            className="rounded-lg bg-accent px-4 py-3 text-sm font-medium text-white btn-press disabled:opacity-40 hover:bg-accent-hover transition-colors"
          >
            Send
          </button>
        </div>
      </div>
    </div>
  );
}
```

**Step 2: Commit**

```bash
git add apps/web/src/components/vigil/ && git commit -m "feat: add VigilChat component with message history and input"
```

---

### Task 4.2: Create ChatMessage Component

**Files:**
- Create: `apps/web/src/components/vigil/chat-message.tsx`

**Step 1: Build message bubble with embedded card support**

```typescript
'use client';
import type { VigilMessage } from '@/lib/types';
import { BlockerCard } from './cards/blocker-card';
import { StatusCard } from './cards/status-card';
import { CompletionCard } from './cards/completion-card';
import { ActaCard } from './cards/acta-card';

export function ChatMessage({ message }: { message: VigilMessage }) {
  const isUser = message.role === 'user';

  return (
    <div className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}>
      <div
        className={`max-w-[80%] rounded-xl px-4 py-3 text-sm leading-relaxed ${
          isUser
            ? 'bg-accent/15 text-text'
            : 'bg-surface-alt text-text border border-border-subtle'
        }`}
      >
        {/* Markdown content (simple for now, can add react-markdown later) */}
        <div className="whitespace-pre-wrap">{message.content}</div>

        {/* Embedded cards */}
        {message.embeddedCards?.map((card, i) => (
          <div key={i} className="mt-3">
            {card.type === 'blocker' && <BlockerCard card={card} />}
            {card.type === 'status' && <StatusCard card={card} />}
            {card.type === 'completion' && <CompletionCard card={card} />}
            {card.type === 'acta' && <ActaCard card={card} />}
          </div>
        ))}
      </div>
    </div>
  );
}
```

**Step 2: Commit**

```bash
git add apps/web/src/components/vigil/chat-message.tsx && git commit -m "feat: add ChatMessage component with embedded card support"
```

---

### Task 4.3: Create Embedded Card Components

**Files:**
- Create: `apps/web/src/components/vigil/cards/blocker-card.tsx`
- Create: `apps/web/src/components/vigil/cards/status-card.tsx`
- Create: `apps/web/src/components/vigil/cards/completion-card.tsx`
- Create: `apps/web/src/components/vigil/cards/acta-card.tsx`

**Step 1: BlockerCard** — yellow border, session name, question, inline reply input, "Open terminal" button

```typescript
'use client';
import { useState } from 'react';
import { useRouter } from 'next/navigation';
import type { EmbeddedCard } from '@/lib/types';
import { useVigilChat } from '@/lib/api';

export function BlockerCard({ card }: { card: EmbeddedCard }) {
  const [reply, setReply] = useState('');
  const router = useRouter();
  const chatMutation = useVigilChat();

  const handleReply = () => {
    if (!reply.trim()) return;
    // Send reply via Vigil — Vigil will route it to the right session
    chatMutation.mutate(`Re: ${card.sessionPrompt} — ${reply}`);
    setReply('');
  };

  return (
    <div className="rounded-lg border-l-4 border-status-needs-input bg-surface p-3">
      <div className="text-xs font-medium text-status-needs-input mb-1">Needs Input</div>
      <div className="text-sm text-text mb-2">{card.question}</div>
      <div className="flex items-center gap-2">
        <input
          value={reply}
          onChange={(e) => setReply(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleReply()}
          placeholder="Type your answer..."
          className="flex-1 rounded border border-border bg-bg px-3 py-1.5 text-xs text-text focus-accent"
        />
        <button
          type="button"
          onClick={() => router.push(`/dashboard/sessions/${card.sessionId}`)}
          className="rounded px-2 py-1.5 text-xs text-text-muted hover:text-text hover:bg-surface-hover transition-colors"
        >
          Open terminal
        </button>
      </div>
    </div>
  );
}
```

**Step 2: StatusCard** — shows spawn activity with mini dots

```typescript
import type { EmbeddedCard } from '@/lib/types';

export function StatusCard({ card }: { card: EmbeddedCard }) {
  return (
    <div className="rounded-lg border border-border-subtle bg-surface p-3">
      <div className="flex items-center gap-2 text-xs text-text-secondary">
        <span className="h-2 w-2 rounded-full bg-status-working" />
        {card.summary ?? `Spawned ${card.childCount ?? 0} sessions`}
      </div>
    </div>
  );
}
```

**Step 3: CompletionCard** — green border, summary

```typescript
import type { EmbeddedCard } from '@/lib/types';

export function CompletionCard({ card }: { card: EmbeddedCard }) {
  return (
    <div className="rounded-lg border-l-4 border-status-completed bg-surface p-3">
      <div className="text-xs font-medium text-status-completed mb-1">Completed</div>
      <div className="text-sm text-text">{card.summary}</div>
    </div>
  );
}
```

**Step 4: ActaCard** — collapsible markdown briefing

```typescript
'use client';
import { useState } from 'react';
import type { EmbeddedCard } from '@/lib/types';

export function ActaCard({ card }: { card: EmbeddedCard }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="rounded-lg border border-border-subtle bg-surface p-3">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center justify-between text-xs font-medium text-accent"
      >
        <span>Project Briefing (Acta)</span>
        <span>{expanded ? '−' : '+'}</span>
      </button>
      {expanded && (
        <div className="mt-2 whitespace-pre-wrap text-xs text-text-secondary leading-relaxed">
          {card.acta}
        </div>
      )}
    </div>
  );
}
```

**Step 5: Commit**

```bash
git add apps/web/src/components/vigil/cards/ && git commit -m "feat: add embedded card components (blocker, status, completion, acta)"
```

---

## Phase 5: Frontend — Session Monitor Panel

### Task 5.1: Create SessionMonitor Component

**Files:**
- Create: `apps/web/src/components/vigil/session-monitor.tsx`

**Step 1: Build the monitor panel**

```typescript
'use client';
import { useSessionStore } from '@/lib/stores/session-store';
import { SessionTree } from './session-tree';

const STATUS_PRIORITY: Record<string, number> = {
  needs_input: 0,
  auth_required: 1,
  running: 2,
  queued: 3,
  completed: 4,
  failed: 5,
  cancelled: 6,
  interrupted: 7,
};

export function SessionMonitor() {
  const sessions = useSessionStore((s) => Object.values(s.sessions));

  const active = sessions.filter((s) =>
    ['queued', 'running', 'needs_input', 'auth_required'].includes(s.status)
  );
  const blocked = sessions.filter((s) =>
    ['needs_input', 'auth_required'].includes(s.status)
  );
  const completed = sessions.filter((s) =>
    ['completed', 'failed', 'cancelled'].includes(s.status)
  );

  // Only show root sessions (no parentId), sorted by status priority
  const roots = sessions
    .filter((s) => !s.parentId)
    .sort((a, b) => (STATUS_PRIORITY[a.status] ?? 99) - (STATUS_PRIORITY[b.status] ?? 99));

  return (
    <div className="flex h-full flex-col">
      {/* KPI Header */}
      <div className="shrink-0 border-b border-border-subtle px-4 py-3">
        <div className="flex items-center gap-4 text-xs">
          <span className="text-status-working font-medium">{active.length} active</span>
          <span className="text-text-faint">·</span>
          <span className="text-status-needs-input font-medium">{blocked.length} blocked</span>
          <span className="text-text-faint">·</span>
          <span className="text-text-muted">{completed.length} completed</span>
        </div>
      </div>

      {/* Session Tree */}
      <div className="flex-1 overflow-y-auto">
        <SessionTree sessions={roots} allSessions={sessions} />
      </div>
    </div>
  );
}
```

**Step 2: Commit**

```bash
git add apps/web/src/components/vigil/session-monitor.tsx && git commit -m "feat: add SessionMonitor panel with KPI header and session tree"
```

---

### Task 5.2: Create SessionTree Component

**Files:**
- Create: `apps/web/src/components/vigil/session-tree.tsx`

**Step 1: Build hierarchical session tree**

```typescript
'use client';
import { useState } from 'react';
import { useRouter } from 'next/navigation';
import type { Session } from '@/lib/types';
import { StatusBadge } from '@/components/dashboard/status-badge';

function formatDuration(startedAt?: number | null): string {
  if (!startedAt) return '';
  const elapsed = Date.now() - startedAt;
  const secs = Math.floor(elapsed / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ${mins % 60}m`;
}

function SessionRow({
  session,
  children: childSessions,
  depth,
}: {
  session: Session;
  children: Session[];
  depth: number;
}) {
  const [expanded, setExpanded] = useState(true);
  const router = useRouter();
  const hasChildren = childSessions.length > 0;

  return (
    <div>
      <button
        type="button"
        onClick={() => router.push(`/dashboard/sessions/${session.id}`)}
        className="flex w-full items-center gap-2 px-4 py-2.5 text-left hover:bg-surface-hover transition-colors"
        style={{ paddingLeft: `${16 + depth * 20}px` }}
      >
        {/* Expand/collapse for parents */}
        {hasChildren ? (
          <button
            type="button"
            onClick={(e) => { e.stopPropagation(); setExpanded(!expanded); }}
            className="text-text-muted hover:text-text text-xs"
          >
            {expanded ? '▼' : '▶'}
          </button>
        ) : (
          <span className="w-3" /> // spacer
        )}

        {/* Status dot */}
        <StatusBadge status={session.status} />

        {/* Prompt (truncated) */}
        <span className="flex-1 truncate text-xs text-text">
          {session.prompt}
        </span>

        {/* Duration */}
        <span className="shrink-0 text-xs text-text-muted font-mono tabular-nums">
          {formatDuration(session.startedAt)}
        </span>

        {/* Child count badge */}
        {hasChildren && (
          <span className="rounded-full bg-surface-alt px-1.5 py-0.5 text-[10px] text-text-muted">
            {childSessions.length}
          </span>
        )}
      </button>

      {/* Children */}
      {expanded && childSessions.map((child) => (
        <SessionRow
          key={child.id}
          session={child}
          children={[]} // No grandchildren in current model
          depth={depth + 1}
        />
      ))}
    </div>
  );
}

export function SessionTree({
  sessions,
  allSessions,
}: {
  sessions: Session[];
  allSessions: Session[];
}) {
  return (
    <div className="divide-y divide-border-subtle">
      {sessions.map((session) => {
        const children = allSessions.filter((s) => s.parentId === session.id);
        return (
          <SessionRow
            key={session.id}
            session={session}
            children={children}
            depth={0}
          />
        );
      })}
    </div>
  );
}
```

**Step 2: Commit**

```bash
git add apps/web/src/components/vigil/session-tree.tsx && git commit -m "feat: add SessionTree component with parent-child hierarchy"
```

---

## Phase 6: Frontend — Terminal Overlay

### Task 6.1: Create Terminal Overlay Component

**Files:**
- Create: `apps/web/src/components/vigil/terminal-overlay.tsx`
- Modify: `apps/web/src/app/dashboard/sessions/[id]/page.tsx` (optional — keep existing for direct URL access)

The current session detail page at `apps/web/src/app/dashboard/sessions/[id]/page.tsx` already handles terminal display. We keep it as-is for direct URL access. The terminal overlay wraps the existing `TerminalPanel` for in-app overlay use.

**Step 1: Build terminal overlay (used from Vigil context)**

The existing session detail page already serves as the terminal view. No separate overlay component is needed — clicking a session in the monitor panel navigates to `/dashboard/sessions/{id}` which has a back button. The `isSessionPage()` function in `layout.tsx` already hides the sidebar for immersive terminal view.

This task is complete by using the existing session detail page.

**Step 2: Commit**

No changes needed — existing implementation covers this use case.

---

## Phase 7: Frontend — Cleanup Dead Code

### Task 7.1: Remove Unused Components

**Files:**
- Delete: `apps/web/src/components/dashboard/new-session.tsx` (replaced by Vigil chat)
- Delete: `apps/web/src/components/dashboard/session-grid.tsx` (replaced by monitor panel)
- Delete: `apps/web/src/components/dashboard/session-list.tsx` (replaced by monitor panel)
- Delete: `apps/web/src/components/dashboard/kpi-bar.tsx` (KPIs moved to monitor header)
- Delete: `apps/web/src/components/dashboard/recent-activity.tsx` (if unused)

**Step 1: Delete unused components**

```bash
rm apps/web/src/components/dashboard/new-session.tsx
rm apps/web/src/components/dashboard/session-grid.tsx
rm apps/web/src/components/dashboard/session-list.tsx
rm apps/web/src/components/dashboard/kpi-bar.tsx
rm apps/web/src/components/dashboard/recent-activity.tsx
```

**Step 2: Remove imports from layout.tsx and other files**

Grep for imports of deleted components and remove them.

**Step 3: Verify build**

```bash
cd apps/web && npx next build
```

Expected: No import errors.

**Step 4: Commit**

```bash
git add -A && git commit -m "chore: remove unused session grid, KPI bar, and new-session components"
```

---

### Task 7.2: Clean Up Old Server References

**Files:**
- Modify: `apps/web/next.config.ts` (update proxy comment, port already correct)
- Modify: `apps/web/src/lib/ws-url.ts` (update comment from "Fastify" to "daemon")

**Step 1: Update comments referencing old backend**

In `next.config.ts`:
- Change comment from "Fastify" to "Rust daemon"

In `ws-url.ts`:
- Change comment from "Fastify server" to "daemon"

**Step 2: Commit**

```bash
git add -A && git commit -m "chore: update comments to reference Rust daemon instead of Fastify"
```

---

## Phase 8: Build Verification

### Task 8.1: Full Build and Test

**Step 1: Run Rust tests and clippy**

```bash
cd apps/daemon && cargo clippy -- -D warnings && cargo test
```

Expected: All tests pass, no warnings.

**Step 2: Run frontend build**

```bash
cd apps/web && npx next build
```

Expected: Build succeeds with no errors.

**Step 3: Run shared package build**

```bash
cd packages/shared && npm run build
```

Expected: No errors.

**Step 4: Full workspace build**

```bash
npm run build
```

Expected: All workspaces build successfully.

**Step 5: Commit any fixes**

```bash
git add -A && git commit -m "fix: resolve build issues from UI restructure"
```

---

## Summary

| Phase | Tasks | Description |
|-------|-------|-------------|
| 0 | 2 | Remove old backend, update workspace |
| 1 | 5 | Backend: chat history, global Vigil, escalation, OpenAPI |
| 2 | 4 | Frontend: types, API hooks, stores, WS extension |
| 3 | 2 | Frontend: layout + main page restructure |
| 4 | 3 | Frontend: Vigil chat + message + embedded cards |
| 5 | 2 | Frontend: session monitor + tree |
| 6 | 1 | Frontend: terminal overlay (reuse existing) |
| 7 | 2 | Frontend: cleanup dead code + references |
| 8 | 1 | Build verification |
| **Total** | **22** | |
