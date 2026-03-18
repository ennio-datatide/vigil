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
