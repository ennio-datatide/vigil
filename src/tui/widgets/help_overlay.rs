use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect) {
    // Semi-transparent background
    let bg = Block::new().style(Style::new().bg(theme::BG));
    frame.render_widget(bg, area);

    let width = 50.min(area.width.saturating_sub(4));
    let height = 16.min(area.height.saturating_sub(4));
    let x = (area.width - width) / 2;
    let y = (area.height - height) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let block = Block::bordered()
        .title(Line::styled(" Help ", theme::header()))
        .border_style(theme::border_focus());

    let bindings = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  \u{2191}\u{2193}        ", theme::text()),
            Span::styled("Navigate session list", theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("  \u{23ce}         ", theme::text()),
            Span::styled("Open terminal pane", theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("  c         ", theme::text()),
            Span::styled("Open chat with Vigil", theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("  Tab       ", theme::text()),
            Span::styled("Switch pane (terminal view)", theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl-D    ", theme::text()),
            Span::styled("Close pane", theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("  Esc       ", theme::text()),
            Span::styled("Go back", theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("  1         ", theme::text()),
            Span::styled("Session list", theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("  ?         ", theme::text()),
            Span::styled("Toggle this help", theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("  q         ", theme::text()),
            Span::styled("Quit", theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl-C    ", theme::text()),
            Span::styled("Force quit", theme::muted()),
        ]),
        Line::raw(""),
        Line::styled("  Press any key to close", theme::muted()),
    ];

    let paragraph = Paragraph::new(bindings).block(block);
    frame.render_widget(paragraph, popup_area);
}
