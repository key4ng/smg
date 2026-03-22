use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::theme;
use crate::app::{App, LogLevel};

pub fn render_logs(f: &mut Frame, app: &App, area: Rect) {
    let total = app.log_entries.len();
    let title = format!(" Logs ({total}) ");
    let block = Block::default()
        .title(title)
        .title_style(theme::title())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER))
        .style(Style::default().bg(theme::BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.log_entries.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No log entries yet.",
                theme::label(),
            ))),
            inner,
        );
        return;
    }

    let lines: Vec<Line> = app
        .log_entries
        .iter()
        .map(|entry| {
            let time = entry.timestamp.format("%H:%M:%S").to_string();
            let (level_str, level_color) = match entry.level {
                LogLevel::Info => ("INFO", theme::GREEN),
                LogLevel::Warn => ("WARN", theme::YELLOW),
                LogLevel::Error => ("ERR ", theme::RED),
            };

            Line::from(vec![
                Span::styled(
                    format!("{time} "),
                    Style::default().fg(theme::TEXT_MUTED),
                ),
                Span::styled(
                    format!("{level_str} "),
                    Style::default()
                        .fg(level_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(&entry.message, theme::text()),
            ])
        })
        .collect();

    let total_lines = lines.len() as u16;
    let visible = inner.height;
    let max_scroll = total_lines.saturating_sub(visible);
    let scroll = if app.log_scroll >= max_scroll {
        max_scroll
    } else {
        app.log_scroll
    };

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
}
