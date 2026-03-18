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
    let border_style = if is_active {
        theme::border_focus()
    } else {
        theme::border()
    };
    let title_style = if is_active {
        theme::header()
    } else {
        theme::muted()
    };

    let block = Block::bordered()
        .title(Line::styled(format!(" {id_short} "), title_style))
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let parser = pane.parser.read().unwrap();
    let pseudo_term = PseudoTerminal::new(parser.screen());
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
