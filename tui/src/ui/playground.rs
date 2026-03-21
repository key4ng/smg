use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::theme;
use crate::app::App;

pub fn render_playground(f: &mut Frame, app: &App, area: Rect) {
    // Layout: messages (fill) + input (3 lines)
    let [messages_area, input_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .areas(area);

    render_messages(f, app, messages_area);
    render_input(f, app, input_area);
}

fn render_messages(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(format!(" Chat — {} ", app.playground_model))
        .title_style(theme::title())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER))
        .style(Style::default().bg(theme::BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.playground_messages.is_empty() {
        let help = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Type a message and press Enter to chat.",
                theme::label(),
            )),
            Line::from(Span::styled(
                "Tab to cycle models. Esc to cancel streaming.",
                theme::label(),
            )),
        ];
        f.render_widget(Paragraph::new(help), inner);
        return;
    }

    // Build all lines from messages
    let mut lines: Vec<Line> = Vec::new();

    for msg in &app.playground_messages {
        let (prefix, prefix_style) = match msg.role.as_str() {
            "user" => (
                "You: ",
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            "assistant" => (
                "AI: ",
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            _ => (
                "",
                Style::default()
                    .fg(theme::YELLOW)
                    .add_modifier(Modifier::BOLD),
            ),
        };

        // First line gets the role prefix
        let content_lines: Vec<&str> = msg.content.split('\n').collect();
        for (i, content_line) in content_lines.iter().enumerate() {
            if i == 0 {
                lines.push(Line::from(vec![
                    Span::styled(prefix, prefix_style),
                    Span::styled(*content_line, theme::text()),
                ]));
            } else {
                // Indent continuation lines
                let indent = " ".repeat(prefix.len());
                lines.push(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(*content_line, theme::text()),
                ]));
            }
        }

        // Show streaming cursor
        if msg.role == "assistant" && app.playground_streaming
            && std::ptr::eq(msg, app.playground_messages.last().unwrap())
        {
            if let Some(last_line) = lines.last_mut() {
                last_line.spans.push(Span::styled("▊", Style::default().fg(theme::ACCENT)));
            }
        }

        lines.push(Line::from("")); // blank line between messages
    }

    // Calculate scroll
    let total_lines = lines.len() as u16;
    let visible = inner.height;
    let max_scroll = total_lines.saturating_sub(visible);
    let scroll = if app.playground_scroll >= max_scroll {
        max_scroll
    } else {
        app.playground_scroll
    };

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.playground_streaming {
        " Streaming... (Esc to stop) "
    } else {
        " Message (Enter to send, Tab to change model) "
    };

    let block = Block::default()
        .title(title)
        .title_style(if app.playground_streaming {
            Style::default().fg(theme::YELLOW)
        } else {
            theme::title()
        })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.playground_streaming {
            theme::YELLOW
        } else {
            theme::BORDER
        }))
        .style(Style::default().bg(theme::BG));

    let input_text = if app.playground_streaming {
        String::new()
    } else {
        format!("{}▊", app.playground_input)
    };

    let paragraph = Paragraph::new(input_text)
        .style(theme::text())
        .block(block);
    f.render_widget(paragraph, area);
}
