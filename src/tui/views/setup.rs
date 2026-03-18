use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::{state::App, theme};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(3), // title
        Constraint::Length(1), // spacer
        Constraint::Length(3), // option 1
        Constraint::Length(3), // option 2
        Constraint::Length(3), // option 3
        Constraint::Min(1),    // spacer
        Constraint::Length(1), // help
    ])
    .split(area);

    let title = Paragraph::new(vec![
        Line::styled("  \u{25c9} VIGIL", theme::header()),
        Line::raw(""),
        Line::styled("  First-time setup", theme::muted()),
    ]);
    frame.render_widget(title, chunks[0]);

    let options = [
        "  Install ultrapowers plugin (recommended)",
        "  Already installed \u{2014} verify",
        "  Skip setup",
    ];

    for (i, option) in options.iter().enumerate() {
        let is_selected = i == app.setup_selection;
        let style = if is_selected {
            theme::selected()
        } else {
            theme::text()
        };
        let cursor = if is_selected { "\u{258c}" } else { " " };
        let line = Line::from(vec![
            Span::styled(cursor, theme::border_focus()),
            Span::styled(*option, style),
        ]);
        frame.render_widget(line, chunks[i + 2]);
    }

    let help = Line::from(vec![
        Span::styled("  \u{2191}\u{2193}", theme::text()),
        Span::styled(" Navigate   ", theme::muted()),
        Span::styled("\u{23ce}", theme::text()),
        Span::styled(" Select", theme::muted()),
    ]);
    frame.render_widget(help, chunks[6]);
}
