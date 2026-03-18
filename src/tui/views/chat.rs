use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::{
    state::{App, ChatSender},
    theme,
};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(2), // header
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
        let sender_style = match msg.sender {
            ChatSender::Vigil => theme::vigil_message(),
            ChatSender::User => theme::user_message(),
            ChatSender::System => theme::status_blocked(),
        };
        let sender_name = match msg.sender {
            ChatSender::Vigil => "vigil",
            ChatSender::User => "you",
            ChatSender::System => "system",
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

    let total_lines = lines.len();
    let paragraph =
        Paragraph::new(lines).scroll((total_lines.saturating_sub(area.height as usize) as u16, 0));
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
