# Vigil TUI Pivot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use ultrapowers:subagent-driven-development (recommended) or ultrapowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pivot Vigil from a web-based tool to a terminal-only TUI, removing the Next.js frontend and pipeline system, adding a ratatui TUI alongside the existing daemon services.

**Architecture:** Single Rust binary running ratatui TUI as the main async tokio task, axum HTTP server (hook ingestion only) as a spawned task, and background services. Elm Architecture (TEA) for the TUI. Communication via watch/mpsc channels and CancellationToken.

**Tech Stack:** Rust, ratatui 0.30, tui-term 0.3, axum, tokio, sqlx (SQLite), color-eyre, tokio-util

**Skills:** `ratatui-patterns`, `rust-best-practices`, `testing-tdd`, `design-patterns`, `database-design`, `api-design`, `observability`, `resilience`

---

## Phase 1: Add TUI to Existing Daemon

### Task 1: Add TUI Dependencies

**Files:**
- Modify: `apps/daemon/Cargo.toml`

- [ ] **Step 1: Add ratatui, tui-term, color-eyre, tokio-util dependencies**

Add to `[dependencies]` in `apps/daemon/Cargo.toml`:

```toml
ratatui = "0.30"
tui-term = "0.3"
color-eyre = "0.6"
tokio-util = { version = "0.7", features = ["rt"] }
tracing-appender = "0.2"
```

- [ ] **Step 2: Verify it compiles**

Run: `cd apps/daemon && cargo check`
Expected: Compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add apps/daemon/Cargo.toml
git commit -m "chore: add ratatui, tui-term, color-eyre, tokio-util dependencies"
```

---

### Task 2: Create Theme Module

**Files:**
- Create: `apps/daemon/src/tui/mod.rs`
- Create: `apps/daemon/src/tui/theme.rs`
- Modify: `apps/daemon/src/lib.rs` (add `pub mod tui;`)

- [ ] **Step 1: Create tui module with theme constants**

`apps/daemon/src/tui/mod.rs`:
```rust
pub mod theme;
```

`apps/daemon/src/tui/theme.rs`:
```rust
use ratatui::style::{Color, Modifier, Style};

// Semantic color palette — Refined Command Center
pub const BG: Color = Color::Rgb(12, 14, 20);
pub const SURFACE: Color = Color::Rgb(22, 25, 38);
pub const BORDER: Color = Color::Rgb(37, 42, 58);
pub const BORDER_FOCUS: Color = Color::Rgb(79, 195, 247);
pub const TEXT: Color = Color::Rgb(220, 224, 235);
pub const TEXT_MUTED: Color = Color::Rgb(99, 107, 131);
pub const ACCENT: Color = Color::Rgb(79, 195, 247);
pub const SUCCESS: Color = Color::Rgb(102, 187, 106);
pub const WARNING: Color = Color::Rgb(255, 167, 38);
pub const ERROR: Color = Color::Rgb(239, 83, 80);
pub const HIGHLIGHT: Color = Color::Rgb(171, 71, 188);

// Prebuilt styles
pub fn text() -> Style {
    Style::new().fg(TEXT).bg(BG)
}

pub fn muted() -> Style {
    Style::new().fg(TEXT_MUTED).bg(BG)
}

pub fn selected() -> Style {
    Style::new().fg(ACCENT).bg(SURFACE)
}

pub fn status_running() -> Style {
    Style::new().fg(SUCCESS)
}

pub fn status_blocked() -> Style {
    Style::new().fg(WARNING).bg(Color::Rgb(40, 35, 20))
}

pub fn status_failed() -> Style {
    Style::new().fg(ERROR)
}

pub fn status_completed() -> Style {
    Style::new().fg(TEXT_MUTED)
}

pub fn border() -> Style {
    Style::new().fg(BORDER)
}

pub fn border_focus() -> Style {
    Style::new().fg(BORDER_FOCUS)
}

pub fn vigil_message() -> Style {
    Style::new().fg(HIGHLIGHT)
}

pub fn user_message() -> Style {
    Style::new().fg(ACCENT)
}

pub fn header() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}
```

- [ ] **Step 2: Add `pub mod tui;` to lib.rs**

Add to `apps/daemon/src/lib.rs`:
```rust
pub mod tui;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`
Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/tui/ apps/daemon/src/lib.rs
git commit -m "feat: add TUI theme module with semantic color palette"
```

---

### Task 3: Create TUI State (Model)

**Files:**
- Create: `apps/daemon/src/tui/state.rs`
- Modify: `apps/daemon/src/tui/mod.rs`

- [ ] **Step 1: Create the app state struct**

`apps/daemon/src/tui/state.rs`:
```rust
use crate::db::models::Session;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    SessionList,
    Chat,
    Terminal,
    Setup,
}

#[derive(Debug, Clone)]
pub enum Message {
    Key(ratatui::crossterm::event::KeyEvent),
    SessionsUpdated(Vec<Session>),
    OutputReceived { session_id: String, bytes: Vec<u8> },
    NotificationReceived { session_id: String, message: String },
    ChatResponse(String),
    Tick,
    Quit,
}

#[derive(Debug)]
pub struct App {
    pub view: View,
    pub previous_view: Option<View>,
    pub sessions: Vec<Session>,
    pub selected_session: usize,
    pub chat_messages: Vec<ChatMessage>,
    pub chat_input: String,
    pub panes: Vec<Pane>,
    pub active_pane: usize,
    pub should_quit: bool,
    pub confirm_quit: bool,
    pub show_help: bool,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub sender: ChatSender,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatSender {
    User,
    Vigil,
}

#[derive(Debug)]
pub struct Pane {
    pub session_id: String,
    pub parser: std::sync::Arc<std::sync::RwLock<tui_term::vt100::Parser>>,
}

impl App {
    pub fn new() -> Self {
        Self {
            view: View::SessionList,
            previous_view: None,
            sessions: Vec::new(),
            selected_session: 0,
            chat_messages: Vec::new(),
            chat_input: String::new(),
            panes: Vec::new(),
            active_pane: 0,
            should_quit: false,
            confirm_quit: false,
            show_help: false,
        }
    }

    pub fn active_sessions_count(&self) -> usize {
        self.sessions.iter().filter(|s| s.status == "running").count()
    }

    pub fn blocked_sessions_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.status == "needs_input" || s.status == "auth_required")
            .count()
    }

    pub fn navigate_to(&mut self, view: View) {
        self.previous_view = Some(self.view.clone());
        self.view = view;
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.previous_view.take() {
            self.view = prev;
        } else {
            self.view = View::SessionList;
        }
    }
}
```

- [ ] **Step 2: Update tui/mod.rs**

```rust
pub mod state;
pub mod theme;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`
Expected: Compiles (may need to adjust Session model import path)

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/tui/
git commit -m "feat: add TUI state model (App, Message, View, ChatMessage, Pane)"
```

---

### Task 4: Create Session List View

**Files:**
- Create: `apps/daemon/src/tui/views/mod.rs`
- Create: `apps/daemon/src/tui/views/session_list.rs`
- Modify: `apps/daemon/src/tui/mod.rs`

- [ ] **Step 1: Create session list view**

`apps/daemon/src/tui/views/mod.rs`:
```rust
pub mod session_list;
```

`apps/daemon/src/tui/views/session_list.rs`:
```rust
use ratatui::prelude::*;
use ratatui::widgets::*;
use crate::tui::{state::App, theme};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(2),  // header
        Constraint::Min(1),    // session list
        Constraint::Length(3), // status bar
        Constraint::Length(1), // help bar
    ])
    .split(area);

    render_header(app, frame, chunks[0]);
    render_sessions(app, frame, chunks[1]);
    render_status_bar(app, frame, chunks[2]);
    render_help_bar(frame, chunks[3]);
}

fn render_header(app: &App, frame: &mut Frame, area: Rect) {
    let active = app.active_sessions_count();
    let blocked = app.blocked_sessions_count();

    let header = Line::from(vec![
        Span::styled("  ◉ VIGIL", theme::header()),
        Span::raw("  "),
        Span::styled(
            format!("{}  ", " ".repeat(area.width as usize - 30)),
            theme::text(),
        ),
        Span::styled(format!("{active} active"), theme::status_running()),
        Span::styled(" · ", theme::muted()),
        Span::styled(
            format!("{blocked} blocked"),
            if blocked > 0 { theme::status_blocked() } else { theme::muted() },
        ),
    ]);

    frame.render_widget(header, area);

    // Divider
    let divider_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let divider = Line::styled("─".repeat(area.width as usize), theme::border());
    frame.render_widget(divider, divider_area);
}

fn render_sessions(app: &App, frame: &mut Frame, area: Rect) {
    if app.sessions.is_empty() {
        let empty = Paragraph::new("No sessions running. Press c to chat with Vigil…")
            .style(theme::muted())
            .alignment(Alignment::Center);
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<Line> = app
        .sessions
        .iter()
        .enumerate()
        .map(|(i, session)| {
            let is_selected = i == app.selected_session;
            let cursor = if is_selected { "▌" } else { " " };
            let (icon, icon_style) = match session.status.as_str() {
                "running" => ("●", theme::status_running()),
                "needs_input" | "auth_required" => ("⚠", theme::status_blocked()),
                "completed" => ("✓", theme::status_completed()),
                "failed" | "cancelled" => ("✗", theme::status_failed()),
                _ => ("○", theme::muted()),
            };

            let id_short = &session.id[..4.min(session.id.len())];
            let project = session
                .project_path
                .split('/')
                .last()
                .unwrap_or("unknown");

            let line_style = if session.status == "needs_input" || session.status == "auth_required" {
                theme::status_blocked()
            } else if is_selected {
                theme::selected()
            } else {
                theme::text()
            };

            Line::from(vec![
                Span::styled(cursor, theme::border_focus()),
                Span::styled(format!(" {icon}  "), icon_style),
                Span::styled(format!("{id_short:<6}"), theme::muted()),
                Span::styled(format!("{project:<22}"), line_style),
                Span::styled(format!("{:<14}", session.status), line_style),
            ])
        })
        .collect();

    let list = Paragraph::new(items);
    frame.render_widget(list, area);
}

fn render_status_bar(_app: &App, frame: &mut Frame, area: Rect) {
    let bar = Block::new().style(Style::new().bg(theme::SURFACE));
    frame.render_widget(bar, area);
}

fn render_help_bar(frame: &mut Frame, area: Rect) {
    let help = Line::from(vec![
        Span::styled("  ↑↓", theme::text()),
        Span::styled(" Navigate   ", theme::muted()),
        Span::styled("⏎", theme::text()),
        Span::styled(" Open   ", theme::muted()),
        Span::styled("c", theme::text()),
        Span::styled(" Chat   ", theme::muted()),
        Span::styled("?", theme::text()),
        Span::styled(" Help   ", theme::muted()),
        Span::styled("q", theme::text()),
        Span::styled(" Quit", theme::muted()),
    ]);
    frame.render_widget(help, area);
}
```

- [ ] **Step 2: Update tui/mod.rs**

```rust
pub mod state;
pub mod theme;
pub mod views;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/tui/
git commit -m "feat: add session list TUI view with themed rendering"
```

---

### Task 5: Create Chat View

**Files:**
- Create: `apps/daemon/src/tui/views/chat.rs`
- Modify: `apps/daemon/src/tui/views/mod.rs`

- [ ] **Step 1: Create chat view**

`apps/daemon/src/tui/views/chat.rs`:
```rust
use ratatui::prelude::*;
use ratatui::widgets::*;
use crate::tui::{state::{App, ChatSender}, theme};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(2),  // header
        Constraint::Min(1),    // messages
        Constraint::Length(2), // input
    ])
    .split(area);

    render_header(frame, chunks[0]);
    render_messages(app, frame, chunks[1]);
    render_input(app, frame, chunks[2]);
}

fn render_header(frame: &mut Frame, area: Rect) {
    let header = Line::from(vec![
        Span::styled("  ◉ VIGIL", theme::header()),
        Span::styled(" · chat", theme::muted()),
    ]);
    frame.render_widget(header, area);

    let divider_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let divider = Line::styled("─".repeat(area.width as usize), theme::border());
    frame.render_widget(divider, divider_area);
}

fn render_messages(app: &App, frame: &mut Frame, area: Rect) {
    if app.chat_messages.is_empty() {
        let empty = Paragraph::new("Chat with Vigil to dispatch workers and manage sessions…")
            .style(theme::muted())
            .alignment(Alignment::Center);
        frame.render_widget(empty, area);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    for msg in &app.chat_messages {
        let time = msg.timestamp.format("%H:%M").to_string();
        let (sender_style, alignment) = match msg.sender {
            ChatSender::Vigil => (theme::vigil_message(), Alignment::Left),
            ChatSender::User => (theme::user_message(), Alignment::Right),
        };
        let sender_name = match msg.sender {
            ChatSender::Vigil => "vigil",
            ChatSender::User => "you",
        };

        lines.push(Line::from(vec![
            Span::styled(format!("    {sender_name}"), sender_style),
            Span::styled(format!(" · {time}"), theme::muted()),
        ]));

        for text_line in msg.content.lines() {
            lines.push(Line::styled(format!("    {text_line}"), theme::text()));
        }
        lines.push(Line::raw("")); // spacing
    }

    let paragraph = Paragraph::new(lines)
        .scroll((lines.len().saturating_sub(area.height as usize) as u16, 0));
    frame.render_widget(paragraph, area);
}

fn render_input(app: &App, frame: &mut Frame, area: Rect) {
    let divider = Line::styled("─".repeat(area.width as usize), theme::border());
    frame.render_widget(divider, Rect::new(area.x, area.y, area.width, 1));

    let input_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let input = Line::from(vec![
        Span::styled("  › ", theme::user_message()),
        Span::styled(&app.chat_input, theme::text()),
        Span::styled("_", theme::text()), // cursor
        Span::raw(" ".repeat((area.width as usize).saturating_sub(app.chat_input.len() + 10))),
        Span::styled("Esc Back", theme::muted()),
    ]);
    frame.render_widget(input, input_area);
}
```

- [ ] **Step 2: Update views/mod.rs**

```rust
pub mod chat;
pub mod session_list;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/tui/views/
git commit -m "feat: add Vigil chat TUI view"
```

---

### Task 6: Create Terminal Panes View

**Files:**
- Create: `apps/daemon/src/tui/views/terminal.rs`
- Modify: `apps/daemon/src/tui/views/mod.rs`

- [ ] **Step 1: Create terminal panes view**

`apps/daemon/src/tui/views/terminal.rs`:
```rust
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_term::widget::PseudoTerminal;
use crate::tui::{state::App, theme};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    if app.panes.is_empty() {
        let empty = Paragraph::new("No terminal panes open. Press Esc to go back…")
            .style(theme::muted())
            .alignment(Alignment::Center);
        frame.render_widget(empty, area);
        return;
    }

    let chunks = Layout::vertical([
        Constraint::Min(1),    // panes
        Constraint::Length(1), // help bar
    ])
    .split(area);

    let pane_areas = compute_pane_layout(app.panes.len(), chunks[0]);

    for (i, (pane, pane_area)) in app.panes.iter().zip(&pane_areas).enumerate() {
        let is_active = i == app.active_pane;
        render_pane(pane, is_active, frame, *pane_area);
    }

    render_help_bar(frame, chunks[1]);
}

fn compute_pane_layout(count: usize, area: Rect) -> Vec<Rect> {
    match count {
        1 => vec![area],
        2 => Layout::horizontal([Constraint::Percentage(50); 2])
            .split(area)
            .to_vec(),
        3 => {
            let rows = Layout::vertical([Constraint::Percentage(50); 2]).split(area);
            let top = Layout::horizontal([Constraint::Percentage(50); 2]).split(rows[0]);
            vec![top[0], top[1], rows[1]]
        }
        _ => {
            let rows = Layout::vertical([Constraint::Percentage(50); 2]).split(area);
            let top = Layout::horizontal([Constraint::Percentage(50); 2]).split(rows[0]);
            let bot = Layout::horizontal([Constraint::Percentage(50); 2]).split(rows[1]);
            vec![top[0], top[1], bot[0], bot[1]]
        }
    }
}

fn render_pane(pane: &crate::tui::state::Pane, is_active: bool, frame: &mut Frame, area: Rect) {
    let id_short = &pane.session_id[..4.min(pane.session_id.len())];
    let border_style = if is_active { theme::border_focus() } else { theme::border() };
    let title_style = if is_active { theme::header() } else { theme::muted() };

    let block = Block::bordered()
        .title(Line::styled(format!(" {id_short} "), title_style))
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let screen = pane.parser.read().unwrap();
    let pseudo_term = PseudoTerminal::new(screen.screen());
    frame.render_widget(pseudo_term, inner);
}

fn render_help_bar(frame: &mut Frame, area: Rect) {
    let help = Line::from(vec![
        Span::styled("  Tab", theme::text()),
        Span::styled(" Switch   ", theme::muted()),
        Span::styled("⏎", theme::text()),
        Span::styled(" Focus   ", theme::muted()),
        Span::styled("^D", theme::text()),
        Span::styled(" Close   ", theme::muted()),
        Span::styled("Esc", theme::text()),
        Span::styled(" Back", theme::muted()),
    ]);
    frame.render_widget(help, area);
}
```

- [ ] **Step 2: Update views/mod.rs**

```rust
pub mod chat;
pub mod session_list;
pub mod terminal;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`

- [ ] **Step 4: Commit**

```bash
git add apps/daemon/src/tui/views/
git commit -m "feat: add terminal panes TUI view with tui-term PTY rendering"
```

---

### Task 7: Create TUI Event Loop and Wire Into Main

**Files:**
- Create: `apps/daemon/src/tui/app.rs`
- Modify: `apps/daemon/src/tui/mod.rs`
- Modify: `apps/daemon/src/main.rs`

- [ ] **Step 1: Create the TUI event loop**

`apps/daemon/src/tui/app.rs`:
```rust
use std::time::Duration;
use color_eyre::Result;
use futures::StreamExt;
use ratatui::crossterm::event::{EventStream, Event, KeyCode, KeyModifiers};
use ratatui::DefaultTerminal;
use tokio_util::sync::CancellationToken;
use crate::tui::state::{App, Message, View};
use crate::tui::views;

pub async fn run(
    mut terminal: DefaultTerminal,
    cancel: CancellationToken,
) -> Result<()> {
    let mut app = App::new();
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(33));

    loop {
        tokio::select! {
            Some(Ok(event)) = events.next() => {
                if let Event::Key(key) = event {
                    update(&mut app, Message::Key(key));
                }
            }
            _ = tick.tick() => {
                terminal.draw(|frame| view(&app, frame))?;
            }
            _ = cancel.cancelled() => {
                app.should_quit = true;
                break;
            }
        }
        if app.should_quit {
            cancel.cancel();
            break;
        }
    }

    Ok(())
}

fn update(app: &mut App, msg: Message) {
    match msg {
        Message::Key(key) => handle_key(app, key),
        Message::SessionsUpdated(sessions) => app.sessions = sessions,
        Message::Tick => {}
        Message::Quit => app.should_quit = true,
        _ => {}
    }
}

fn handle_key(app: &mut App, key: ratatui::crossterm::event::KeyEvent) {
    // Quit confirmation
    if app.confirm_quit {
        match key.code {
            KeyCode::Char('y') => app.should_quit = true,
            _ => app.confirm_quit = false,
        }
        return;
    }

    // Help overlay
    if app.show_help {
        app.show_help = false;
        return;
    }

    // Ctrl-C = quit
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if app.active_sessions_count() > 0 {
            app.confirm_quit = true;
        } else {
            app.should_quit = true;
        }
        return;
    }

    match app.view {
        View::SessionList => handle_session_list_key(app, key),
        View::Chat => handle_chat_key(app, key),
        View::Terminal => handle_terminal_key(app, key),
        View::Setup => {}
    }
}

fn handle_session_list_key(app: &mut App, key: ratatui::crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Up => {
            if app.selected_session > 0 {
                app.selected_session -= 1;
            }
        }
        KeyCode::Down => {
            if app.selected_session < app.sessions.len().saturating_sub(1) {
                app.selected_session += 1;
            }
        }
        KeyCode::Enter => {
            // TODO: open terminal pane for selected session
            app.navigate_to(View::Terminal);
        }
        KeyCode::Char('c') => app.navigate_to(View::Chat),
        KeyCode::Char('q') => {
            if app.active_sessions_count() > 0 {
                app.confirm_quit = true;
            } else {
                app.should_quit = true;
            }
        }
        KeyCode::Char('?') => app.show_help = true,
        KeyCode::Char('1') => {} // already on session list
        _ => {}
    }
}

fn handle_chat_key(app: &mut App, key: ratatui::crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Esc => app.go_back(),
        KeyCode::Enter => {
            if !app.chat_input.is_empty() {
                let content = app.chat_input.clone();
                app.chat_input.clear();
                app.chat_messages.push(crate::tui::state::ChatMessage {
                    sender: crate::tui::state::ChatSender::User,
                    content,
                    timestamp: chrono::Utc::now(),
                });
                // TODO: send to VigilManager
            }
        }
        KeyCode::Char(c) => app.chat_input.push(c),
        KeyCode::Backspace => { app.chat_input.pop(); }
        _ => {}
    }
}

fn handle_terminal_key(app: &mut App, key: ratatui::crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Esc => app.go_back(),
        KeyCode::Tab => {
            if !app.panes.is_empty() {
                app.active_pane = (app.active_pane + 1) % app.panes.len();
            }
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if !app.panes.is_empty() {
                app.panes.remove(app.active_pane);
                if app.active_pane >= app.panes.len() && !app.panes.is_empty() {
                    app.active_pane = app.panes.len() - 1;
                }
                if app.panes.is_empty() {
                    app.go_back();
                }
            }
        }
        KeyCode::Char('1') => app.navigate_to(View::SessionList),
        _ => {
            // TODO: forward to PTY stdin
        }
    }
}

fn view(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Minimum terminal size check
    if area.width < 80 || area.height < 24 {
        let msg = Paragraph::new("Terminal too small (need 80×24)…")
            .style(crate::tui::theme::muted())
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    match app.view {
        View::SessionList => views::session_list::render(app, frame, area),
        View::Chat => views::chat::render(app, frame, area),
        View::Terminal => views::terminal::render(app, frame, area),
        View::Setup => {} // TODO: first-run setup
    }

    // Quit confirmation overlay
    if app.confirm_quit {
        let count = app.active_sessions_count();
        let popup = Paragraph::new(format!(
            "{count} sessions still running. Quit anyway? y/n"
        ))
        .style(crate::tui::theme::status_blocked())
        .alignment(ratatui::layout::Alignment::Center);
        let popup_area = ratatui::layout::Rect::new(
            area.width / 4,
            area.height / 2,
            area.width / 2,
            1,
        );
        frame.render_widget(popup, popup_area);
    }
}
```

- [ ] **Step 2: Update tui/mod.rs**

```rust
pub mod app;
pub mod state;
pub mod theme;
pub mod views;
```

- [ ] **Step 3: Wire TUI into main.rs**

Add TUI startup to `apps/daemon/src/main.rs`. The exact integration depends on the current main.rs structure, but the pattern is:

```rust
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Set up file-only logging (before ratatui::init)
    let file_appender = tracing_appender::rolling::daily(
        dirs::home_dir().unwrap().join(".vigil/logs"),
        "vigil.log",
    );
    tracing_subscriber::fmt()
        .with_writer(file_appender)
        .with_ansi(false)
        .init();

    let cancel = CancellationToken::new();

    // Start existing daemon services + HTTP server (spawned task)
    let server_cancel = cancel.clone();
    tokio::spawn(async move {
        // existing axum server setup...
        // add .with_graceful_shutdown(server_cancel.cancelled_owned())
    });

    // Start background services (existing)
    // ...

    // Run TUI as main task
    let terminal = ratatui::init();
    let result = crate::tui::app::run(terminal, cancel.clone()).await;
    ratatui::restore();

    result
}
```

- [ ] **Step 4: Verify it compiles and runs**

Run: `cd apps/daemon && cargo run`
Expected: TUI renders with empty session list. Press `q` to quit. Terminal restores cleanly.

- [ ] **Step 5: Commit**

```bash
git add apps/daemon/src/tui/ apps/daemon/src/main.rs
git commit -m "feat: wire TUI event loop into daemon main with ratatui::init"
```

---

## Phase 2: Strip HTTP Surface

### Task 8: Remove WebSocket Endpoints and Frontend-Only API Routes

**Files:**
- Delete: `apps/daemon/src/api/ws_dashboard.rs`
- Delete: `apps/daemon/src/api/ws_terminal.rs`
- Delete: `apps/daemon/src/api/sessions.rs`
- Delete: `apps/daemon/src/api/projects.rs`
- Delete: `apps/daemon/src/api/notifications.rs`
- Delete: `apps/daemon/src/api/pipelines.rs`
- Delete: `apps/daemon/src/api/pipeline_executions.rs`
- Delete: `apps/daemon/src/api/settings.rs`
- Delete: `apps/daemon/src/api/skills.rs`
- Delete: `apps/daemon/src/api/sub_sessions.rs`
- Delete: `apps/daemon/src/api/memory.rs`
- Delete: `apps/daemon/src/api/filesystem.rs`
- Delete: `apps/daemon/src/api/middleware.rs`
- Modify: `apps/daemon/src/api.rs` (strip router to events + health + vigil)
- Modify: `apps/daemon/src/api/health.rs` (keep or fold into api.rs)

- [ ] **Step 1: Delete frontend-only API files**

```bash
cd apps/daemon
rm src/api/ws_dashboard.rs src/api/ws_terminal.rs
rm src/api/sessions.rs src/api/projects.rs src/api/notifications.rs
rm src/api/pipelines.rs src/api/pipeline_executions.rs
rm src/api/settings.rs src/api/skills.rs src/api/sub_sessions.rs
rm src/api/memory.rs src/api/filesystem.rs src/api/middleware.rs
```

- [ ] **Step 2: Update api.rs router to only include events, health, vigil**

Strip the router in `apps/daemon/src/api.rs` down to:
```rust
pub fn router(deps: AppDeps) -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .route("/events", post(events::ingest))
        .route("/api/vigil/chat", post(vigil::chat))
        .with_state(deps)
}
```

Remove all other route registrations and the module declarations for deleted files.

- [ ] **Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`
Expected: Compiles (fix any remaining references to deleted modules)

- [ ] **Step 4: Commit**

```bash
git add -A apps/daemon/src/api/
git commit -m "feat: strip HTTP API to events, health, and vigil chat only"
```

---

### Task 9: Remove Pipeline Services

**Files:**
- Delete: `apps/daemon/src/services/pipeline_store.rs`
- Delete: `apps/daemon/src/services/pipeline_runner.rs`
- Delete: `apps/daemon/src/services/pipeline_execution_store.rs`
- Modify: `apps/daemon/src/services.rs` (remove pipeline modules)
- Modify: `apps/daemon/src/deps.rs` (remove pipeline deps)
- Modify: `apps/daemon/src/lib.rs` (remove pipeline startup)

- [ ] **Step 1: Delete pipeline service files**

```bash
cd apps/daemon
rm src/services/pipeline_store.rs src/services/pipeline_runner.rs src/services/pipeline_execution_store.rs
```

- [ ] **Step 2: Remove pipeline references from services.rs, deps.rs, lib.rs**

Remove `pub mod pipeline_store;`, `pub mod pipeline_runner;`, `pub mod pipeline_execution_store;` from services.rs. Remove corresponding fields from `AppDeps` in deps.rs. Remove pipeline startup code from lib.rs.

- [ ] **Step 3: Verify it compiles**

Run: `cd apps/daemon && cargo check`

- [ ] **Step 4: Commit**

```bash
git add -A apps/daemon/src/
git commit -m "feat: remove pipeline services (ultrapowers handles orchestration)"
```

---

### Task 10: Add Database Migration to Drop Pipeline Tables

**Files:**
- Create: `apps/daemon/migrations/004_drop_pipelines.sql`

- [ ] **Step 1: Create migration**

`apps/daemon/migrations/004_drop_pipelines.sql`:
```sql
-- Drop pipeline tables (orchestration moved to ultrapowers)
DROP TABLE IF EXISTS pipeline_executions;
DROP TABLE IF EXISTS pipelines;
```

- [ ] **Step 2: Verify migration runs**

Run: `cd apps/daemon && cargo test`
Expected: Tests pass (in-memory SQLite runs all migrations)

- [ ] **Step 3: Commit**

```bash
git add apps/daemon/migrations/
git commit -m "feat: add migration to drop pipeline tables"
```

---

### Task 11: Remove E2E Tests for Deleted Routes

**Files:**
- Delete or modify: `apps/daemon/src/e2e.rs` and any e2e test files

- [ ] **Step 1: Remove e2e tests that test deleted API routes**

Check `apps/daemon/src/e2e.rs` — remove tests for sessions, projects, notifications, pipelines, settings API endpoints. Keep tests for events ingestion and health check if they exist.

- [ ] **Step 2: Verify remaining tests pass**

Run: `cd apps/daemon && cargo test`

- [ ] **Step 3: Commit**

```bash
git add -A apps/daemon/src/
git commit -m "chore: remove e2e tests for deleted API routes"
```

---

## Phase 3: Flatten Repo Structure

### Task 12: Move Daemon to Repo Root and Delete Frontend

**Files:**
- Move: `apps/daemon/*` → repo root
- Delete: `apps/web/` (entire directory)
- Delete: `packages/shared/` (entire directory)
- Delete: `package.json`, `package-lock.json`, `turbo.json`, `tsconfig.base.json`, `biome.json`, `.node-version`
- Delete: `apps/` (now empty)
- Delete: `ultrapowers-skills/` (dead submodule)

- [ ] **Step 1: Move daemon files to repo root**

```bash
cd /Users/techno1731/Code/Personal/vigil
git mv apps/daemon/Cargo.toml Cargo.toml
git mv apps/daemon/Cargo.lock Cargo.lock 2>/dev/null || true
git mv apps/daemon/src src
git mv apps/daemon/migrations migrations
```

- [ ] **Step 2: Delete frontend and shared packages**

```bash
git rm -rf apps/web
git rm -rf packages/shared
git rm -rf apps 2>/dev/null || true
git rm -rf ultrapowers-skills 2>/dev/null || true
```

- [ ] **Step 3: Delete Node.js / Turborepo config files**

```bash
git rm package.json package-lock.json turbo.json tsconfig.base.json biome.json .node-version 2>/dev/null || true
rm -rf node_modules .turbo
```

- [ ] **Step 4: Update Cargo.toml paths if needed**

Verify `Cargo.toml` paths are correct now that it's at the repo root. The `[package]` name should be `vigil`.

- [ ] **Step 5: Update .gitignore**

Remove Node.js entries, keep Rust entries. Add:
```
/target
.fastembed_cache/
```

- [ ] **Step 6: Verify build**

Run: `cargo build && cargo test`
Expected: Builds and tests pass from repo root

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: flatten repo structure — single Rust crate, delete frontend"
```

---

### Task 13: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Replace CLAUDE.md contents**

Update to reflect the new single-crate Rust project. Remove all references to npm, Turborepo, Vitest, Biome, web frontend, and shared packages. See spec for new CLAUDE.md content.

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for single-crate Rust project"
```

---

## Phase 4: Polish TUI and Wire Services

### Task 14: Connect TUI to SessionStore for Live Updates

**Files:**
- Modify: `src/tui/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add watch channel for session state**

In `main.rs`, create a `tokio::sync::watch` channel. Spawn a task that polls `SessionStore` periodically (or subscribes to EventBus) and sends updates through the watch channel. Pass the receiver to the TUI event loop.

- [ ] **Step 2: Update TUI event loop to receive session updates**

Add `state_rx.changed()` branch to the `tokio::select!` in `app.rs`. On change, update `app.sessions`.

- [ ] **Step 3: Verify live updates**

Start vigil, create a test session via curl to `/events`, verify it appears in the TUI.

- [ ] **Step 4: Commit**

```bash
git add src/
git commit -m "feat: connect TUI session list to live SessionStore updates"
```

---

### Task 15: Connect Chat to VigilManager

**Files:**
- Modify: `src/tui/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add mpsc channel for chat commands**

Create a `tokio::sync::mpsc` channel for sending chat messages from TUI to a background task that calls `VigilManager::send_message()`.

- [ ] **Step 2: Wire chat input to mpsc sender**

When user presses Enter in chat view, send the message through the mpsc channel. Background task forwards to VigilManager, receives response, sends back via watch channel.

- [ ] **Step 3: Display Vigil responses in chat**

Add Vigil's response as a `ChatMessage` with `ChatSender::Vigil`.

- [ ] **Step 4: Commit**

```bash
git add src/
git commit -m "feat: connect chat view to VigilManager for interactive conversation"
```

---

### Task 16: Connect Terminal Panes to OutputManager

**Files:**
- Modify: `src/tui/app.rs`
- Modify: `src/tui/state.rs`
- Modify: `src/tui/views/terminal.rs`

- [ ] **Step 1: On Enter from session list, create Pane with vt100 parser**

When user opens a session, create a `vt100::Parser` sized to the pane area, subscribe to the OutputManager broadcast for that session, and spawn a background task feeding bytes to the parser.

- [ ] **Step 2: Handle pane resize**

On terminal resize event, recreate parsers with new dimensions.

- [ ] **Step 3: Limit to 4 panes**

When opening a 5th session, replace the active pane.

- [ ] **Step 4: Verify PTY output renders**

Start vigil, spawn a session, open its terminal pane, verify output streams live.

- [ ] **Step 5: Commit**

```bash
git add src/
git commit -m "feat: connect terminal panes to live PTY output via tui-term"
```

---

### Task 17: Add First-Run Setup Flow

**Files:**
- Create: `src/tui/views/setup.rs`
- Modify: `src/tui/views/mod.rs`
- Modify: `src/tui/app.rs`
- Modify: `src/tui/state.rs`

- [ ] **Step 1: Create setup view**

A simple menu: "Install ultrapowers now (recommended)" / "Already installed" / "Skip". On "Install now", shell out to `claude` CLI commands. On "Already installed", verify plugin directory exists.

- [ ] **Step 2: Wire into app startup**

If `~/.vigil/` doesn't exist or is first run, set `app.view = View::Setup`. After setup completes, navigate to SessionList.

- [ ] **Step 3: Commit**

```bash
git add src/
git commit -m "feat: add first-run setup flow for ultrapowers plugin installation"
```

---

### Task 18: Add Notification Indicators

**Files:**
- Modify: `src/tui/views/session_list.rs`
- Modify: `src/tui/app.rs`

- [ ] **Step 1: Subscribe to notifications from NotificationStore**

Add notification count to the session list header. Highlight blocked sessions with amber tint.

- [ ] **Step 2: Show inline notification in chat**

When a worker hits a blocker, add a system message to chat: "Worker #xxxx is blocked: {reason}"

- [ ] **Step 3: Commit**

```bash
git add src/
git commit -m "feat: add notification indicators to session list and chat"
```

---

### Task 19: Add Help Overlay

**Files:**
- Create: `src/tui/widgets/mod.rs`
- Create: `src/tui/widgets/help_overlay.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/app.rs`

- [ ] **Step 1: Create help overlay widget**

A centered popup listing all keybindings. Renders on top of current view when `app.show_help` is true. Dismiss with any key.

- [ ] **Step 2: Commit**

```bash
git add src/
git commit -m "feat: add help overlay (? key)"
```

---

### Task 20: Add Snapshot Tests

**Files:**
- Create: `tests/tui_snapshots.rs` (or `src/tui/tests.rs`)

- [ ] **Step 1: Write snapshot tests for each view**

Use `ratatui::Terminal::new(TestBackend::new(80, 24))` + `insta::assert_snapshot!()`. Test: session list with data, session list empty, chat with messages, terminal panes, help overlay, quit confirmation, terminal too small.

- [ ] **Step 2: Run and accept snapshots**

Run: `cargo test` then `cargo insta review`

- [ ] **Step 3: Commit**

```bash
git add src/ tests/ snapshots/
git commit -m "test: add TUI snapshot tests for all views"
```

---

Plan complete and saved to `docs/ultrapowers/plans/2026-03-18-vigil-tui-pivot.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?