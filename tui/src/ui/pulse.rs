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
    let state = app.state.read().unwrap();

    // Determine which panels have data
    let has_cluster = state.cluster.is_some();
    let has_gpus = state.gpus.is_some();
    let has_node_panel = has_cluster || has_gpus;
    let has_token_usage = state
        .loads
        .as_ref()
        .map(|l| l.workers.iter().any(|w| w.details.is_some()))
        .unwrap_or(false);

    if width < 80 {
        // Narrow: single column
        let mut constraints: Vec<Constraint> = vec![Constraint::Fill(1)]; // Worker Health
        if has_node_panel {
            constraints.push(Constraint::Fill(1)); // Node/GPU
        }
        constraints.push(Constraint::Fill(1)); // Rate Limits

        let rows = Layout::vertical(constraints).split(area);
        let mut i = 0;
        render_worker_health(f, &state, rows[i]);
        i += 1;
        if has_node_panel {
            render_node_status(f, &state, rows[i]);
            i += 1;
        }
        render_rate_limits(f, &state, rows[i]);
        return;
    }

    let columns = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Percentage(50),
    ])
    .split(area);

    // Left column: Worker Health + (Node/GPU if available) + Rate Limits
    let left = if has_node_panel {
        Layout::vertical([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(columns[0])
    } else {
        Layout::vertical([
            Constraint::Ratio(1, 2),
            Constraint::Ratio(1, 2),
        ])
        .split(columns[0])
    };

    // Right column: Throughput + (Token Usage if available) + Cache Hit
    let right = if has_token_usage {
        Layout::vertical([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(columns[1])
    } else {
        Layout::vertical([
            Constraint::Ratio(1, 2),
            Constraint::Ratio(1, 2),
        ])
        .split(columns[1])
    };

    // Left panels
    render_worker_health(f, &state, left[0]);
    if has_node_panel {
        render_node_status(f, &state, left[1]);
        render_rate_limits(f, &state, left[2]);
    } else {
        render_rate_limits(f, &state, left[1]);
    }

    // Right panels
    if width < 100 {
        render_throughput_compact(f, &state, right[0]);
        if has_token_usage {
            render_token_usage(f, &state, right[1]);
            render_cache_hit_compact(f, &state, right[2]);
        } else {
            render_cache_hit_compact(f, &state, right[1]);
        }
    } else {
        render_throughput(f, &state, right[0]);
        if has_token_usage {
            render_token_usage(f, &state, right[1]);
            render_cache_hit(f, &state, right[2]);
        } else {
            render_cache_hit(f, &state, right[1]);
        }
    }
}

fn render_worker_health(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    let block = theme::panel(" WORKER HEALTH ");

    let lines = if let Some(ref w) = state.workers {
        w.workers
            .iter()
            .map(|worker| {
                let (dot_color, status) = if worker.is_healthy {
                    (theme::GREEN, "healthy")
                } else {
                    (theme::RED, "unhealthy")
                };

                // Short name from URL (e.g. "api.openai.com" from "https://api.openai.com/v1")
                let name = worker
                    .url
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_end_matches('/')
                    .split('/')
                    .next()
                    .unwrap_or(&worker.url);

                let rt = if worker.runtime_type.is_empty() {
                    "unknown"
                } else {
                    &worker.runtime_type
                };

                let status_style = if worker.is_healthy {
                    ratatui::style::Style::default()
                        .fg(theme::GREEN)
                        .add_modifier(ratatui::style::Modifier::BOLD)
                } else {
                    ratatui::style::Style::default()
                        .fg(theme::RED)
                        .add_modifier(ratatui::style::Modifier::BOLD)
                };

                Line::from(vec![
                    Span::styled("● ", ratatui::style::Style::default().fg(dot_color)),
                    Span::styled(format!("{name} "), theme::text()),
                    Span::styled(format!("{rt} "), theme::label()),
                    Span::styled(status, status_style),
                ])
            })
            .collect()
    } else {
        vec![Line::styled("No data", theme::label())]
    };

    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Renders either GPU status (single-node) or cluster info (multi-node).
fn render_node_status(f: &mut Frame, state: &crate::state::GatewayState, area: Rect) {
    if let Some(ref gpus) = state.gpus {
        render_gpu_status(f, gpus, area);
    } else if let Some(ref c) = state.cluster {
        render_cluster(f, c, area);
    }
}

fn render_gpu_status(f: &mut Frame, gpus: &[crate::state::GpuInfo], area: Rect) {
    let block = theme::panel(" GPUs ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines: Vec<Line> = gpus
        .iter()
        .map(|gpu| {
            let mem_ratio = if gpu.memory_total_mb > 0 {
                gpu.memory_used_mb as f64 / gpu.memory_total_mb as f64
            } else {
                0.0
            };
            let util_ratio = gpu.utilization_pct as f64 / 100.0;

            let color = theme::severity(util_ratio);
            let bar_width = (inner.width / 4).max(6) as usize;
            let (filled, empty, _) = sparkline::gauge_bar(mem_ratio, bar_width);

            let mem_gb_used = gpu.memory_used_mb as f64 / 1024.0;
            let mem_gb_total = gpu.memory_total_mb as f64 / 1024.0;

            // Shorten GPU name (e.g. "NVIDIA A100-SXM4-80GB" → "A100-80GB")
            let short_name = shorten_gpu_name(&gpu.name);

            Line::from(vec![
                Span::styled(format!("GPU{} ", gpu.index), theme::label()),
                Span::styled(format!("{:<12} ", short_name), theme::text()),
                Span::styled(filled, ratatui::style::Style::default().fg(color)),
                Span::styled(empty, ratatui::style::Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(
                    format!(" {:.1}/{:.0}G ", mem_gb_used, mem_gb_total),
                    theme::label(),
                ),
                Span::styled(format!("{}% ", gpu.utilization_pct), ratatui::style::Style::default().fg(color)),
                Span::styled(format!("{}°C", gpu.temperature_c), theme::label()),
            ])
        })
        .collect();

    f.render_widget(Paragraph::new(lines), inner);
}

fn shorten_gpu_name(name: &str) -> String {
    // Strip common prefixes
    let name = name
        .trim_start_matches("NVIDIA ")
        .trim_start_matches("Tesla ")
        .trim_start_matches("GeForce ");
    // Truncate if too long
    if name.len() > 12 {
        name[..12].to_string()
    } else {
        name.to_string()
    }
}

fn render_cluster(
    f: &mut Frame,
    c: &crate::client::ClusterStatusResponse,
    area: Rect,
) {
    let block = theme::panel(" CLUSTER ");

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
    // Determine if we have gen_throughput data or should use req/s fallback
    let use_rps = !state.requests_per_sec_history.is_empty()
        && !state.throughput_history.iter().any(|&v| v > 0.0 && state.requests_per_sec_history.is_empty());

    let title = if use_rps { " THROUGHPUT (req/s) " } else { " THROUGHPUT " };
    let block = theme::panel(title);
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
    let unit = if use_rps { "req/s" } else { "tok/s" };
    let header_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Latest: ", theme::label()),
            Span::styled(format!("{:.1} {}", latest, unit), theme::text().fg(theme::GREEN)),
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
