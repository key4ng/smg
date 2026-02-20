use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

const HELP_TEXT: &str = "\
Navigation
  1-5          Switch view (Pulse/Workers/Models/Traffic/Mesh)
  j / Down     Move selection down
  k / Up       Move selection up
  q / Ctrl+C   Quit

Workers View
  a            Add worker (enters command mode)
  d            Delete selected worker
  /            Filter workers
  :            Command mode

Commands
  :add <url> [--provider <p>] [--runtime <r>]
                   Add a worker (provider auto-sets runtime=external)
  :delete <id>     Delete a worker by ID
  :quit            Quit the TUI

Providers: openai, anthropic, gemini, xai
Runtimes:  sglang (default), vllm, trtllm, external

General
  ?            Toggle this help
  Esc          Close overlay / clear filter
";

pub fn render_help(f: &mut Frame) {
    let popup = centered_rect(60, 70, f.area());
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(HELP_TEXT)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, popup);
}

/// Return a centered `Rect` occupying `percent_x`% width and `percent_y`% height.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, vert, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);

    let [horiz] = Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center)
        .areas(vert);

    horiz
}
