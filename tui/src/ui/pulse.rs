use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;

pub fn render_pulse(f: &mut Frame, app: &App, area: Rect) {
    let [top, bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let [tl, tr] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);
    let [bl, br] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(bottom);

    let state = app.state.read().unwrap();

    render_workers_summary(f, &state, tl);
    render_cluster_info(f, &state, tr);
    render_rate_limits(f, &state, bl);
    render_load_chart(f, &state, br);
}

fn render_workers_summary(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Workers ")
        .title_style(style_title());

    let lines = if let Some(ref w) = state.workers {
        let s = &w.stats;
        let healthy = w.workers.iter().filter(|w| w.is_healthy).count();
        let total_load: usize = w.workers.iter().map(|w| w.load).sum();
        vec![
            Line::from(vec![
                Span::styled("Total:    ", Style::default().fg(Color::Gray)),
                Span::styled(w.total.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Healthy:  ", Style::default().fg(Color::Gray)),
                Span::styled(healthy.to_string(), Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::styled("Load:     ", Style::default().fg(Color::Gray)),
                Span::styled(total_load.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Regular:  ", Style::default().fg(Color::Gray)),
                Span::styled(
                    s.regular_count.to_string(),
                    Style::default().fg(Color::White),
                ),
                Span::raw("  "),
                Span::styled("Prefill: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    s.prefill_count.to_string(),
                    Style::default().fg(Color::White),
                ),
                Span::raw("  "),
                Span::styled("Decode: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    s.decode_count.to_string(),
                    Style::default().fg(Color::White),
                ),
            ]),
        ]
    } else {
        vec![Line::styled(
            "No data",
            Style::default().fg(Color::DarkGray),
        )]
    };

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_cluster_info(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Cluster ")
        .title_style(style_title());

    let lines = if let Some(ref c) = state.cluster {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Node:     ", Style::default().fg(Color::Gray)),
                Span::styled(
                    c.node_name.as_deref().unwrap_or("unknown"),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("Size:     ", Style::default().fg(Color::Gray)),
                Span::styled(
                    c.cluster_size
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
            ]),
        ];

        if let Some(ref stores) = c.stores {
            lines.push(Line::from(""));
            lines.push(Line::styled("Stores:", Style::default().fg(Color::Gray)));
            for store in stores {
                let (indicator, color) = if store.healthy {
                    ("●", Color::Green)
                } else {
                    ("●", Color::Red)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {indicator} "), Style::default().fg(color)),
                    Span::styled(&store.name, Style::default().fg(Color::White)),
                ]));
            }
        }
        lines
    } else {
        vec![Line::styled(
            "No data",
            Style::default().fg(Color::DarkGray),
        )]
    };

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_rate_limits(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rate Limiting ")
        .title_style(style_title());

    let lines = if let Some(ref r) = state.rate_limits {
        let limit = r.limit.unwrap_or(0);
        let current = r.current.unwrap_or(0);
        let remaining = r.remaining.unwrap_or(0);

        let pct = if limit > 0 {
            (current as f64 / limit as f64 * 100.0) as u16
        } else {
            0
        };
        let bar_width = 30u16;
        let filled = (bar_width as f64 * pct as f64 / 100.0) as usize;
        let empty = bar_width as usize - filled;
        let bar_color = if pct > 80 {
            Color::Red
        } else if pct > 50 {
            Color::Yellow
        } else {
            Color::Green
        };

        vec![
            Line::from(vec![
                Span::styled("Limit:     ", Style::default().fg(Color::Gray)),
                Span::styled(limit.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Current:   ", Style::default().fg(Color::Gray)),
                Span::styled(current.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Remaining: ", Style::default().fg(Color::Gray)),
                Span::styled(remaining.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("█".repeat(filled), Style::default().fg(bar_color)),
                Span::styled("░".repeat(empty), Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" {pct}%"), Style::default().fg(Color::White)),
            ]),
        ]
    } else {
        vec![Line::styled(
            "No data",
            Style::default().fg(Color::DarkGray),
        )]
    };

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_load_chart(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Worker Loads ")
        .title_style(style_title());

    if let Some(ref loads) = state.loads {
        let bars: Vec<Bar> = loads
            .workers
            .iter()
            .map(|w| {
                let label = w.worker.rsplit('/').next().unwrap_or(&w.worker).to_string();
                let value = w.load.max(0) as u64;
                Bar::default()
                    .value(value)
                    .label(Line::from(label))
                    .style(Style::default().fg(Color::Cyan))
            })
            .collect();

        let chart = BarChart::default()
            .block(block)
            .data(BarGroup::default().bars(&bars))
            .bar_width(3)
            .bar_gap(1)
            .direction(ratatui::layout::Direction::Horizontal);

        f.render_widget(chart, area);
    } else {
        let paragraph = Paragraph::new("No data")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(paragraph, area);
    }
}

fn style_title() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}
