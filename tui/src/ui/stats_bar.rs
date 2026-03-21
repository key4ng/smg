use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::theme;
use crate::state::GatewayState;

pub fn render_stats_bar(f: &mut Frame, state: &GatewayState, area: Rect) {
    let bg = Style::default().bg(theme::STATS_BG);
    f.render_widget(ratatui::widgets::Block::default().style(bg), area);

    let chunks = Layout::horizontal([
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
    ])
    .split(area);

    // Cell 0: Logo + connection status
    let conn = if state.connected {
        Span::styled("● connected", Style::default().fg(theme::GREEN))
    } else {
        Span::styled("● disconnected", Style::default().fg(theme::RED))
    };
    let logo = Line::from(vec![
        Span::styled("⎔ SMG ", theme::title()),
        conn,
    ]);
    f.render_widget(Paragraph::new(logo).style(bg), chunks[0]);

    // Cell 1: Workers count
    let (total, healthy) = state
        .workers
        .as_ref()
        .map(|w| {
            let h = w.workers.iter().filter(|w| w.is_healthy).count();
            (w.total, h)
        })
        .unwrap_or((0, 0));
    let workers_line = if state.connected {
        let unhealthy = total.saturating_sub(healthy);
        let health_span = if unhealthy == 0 {
            Span::styled("all healthy", Style::default().fg(theme::GREEN))
        } else {
            Span::styled(format!("{unhealthy} unhealthy"), Style::default().fg(theme::RED))
        };
        Line::from(vec![
            Span::styled("WORKERS ", theme::label()),
            Span::styled(total.to_string(), theme::text()),
            Span::raw(" "),
            health_span,
        ])
    } else {
        Line::from(vec![
            Span::styled("WORKERS ", theme::label()),
            Span::styled("--", theme::text()),
        ])
    };
    f.render_widget(Paragraph::new(workers_line).style(bg), chunks[1]);

    // Cell 2: Models count
    let model_count = state.models.as_ref().map(|m| m.data.len()).unwrap_or(0);
    let models_line = if state.connected {
        Line::from(vec![
            Span::styled("MODELS ", theme::label()),
            Span::styled(model_count.to_string(), theme::text()),
        ])
    } else {
        Line::from(vec![
            Span::styled("MODELS ", theme::label()),
            Span::styled("--", theme::text()),
        ])
    };
    f.render_widget(Paragraph::new(models_line).style(bg), chunks[2]);

    // Cell 3: Throughput
    let throughput = state.throughput_history.back().copied().unwrap_or(0.0);
    let tp_line = if state.connected {
        Line::from(vec![
            Span::styled("THROUGHPUT ", theme::label()),
            Span::styled(format!("{throughput:.0}"), theme::text()),
            Span::styled(" tok/s", theme::label()),
        ])
    } else {
        Line::from(vec![
            Span::styled("THROUGHPUT ", theme::label()),
            Span::styled("--", theme::text()),
        ])
    };
    f.render_widget(Paragraph::new(tp_line).style(bg), chunks[3]);

    // Cell 4: Avg Load
    let (avg_load, load_color) = if state.connected {
        let avg = state.loads.as_ref()
            .map(|l| {
                if l.workers.is_empty() { 0.0 }
                else { l.workers.iter().map(|w| w.load as f64).sum::<f64>() / l.workers.len() as f64 }
            })
            .unwrap_or(0.0);
        let ratio = avg / 100.0;
        (format!("{avg:.0}%"), theme::severity(ratio))
    } else {
        ("--".to_string(), theme::TEXT_MUTED)
    };
    let load_line = Line::from(vec![
        Span::styled("LOAD ", theme::label()),
        Span::styled(avg_load, Style::default().fg(load_color)),
    ]);
    f.render_widget(Paragraph::new(load_line).style(bg), chunks[4]);
}
