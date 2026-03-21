use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use super::theme;
use super::sparkline;

pub fn render_pulse(f: &mut Frame, app: &App, area: Rect) {
    let width = area.width;

    if width < 80 {
        // Narrow: left column only, takes full width
        let left = Layout::vertical([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(area);

        let state = app.state.read().unwrap();
        render_worker_health(f, &state, left[0]);
        render_cluster(f, &state, left[1]);
        render_rate_limits(f, &state, left[2]);
        return;
    }

    let columns = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Percentage(50),
    ])
    .split(area);

    let left = Layout::vertical([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(columns[0]);

    let right = Layout::vertical([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(columns[1]);

    let state = app.state.read().unwrap();

    render_worker_health(f, &state, left[0]);
    render_cluster(f, &state, left[1]);
    render_rate_limits(f, &state, left[2]);

    if width < 100 {
        // Compact: right column with text numbers instead of sparklines
        render_throughput_compact(f, &state, right[0]);
        render_token_usage(f, &state, right[1]);
        render_cache_hit_compact(f, &state, right[2]);
    } else {
        // Full layout with sparklines
        render_throughput(f, &state, right[0]);
        render_token_usage(f, &state, right[1]);
        render_cache_hit(f, &state, right[2]);
    }
}

fn render_worker_health(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = theme::panel(" WORKER HEALTH ");

    let lines = if let Some(ref w) = state.workers {
        let healthy = w.workers.iter().filter(|w| w.is_healthy).count();
        let unhealthy = w.total.saturating_sub(healthy);
        let s = &w.stats;

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Healthy:   ", theme::label()),
                Span::styled(healthy.to_string(), theme::text().fg(theme::GREEN)),
            ]),
        ];

        if unhealthy > 0 {
            lines.push(Line::from(vec![
                Span::styled("Unhealthy: ", theme::label()),
                Span::styled(unhealthy.to_string(), theme::text().fg(theme::RED)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("BY TYPE  ", theme::label()),
            Span::styled("regular: ", theme::label()),
            Span::styled(s.regular_count.to_string(), theme::text()),
            Span::styled("  prefill: ", theme::label()),
            Span::styled(s.prefill_count.to_string(), theme::text()),
            Span::styled("  decode: ", theme::label()),
            Span::styled(s.decode_count.to_string(), theme::text()),
        ]));

        lines
    } else {
        vec![Line::styled("No data", theme::label())]
    };

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_cluster(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = theme::panel(" CLUSTER ");

    let lines = if let Some(ref c) = state.cluster {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Node:  ", theme::label()),
                Span::styled(
                    c.node_name.as_deref().unwrap_or("unknown"),
                    theme::text(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Size:  ", theme::label()),
                Span::styled(
                    c.cluster_size
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "-".into()),
                    theme::text(),
                ),
            ]),
        ];

        if let Some(ref stores) = c.stores {
            lines.push(Line::from(""));
            lines.push(Line::styled("Stores:", theme::label()));
            for store in stores {
                let color = if store.healthy { theme::GREEN } else { theme::RED };
                lines.push(Line::from(vec![
                    Span::styled("  ● ", ratatui::style::Style::default().fg(color)),
                    Span::styled(&store.name, theme::text()),
                ]));
            }
        }

        lines
    } else {
        vec![Line::styled("No data", theme::label())]
    };

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_rate_limits(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = theme::panel(" RATE LIMITS ");

    let lines = if let Some(ref r) = state.rate_limits {
        let limit = r.limit.unwrap_or(0);
        let current = r.current.unwrap_or(0);
        let remaining = r.remaining.unwrap_or(0);

        let ratio = if limit > 0 {
            current as f64 / limit as f64
        } else {
            0.0
        };

        let severity_color = theme::severity(ratio);
        let (filled, empty, pct) = sparkline::gauge_bar(ratio, 28);

        vec![
            Line::from(vec![
                Span::styled("Limit:     ", theme::label()),
                Span::styled(limit.to_string(), theme::text()),
            ]),
            Line::from(vec![
                Span::styled("Current:   ", theme::label()),
                Span::styled(current.to_string(), theme::text()),
            ]),
            Line::from(vec![
                Span::styled("Remaining: ", theme::label()),
                Span::styled(remaining.to_string(), theme::text()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(filled, ratatui::style::Style::default().fg(severity_color)),
                Span::styled(empty, ratatui::style::Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(format!(" {pct}%"), theme::text()),
            ]),
        ]
    } else {
        vec![Line::styled("No data", theme::label())]
    };

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_throughput(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = theme::panel(" THROUGHPUT ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.throughput_history.is_empty() {
        f.render_widget(
            Paragraph::new(Line::styled("No data", theme::label())),
            inner,
        );
        return;
    }

    // Header: latest value
    let latest = state.throughput_history.back().copied().unwrap_or(0.0);
    let header_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Latest: ", theme::label()),
            Span::styled(format!("{:.1} tok/s", latest), theme::text().fg(theme::GREEN)),
        ])),
        header_area,
    );

    // Sparkline area
    if inner.height > 2 {
        let sparkline_area = Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: inner.height.saturating_sub(2),
        };
        sparkline::render_sparkline(f, &state.throughput_history, theme::GREEN, sparkline_area);
    }

    // Time labels
    if inner.height >= 2 {
        let label_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        let padding = " ".repeat(label_area.width.saturating_sub(7) as usize);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("-60s", theme::label()),
                Span::raw(padding),
                Span::styled("now", theme::label()),
            ])),
            label_area,
        );
    }
}

fn render_throughput_compact(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = theme::panel(" THROUGHPUT ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.throughput_history.is_empty() {
        f.render_widget(
            Paragraph::new(Line::styled("No data", theme::label())),
            inner,
        );
        return;
    }

    let latest = state.throughput_history.back().copied().unwrap_or(0.0);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Latest: ", theme::label()),
            Span::styled(format!("{:.1} tok/s", latest), theme::text().fg(theme::GREEN)),
        ])),
        inner,
    );
}

fn render_token_usage(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = theme::panel(" TOKEN USAGE BY WORKER ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = if let Some(ref loads) = state.loads {
        let bar_width = (inner.width.saturating_sub(20)) as usize;
        let bar_width = bar_width.max(8);

        loads
            .workers
            .iter()
            .map(|wl| {
                let ratio = wl
                    .details
                    .as_ref()
                    .map(|d| d.effective_token_usage())
                    .unwrap_or(0.0);

                let name = wl.worker.rsplit('/').next().unwrap_or(&wl.worker);
                let short_name = if name.len() > 14 {
                    &name[..14]
                } else {
                    name
                };

                let color = theme::severity(ratio);
                let (filled, empty, pct) = sparkline::gauge_bar(ratio, bar_width);

                Line::from(vec![
                    Span::styled(format!("{:<14} ", short_name), theme::text()),
                    Span::styled(filled, ratatui::style::Style::default().fg(color)),
                    Span::styled(empty, ratatui::style::Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled(format!(" {pct}%"), theme::label()),
                ])
            })
            .collect::<Vec<_>>()
    } else {
        vec![Line::styled("No data", theme::label())]
    };

    f.render_widget(Paragraph::new(lines), inner);
}

fn render_cache_hit(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = theme::panel(" CACHE HIT RATE ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.cache_hit_history.is_empty() {
        f.render_widget(
            Paragraph::new(Line::styled("No data", theme::label())),
            inner,
        );
        return;
    }

    // Header: latest value
    let latest = state.cache_hit_history.back().copied().unwrap_or(0.0);
    let header_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Latest: ", theme::label()),
            Span::styled(
                format!("{:.1}%", latest * 100.0),
                theme::text().fg(theme::PURPLE),
            ),
        ])),
        header_area,
    );

    // Sparkline area
    if inner.height > 2 {
        let sparkline_area = Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: inner.height.saturating_sub(2),
        };
        sparkline::render_sparkline(f, &state.cache_hit_history, theme::PURPLE, sparkline_area);
    }

    // Time labels
    if inner.height >= 2 {
        let label_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        let padding = " ".repeat(label_area.width.saturating_sub(7) as usize);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("-60s", theme::label()),
                Span::raw(padding),
                Span::styled("now", theme::label()),
            ])),
            label_area,
        );
    }
}

fn render_cache_hit_compact(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = theme::panel(" CACHE HIT RATE ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.cache_hit_history.is_empty() {
        f.render_widget(
            Paragraph::new(Line::styled("No data", theme::label())),
            inner,
        );
        return;
    }

    let latest = state.cache_hit_history.back().copied().unwrap_or(0.0);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Latest: ", theme::label()),
            Span::styled(
                format!("{:.1}%", latest * 100.0),
                theme::text().fg(theme::PURPLE),
            ),
        ])),
        inner,
    );
}
