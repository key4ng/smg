use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::{app::App, types::View};

const LOGO_COLOR: Color = Color::Rgb(80, 140, 160);

const LOGO: &[&str] = &[
    " ██████  ██      ██  ██████ ",
    "██       ███    ███ ██      ",
    " ██████  ██ ████ ██ ██  ███ ",
    "      ██ ██  ██  ██ ██   ██ ",
    " ██████  ██      ██  ██████ ",
];

pub fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::BOTTOM);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Left: logo | Right: tabs + connection status
    let [logo_area, right_area] =
        Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)]).areas(inner);

    // ── Logo ──
    let logo_lines: Vec<Line> = LOGO
        .iter()
        .map(|l| Line::styled(*l, Style::default().fg(LOGO_COLOR)))
        .collect();
    f.render_widget(Paragraph::new(logo_lines), logo_area);

    // ── Right side: tabs on top, connection status below ──
    let [tabs_area, _, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(right_area);

    // Tab bar
    let tabs: Vec<Span> = View::all()
        .iter()
        .map(|v| {
            let label = format!(" {}:{} ", v.index(), v.label());
            if *v == app.view {
                Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(label, Style::default().fg(Color::Gray))
            }
        })
        .collect();
    f.render_widget(Paragraph::new(Line::from(tabs)), tabs_area);

    // Connection indicator
    let state = app.state.read().unwrap();
    let (dot, color) = if state.connected {
        ("●", Color::Green)
    } else {
        ("●", Color::Red)
    };
    let status_text = if state.connected {
        format!("{dot} connected")
    } else {
        format!("{dot} offline")
    };
    let status = Paragraph::new(status_text).style(Style::default().fg(color));
    f.render_widget(status, status_area);
}
