use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::db::models::SessionStatus;
use crate::tui::{state::App, theme};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(2), // header
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
            format!("{:}", " ".repeat(area.width as usize - 30)),
            theme::text(),
        ),
        Span::styled(format!("{active} active"), theme::status_running()),
        Span::styled(" · ", theme::muted()),
        Span::styled(
            format!("{blocked} blocked"),
            if blocked > 0 {
                theme::status_blocked()
            } else {
                theme::muted()
            },
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
            let (icon, icon_style) = status_icon(&session.status);

            let id_short = &session.id[..4.min(session.id.len())];
            let project = session
                .project_path
                .split('/')
                .last()
                .unwrap_or("unknown");

            let line_style = if is_blocked(&session.status) {
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
                Span::styled(
                    format!("{:<14}", format!("{:?}", session.status).to_lowercase()),
                    line_style,
                ),
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

fn status_icon(status: &SessionStatus) -> (&'static str, Style) {
    match status {
        SessionStatus::Running => ("●", theme::status_running()),
        SessionStatus::NeedsInput | SessionStatus::AuthRequired => ("⚠", theme::status_blocked()),
        SessionStatus::Completed => ("✓", theme::status_completed()),
        SessionStatus::Failed | SessionStatus::Cancelled => ("✗", theme::status_failed()),
        _ => ("○", theme::muted()),
    }
}

fn is_blocked(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::NeedsInput | SessionStatus::AuthRequired
    )
}
