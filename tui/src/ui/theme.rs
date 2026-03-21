use ratatui::style::{Color, Modifier, Style};

// Tokyo Night palette
pub const BG: Color = Color::Rgb(26, 27, 38);         // #1a1b26
pub const PANEL_BG: Color = Color::Rgb(36, 40, 59);   // #24283b
pub const STATS_BG: Color = Color::Rgb(30, 32, 48);   // #1e2030
pub const BORDER: Color = Color::Rgb(59, 66, 97);     // #3b4261
pub const TEXT: Color = Color::Rgb(192, 202, 245);     // #c0caf5
pub const TEXT_MUTED: Color = Color::Rgb(86, 95, 137); // #565f89
pub const ACCENT: Color = Color::Rgb(122, 162, 247);   // #7aa2f7
pub const GREEN: Color = Color::Rgb(158, 206, 106);    // #9ece6a
pub const YELLOW: Color = Color::Rgb(224, 175, 104);   // #e0af68
pub const RED: Color = Color::Rgb(247, 118, 142);      // #f7768e
pub const PURPLE: Color = Color::Rgb(187, 154, 247);   // #bb9af7

/// Style for panel titles (accent + bold).
pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Style for section labels (muted + uppercase convention).
pub fn label() -> Style {
    Style::default().fg(TEXT_MUTED)
}

/// Style for primary text.
pub fn text() -> Style {
    Style::default().fg(TEXT)
}

/// Severity color for a 0.0–1.0 ratio.
pub fn severity(ratio: f64) -> Color {
    if ratio < 0.5 {
        GREEN
    } else if ratio < 0.8 {
        YELLOW
    } else {
        RED
    }
}

/// Standard panel block with border.
pub fn panel(title: &str) -> ratatui::widgets::Block<'_> {
    ratatui::widgets::Block::default()
        .title(title)
        .title_style(self::title())
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG))
}
