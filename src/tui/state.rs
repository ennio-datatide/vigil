use std::sync::Arc;

use crate::db::models::{Session, SessionStatus};
use crate::process::output_manager::OutputManager;

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
    pub chat_tx: Option<tokio::sync::mpsc::Sender<String>>,
    pub output_manager: Option<Arc<OutputManager>>,
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

pub struct Pane {
    pub session_id: String,
    pub parser: std::sync::Arc<std::sync::RwLock<tui_term::vt100::Parser>>,
}

impl std::fmt::Debug for Pane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pane")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
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
            chat_tx: None,
            output_manager: None,
            panes: Vec::new(),
            active_pane: 0,
            should_quit: false,
            confirm_quit: false,
            show_help: false,
        }
    }

    pub fn active_sessions_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.status == SessionStatus::Running)
            .count()
    }

    pub fn blocked_sessions_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| {
                s.status == SessionStatus::NeedsInput || s.status == SessionStatus::AuthRequired
            })
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
