use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::{sparkline, theme};
use crate::state::GatewayState;

pub fn render_stats_bar(f: &mut Frame, state: &GatewayState, area: Rect) {
    let bg = Style::default().bg(theme::STATS_BG);
    f.render_widget(ratatui::widgets::Block::default().style(bg), area);

    // Row 0: logo (left) + connection status (right)  — 1 line
    // Row 1–3: stats cards                             — 3 lines
    // Row 4: separator line                            — 1 line
    let rows = Layout::vertical([
        Constraint::Length(1), // logo row
        Constraint::Length(3), // stats cards
        Constraint::Length(1), // separator
    ])
    .split(area);

    render_logo_row(f, state, rows[0], bg);
    render_stats_cards(f, state, rows[1], bg);
    render_separator(f, rows[2]);
}

fn render_logo_row(f: &mut Frame, state: &GatewayState, area: Rect, bg: Style) {
    let cols = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Fill(1),
    ])
    .split(area);

    // Left: logo
    let logo = Line::from(vec![
        Span::styled("⎔ ", theme::label()),
        Span::styled("SMG", theme::title()),
    ]);
    f.render_widget(Paragraph::new(logo).style(bg), cols[0]);

    // Right: connection status
    let conn = if state.connected {
        Line::from(Span::styled(
            "● connected",
            Style::default().fg(theme::GREEN),
        ))
    } else {
        Line::from(Span::styled(
            "● disconnected",
            Style::default().fg(theme::RED),
        ))
    };
    f.render_widget(
        Paragraph::new(conn).alignment(Alignment::Right).style(bg),
        cols[1],
    );
}

fn render_stats_cards(f: &mut Frame, state: &GatewayState, area: Rect, bg: Style) {
    let cols = Layout::horizontal([
        Constraint::Ratio(1, 4),
        Constraint::Ratio(1, 4),
        Constraint::Ratio(1, 4),
        Constraint::Ratio(1, 4),
    ])
    .split(area);

    // Card 1: Workers
    let (total, healthy) = state
        .workers
        .as_ref()
        .map(|w| {
            let h = w.workers.iter().filter(|w| w.is_healthy).count();
            (w.total, h)
        })
        .unwrap_or((0, 0));

    let unhealthy = total.saturating_sub(healthy);
    let health_text = if !state.connected {
        ("--".to_string(), theme::TEXT_MUTED)
    } else if unhealthy == 0 {
        ("all healthy".to_string(), theme::GREEN)
    } else {
        (format!("{unhealthy} unhealthy"), theme::RED)
    };

    let workers_value = if state.connected {
        total.to_string()
    } else {
        "--".to_string()
    };

    render_card(
        f,
        cols[0],
        bg,
        "WORKERS",
        &workers_value,
        Some((&health_text.0, health_text.1)),
    );

    // Card 2: Circuit Breakers
    let cb = &state.circuit_breakers;
    let (cb_value, cb_detail) = if state.connected {
        if cb.open > 0 {
            (
                format!("{} open", cb.open),
                Some((format!("{} failures", cb.total_failures), theme::RED)),
            )
        } else if cb.closed > 0 {
            (
                "all closed".to_string(),
                Some((format!("{} workers", cb.closed), theme::GREEN)),
            )
        } else {
            ("--".to_string(), None)
        }
    } else {
        ("--".to_string(), None)
    };
    let cb_value_color = if cb.open > 0 { theme::RED } else { theme::GREEN };
    render_card_colored(
        f,
        cols[1],
        bg,
        "BREAKERS",
        &cb_value,
        cb_value_color,
        cb_detail.as_ref().map(|(s, c)| (s.as_str(), *c)),
    );

    // Card 3: Throughput
    let (tp_value, tp_detail) = if state.connected {
        let has_gen = state.throughput_history.iter().any(|&v| v > 0.0)
            && state.requests_per_sec_history.is_empty();
        if has_gen {
            let tp = state.throughput_history.back().copied().unwrap_or(0.0);
            (format_number(tp), Some(("tok/s", theme::TEXT_MUTED)))
        } else {
            let rps = state.requests_per_sec_history.back().copied().unwrap_or(0.0);
            (format!("{rps:.1}"), Some(("req/s", theme::TEXT_MUTED)))
        }
    } else {
        ("--".to_string(), None)
    };
    render_card(
        f,
        cols[2],
        bg,
        "THROUGHPUT",
        &tp_value,
        tp_detail.map(|(s, c)| (s, c)),
    );

    // Card 4: Avg Load with gauge bar
    let (avg_load, load_color) = if state.connected {
        let avg = state
            .loads
            .as_ref()
            .map(|l| {
                if l.workers.is_empty() {
                    0.0
                } else {
                    l.workers.iter().map(|w| w.load as f64).sum::<f64>()
                        / l.workers.len() as f64
                }
            })
            .unwrap_or(0.0);
        let ratio = (avg / 100.0).clamp(0.0, 1.0);
        (avg, theme::severity(ratio))
    } else {
        (0.0, theme::TEXT_MUTED)
    };

    render_load_card(f, cols[3], bg, avg_load, load_color, state.connected);
}

fn render_card(
    f: &mut Frame,
    area: Rect,
    bg: Style,
    label: &str,
    value: &str,
    detail: Option<(&str, ratatui::style::Color)>,
) {
    // 3 rows: label, big number, detail
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    // Label (centered, muted)
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(label, theme::label())))
            .alignment(Alignment::Center)
            .style(bg),
        rows[0],
    );

    // Big number (centered, bold)
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            value,
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center)
        .style(bg),
        rows[1],
    );

    // Detail line (centered)
    if let Some((text, color)) = detail {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(color),
            )))
            .alignment(Alignment::Center)
            .style(bg),
            rows[2],
        );
    }
}

fn render_card_colored(
    f: &mut Frame,
    area: Rect,
    bg: Style,
    label: &str,
    value: &str,
    value_color: ratatui::style::Color,
    detail: Option<(&str, ratatui::style::Color)>,
) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(label, theme::label())))
            .alignment(Alignment::Center)
            .style(bg),
        rows[0],
    );

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            value,
            Style::default()
                .fg(value_color)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center)
        .style(bg),
        rows[1],
    );

    if let Some((text, color)) = detail {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(color),
            )))
            .alignment(Alignment::Center)
            .style(bg),
            rows[2],
        );
    }
}

fn render_load_card(
    f: &mut Frame,
    area: Rect,
    bg: Style,
    avg_load: f64,
    load_color: ratatui::style::Color,
    connected: bool,
) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    // Label
    f.render_widget(
        Paragraph::new(Line::from(Span::styled("AVG LOAD", theme::label())))
            .alignment(Alignment::Center)
            .style(bg),
        rows[0],
    );

    // Big percentage
    let value = if connected {
        format!("{:.0}%", avg_load)
    } else {
        "--".to_string()
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &value,
            Style::default()
                .fg(load_color)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center)
        .style(bg),
        rows[1],
    );

    // Gauge bar
    if connected {
        let ratio = (avg_load / 100.0).clamp(0.0, 1.0);
        let bar_width = (area.width / 3).max(6) as usize;
        let (filled, empty, _pct) = sparkline::gauge_bar(ratio, bar_width);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(filled, Style::default().fg(load_color)),
                Span::styled(empty, Style::default().fg(theme::TEXT_MUTED)),
            ]))
            .alignment(Alignment::Center)
            .style(bg),
            rows[2],
        );
    }
}

fn render_separator(f: &mut Frame, area: Rect) {
    let line = "─".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            line,
            Style::default().fg(theme::BORDER),
        ))),
        area,
    );
}

/// Format large numbers: 1234 → "1.2k", 1234567 → "1.2M"
fn format_number(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}k", n / 1_000.0)
    } else {
        format!("{:.0}", n)
    }
}
