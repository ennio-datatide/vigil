//! TUI application event loop.
//!
//! Drives the render/update cycle and dispatches keyboard events to the
//! appropriate view handler.

use std::sync::Arc;
use std::time::Duration;

use color_eyre::Result;
use futures::StreamExt;
use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};
use tokio_util::sync::CancellationToken;

use crate::db::models::{Session, SessionStatus};
use crate::process::output_manager::OutputManager;
use crate::tui::state::{App, ChatMessage, ChatSender, Message, View};
use crate::tui::views;

/// Run the TUI event loop until the user quits or the cancellation token fires.
pub async fn run(
    mut terminal: DefaultTerminal,
    cancel: CancellationToken,
    mut session_rx: tokio::sync::watch::Receiver<Vec<Session>>,
    chat_tx: tokio::sync::mpsc::Sender<String>,
    mut chat_resp_rx: tokio::sync::watch::Receiver<Option<String>>,
    output_manager: Arc<OutputManager>,
) -> Result<()> {
    let mut app = App::new();
    app.chat_tx = Some(chat_tx);
    app.output_manager = Some(output_manager);

    // First-run detection: show setup if ~/.vigil/ doesn't exist yet.
    let vigil_dir = dirs::home_dir()
        .map(|h| h.join(".vigil"))
        .unwrap_or_default();
    if !vigil_dir.exists() {
        app.view = View::Setup;
    }
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(33));

    loop {
        tokio::select! {
            Some(Ok(event)) = events.next() => {
                match event {
                    Event::Key(key) => {
                        update(&mut app, Message::Key(key));
                    }
                    Event::Resize(cols, rows) => {
                        for pane in &app.panes {
                            if let Ok(mut p) = pane.parser.write() {
                                p.screen_mut().set_size(rows, cols);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ = tick.tick() => {
                terminal.draw(|frame| view(&app, frame))?;
            }
            Ok(()) = session_rx.changed() => {
                let sessions = session_rx.borrow_and_update().clone();
                update(&mut app, Message::SessionsUpdated(sessions));
            }
            Ok(()) = chat_resp_rx.changed() => {
                if let Some(response) = chat_resp_rx.borrow_and_update().clone() {
                    update(&mut app, Message::ChatResponse(response));
                }
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

fn is_blocked(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::NeedsInput | SessionStatus::AuthRequired
    )
}

fn update(app: &mut App, msg: Message) {
    match msg {
        Message::Key(key) => handle_key(app, key),
        Message::SessionsUpdated(sessions) => {
            // Check for newly blocked sessions and push system chat messages.
            for session in &sessions {
                if is_blocked(&session.status) {
                    let was_blocked = app
                        .sessions
                        .iter()
                        .find(|s| s.id == session.id)
                        .map(|s| is_blocked(&s.status))
                        .unwrap_or(false);
                    if !was_blocked {
                        let short_id = &session.id[..4.min(session.id.len())];
                        app.chat_messages.push(ChatMessage {
                            sender: ChatSender::System,
                            content: format!(
                                "Worker #{} is blocked ({})",
                                short_id,
                                format!("{:?}", session.status).to_lowercase()
                            ),
                            timestamp: chrono::Utc::now(),
                        });
                    }
                }
            }
            app.sessions = sessions;
        }
        Message::ChatResponse(response) => {
            app.chat_messages.push(ChatMessage {
                sender: ChatSender::Vigil,
                content: response,
                timestamp: chrono::Utc::now(),
            });
        }
        Message::Tick | Message::Quit => {
            if matches!(msg, Message::Quit) {
                app.should_quit = true;
            }
        }
        _ => {}
    }
}

fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) {
    // Quit confirmation overlay.
    if app.confirm_quit {
        match key.code {
            KeyCode::Char('y') => app.should_quit = true,
            _ => app.confirm_quit = false,
        }
        return;
    }

    // Help overlay — any key dismisses it.
    if app.show_help {
        app.show_help = false;
        return;
    }

    // Ctrl-C always tries to quit.
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
        View::Setup => handle_setup_key(app, key),
    }
}

fn open_pane(app: &mut App, session_id: String) {
    // Don't open duplicate panes for the same session — just switch to it.
    if let Some(idx) = app.panes.iter().position(|p| p.session_id == session_id) {
        app.active_pane = idx;
        return;
    }

    // Limit to 4 panes — replace the active pane when at capacity.
    if app.panes.len() >= 4 {
        app.panes.remove(app.active_pane);
    }

    let parser = std::sync::Arc::new(std::sync::RwLock::new(
        tui_term::vt100::Parser::new(24, 80, 0),
    ));

    // Subscribe to output and spawn a feeder task.
    if let Some(ref om) = app.output_manager {
        let om = Arc::clone(om);
        let sid = session_id.clone();
        let parser_clone = Arc::clone(&parser);
        tokio::spawn(async move {
            // Feed any existing buffered output first.
            if let Some(buffer) = om.get_buffer(&sid).await {
                if let Ok(mut p) = parser_clone.write() {
                    p.process(&buffer);
                }
            }
            // Then subscribe to live updates.
            if let Some(mut rx) = om.subscribe(&sid).await {
                while let Ok(data) = rx.recv().await {
                    if let Ok(mut p) = parser_clone.write() {
                        p.process(&data);
                    }
                }
            }
        });
    }

    let pane = crate::tui::state::Pane {
        session_id,
        parser,
    };
    app.panes.push(pane);
    app.active_pane = app.panes.len() - 1;
}

fn handle_setup_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Up => {
            if app.setup_selection > 0 {
                app.setup_selection -= 1;
            }
        }
        KeyCode::Down => {
            if app.setup_selection < 2 {
                app.setup_selection += 1;
            }
        }
        KeyCode::Enter => {
            match app.setup_selection {
                0 => {
                    // TODO: shell out to install ultrapowers
                    app.navigate_to(View::SessionList);
                }
                1 => {
                    // TODO: verify plugin directory
                    app.navigate_to(View::SessionList);
                }
                _ => {
                    app.navigate_to(View::SessionList);
                }
            }
        }
        _ => {}
    }
}

fn handle_session_list_key(app: &mut App, key: crossterm::event::KeyEvent) {
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
            if let Some(session) = app.sessions.get(app.selected_session) {
                let session_id = session.id.clone();
                open_pane(app, session_id);
            }
            if !app.panes.is_empty() {
                app.navigate_to(View::Terminal);
            }
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
        _ => {}
    }
}

fn handle_chat_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Esc => app.go_back(),
        KeyCode::Enter => {
            if !app.chat_input.is_empty() {
                let content = app.chat_input.clone();
                app.chat_input.clear();
                app.chat_messages.push(crate::tui::state::ChatMessage {
                    sender: crate::tui::state::ChatSender::User,
                    content: content.clone(),
                    timestamp: chrono::Utc::now(),
                });
                if let Some(tx) = &app.chat_tx {
                    let _ = tx.try_send(content);
                }
            }
        }
        KeyCode::Char(c) => app.chat_input.push(c),
        KeyCode::Backspace => {
            app.chat_input.pop();
        }
        _ => {}
    }
}

fn handle_terminal_key(app: &mut App, key: crossterm::event::KeyEvent) {
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
        _ => {}
    }
}

fn view(app: &App, frame: &mut Frame) {
    let area = frame.area();

    if area.width < 80 || area.height < 24 {
        let msg =
            ratatui::widgets::Paragraph::new("Terminal too small (need 80\u{d7}24)\u{2026}")
                .style(crate::tui::theme::muted())
                .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    match app.view {
        View::SessionList => views::session_list::render(app, frame, area),
        View::Chat => views::chat::render(app, frame, area),
        View::Terminal => views::terminal::render(app, frame, area),
        View::Setup => views::setup::render(app, frame, area),
    }

    if app.confirm_quit {
        let count = app.active_sessions_count();
        let popup = ratatui::widgets::Paragraph::new(format!(
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
