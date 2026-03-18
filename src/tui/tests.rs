#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::db::models::{AgentType, Session, SessionStatus};
    use crate::tui::state::{App, ChatMessage, ChatSender};
    use crate::tui::views;
    use crate::tui::widgets;

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    fn render_to_string(
        width: u16,
        height: u16,
        render_fn: impl FnOnce(&mut ratatui::Frame),
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(render_fn).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut output = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                output.push_str(cell.symbol());
            }
            output.push('\n');
        }
        output
    }

    fn test_session(id: &str, status: SessionStatus) -> Session {
        Session {
            id: id.to_string(),
            project_path: "/home/user/my-project".to_string(),
            worktree_path: None,
            tmux_session: None,
            prompt: "implement feature X".to_string(),
            skills_used: None,
            status,
            agent_type: AgentType::Claude,
            role: None,
            parent_id: None,
            spawn_type: None,
            spawn_result: None,
            retry_count: 0,
            started_at: Some(1_700_000_000_000),
            ended_at: None,
            exit_reason: None,
            git_metadata: None,
            pipeline_id: None,
            pipeline_step_index: None,
        }
    }

    fn fixed_timestamp() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    // ---------------------------------------------------------------------------
    // Session list snapshots
    // ---------------------------------------------------------------------------

    #[test]
    fn snapshot_session_list_with_sessions() {
        let mut app = App::new();
        app.sessions = vec![
            test_session("abcd1234", SessionStatus::Running),
            test_session("efgh5678", SessionStatus::NeedsInput),
            test_session("ijkl9012", SessionStatus::Completed),
        ];

        let output = render_to_string(80, 24, |frame| {
            views::session_list::render(&app, frame, frame.area());
        });

        insta::assert_snapshot!(output);
    }

    #[test]
    fn snapshot_session_list_empty() {
        let app = App::new();
        let output = render_to_string(80, 24, |frame| {
            views::session_list::render(&app, frame, frame.area());
        });
        insta::assert_snapshot!(output);
    }

    // ---------------------------------------------------------------------------
    // Chat snapshots
    // ---------------------------------------------------------------------------

    #[test]
    fn snapshot_chat_with_messages() {
        let mut app = App::new();
        app.chat_messages = vec![
            ChatMessage {
                sender: ChatSender::User,
                content: "spin up a worker to refactor the auth module".to_string(),
                timestamp: fixed_timestamp(),
            },
            ChatMessage {
                sender: ChatSender::Vigil,
                content: "Spawning a worker for the auth refactor. I'll keep you posted."
                    .to_string(),
                timestamp: fixed_timestamp(),
            },
            ChatMessage {
                sender: ChatSender::System,
                content: "Worker #abcd is blocked (needsinput)".to_string(),
                timestamp: fixed_timestamp(),
            },
        ];

        let output = render_to_string(80, 24, |frame| {
            views::chat::render(&app, frame, frame.area());
        });
        insta::assert_snapshot!(output);
    }

    // ---------------------------------------------------------------------------
    // Terminal view snapshots
    // ---------------------------------------------------------------------------

    #[test]
    fn snapshot_terminal_empty_panes() {
        let app = App::new(); // panes is empty by default
        let output = render_to_string(80, 24, |frame| {
            views::terminal::render(&app, frame, frame.area());
        });
        insta::assert_snapshot!(output);
    }

    // ---------------------------------------------------------------------------
    // Help overlay snapshot
    // ---------------------------------------------------------------------------

    #[test]
    fn snapshot_help_overlay() {
        let mut app = App::new();
        app.show_help = true;

        // Render the session list as the background, then overlay help on top.
        let output = render_to_string(80, 24, |frame| {
            let area = frame.area();
            views::session_list::render(&app, frame, area);
            widgets::help_overlay::render(frame, area);
        });
        insta::assert_snapshot!(output);
    }

    // ---------------------------------------------------------------------------
    // Quit confirmation snapshot
    // ---------------------------------------------------------------------------

    #[test]
    fn snapshot_quit_confirmation() {
        let mut app = App::new();
        app.sessions = vec![
            test_session("abcd1234", SessionStatus::Running),
            test_session("efgh5678", SessionStatus::Running),
        ];
        app.confirm_quit = true;

        let output = render_to_string(80, 24, |frame| {
            let area = frame.area();
            views::session_list::render(&app, frame, area);
            if app.confirm_quit {
                let count = app.active_sessions_count();
                let popup = ratatui::widgets::Paragraph::new(format!(
                    "{count} sessions still running. Quit anyway? y/n"
                ))
                .style(crate::tui::theme::status_blocked())
                .alignment(ratatui::layout::Alignment::Center);
                let popup_area =
                    ratatui::layout::Rect::new(area.width / 4, area.height / 2, area.width / 2, 1);
                frame.render_widget(popup, popup_area);
            }
        });
        insta::assert_snapshot!(output);
    }

    // ---------------------------------------------------------------------------
    // Terminal too small snapshot
    // ---------------------------------------------------------------------------

    #[test]
    fn snapshot_terminal_too_small() {
        let app = App::new();
        let output = render_to_string(40, 12, |frame| {
            let area = frame.area();
            // Mirror the size-guard logic from tui/app.rs `view()`
            if area.width < 80 || area.height < 24 {
                let msg = ratatui::widgets::Paragraph::new(
                    "Terminal too small (need 80\u{d7}24)\u{2026}",
                )
                .style(crate::tui::theme::muted())
                .alignment(ratatui::layout::Alignment::Center);
                frame.render_widget(msg, area);
                return;
            }
            views::session_list::render(&app, frame, area);
        });
        insta::assert_snapshot!(output);
    }

    // ---------------------------------------------------------------------------
    // Setup view snapshot
    // ---------------------------------------------------------------------------

    #[test]
    fn snapshot_setup_view() {
        let app = App::new();
        let output = render_to_string(80, 24, |frame| {
            views::setup::render(&app, frame, frame.area());
        });
        insta::assert_snapshot!(output);
    }

    // ---------------------------------------------------------------------------
    // Chat view -- empty state
    // ---------------------------------------------------------------------------

    #[test]
    fn snapshot_chat_empty() {
        let app = App::new();
        let output = render_to_string(80, 24, |frame| {
            views::chat::render(&app, frame, frame.area());
        });
        insta::assert_snapshot!(output);
    }
}
