use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::{sparkline, theme};
use crate::{app::App, client::WorkerInfo};

pub fn render_detail(f: &mut Frame, app: &App, worker: &WorkerInfo, area: Rect) {
    let title = format!(" Worker: {} ", worker.id);
    let block = theme::panel(&title);

    // Render border block first
    f.render_widget(block, area);

    // Inner area inside the border
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    if inner.width < 3 || inner.height < 1 {
        return;
    }

    // 3-column layout
    let [left_area, mid_area, right_area] = Layout::horizontal([
        Constraint::Percentage(33),
        Constraint::Percentage(34),
        Constraint::Percentage(33),
    ])
    .areas(inner);

    let state = app.state.read().unwrap();

    // ── Left: Configuration ──
    render_config(f, worker, left_area);

    // ── Middle: Load by DP rank ──
    let worker_load = state
        .loads
        .as_ref()
        .and_then(|l| l.workers.iter().find(|wl| wl.worker == worker.url));
    render_load(f, worker, worker_load, mid_area);

    // ── Right: Throughput and Cache Hit sparklines ──
    let throughput = state.per_worker_throughput.get(&worker.url);
    let cache_hit = state.per_worker_cache_hit.get(&worker.url);
    render_sparklines(f, throughput, cache_hit, right_area);
}

fn render_config(f: &mut Frame, worker: &WorkerInfo, area: Rect) {
    if area.height == 0 {
        return;
    }

    let health_color = if worker.is_healthy {
        theme::GREEN
    } else {
        theme::RED
    };
    let health_text = if worker.is_healthy { "healthy" } else { "unhealthy" };

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("Config", Style::default().fg(theme::ACCENT)),
        ]),
        Line::from(vec![
            Span::styled("URL: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(
                truncate_url(&worker.url, area.width.saturating_sub(5) as usize),
                Style::default().fg(theme::TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("Runtime: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(&worker.runtime_type, Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Mode: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(&worker.connection_mode, Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Health: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(health_text, Style::default().fg(health_color)),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(&worker.worker_type, Style::default().fg(theme::TEXT)),
        ]),
    ];

    // Models served by this worker
    if !worker.models.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("Models ({})", worker.models.len()),
                Style::default().fg(theme::ACCENT),
            ),
        ]));
        let max_models = area.height.saturating_sub(lines.len() as u16) as usize;
        for (i, model) in worker.models.iter().enumerate() {
            if i >= max_models {
                lines.push(Line::from(vec![Span::styled(
                    format!("  +{} more", worker.models.len() - i),
                    Style::default().fg(theme::TEXT_MUTED),
                )]));
                break;
            }
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(&model.id, Style::default().fg(theme::TEXT)),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn render_load(
    f: &mut Frame,
    worker: &WorkerInfo,
    worker_load: Option<&crate::client::WorkerLoad>,
    area: Rect,
) {
    if area.height == 0 || area.width < 4 {
        return;
    }

    let mut lines: Vec<Line> = vec![Line::from(vec![Span::styled(
        "Load by DP Rank",
        Style::default().fg(theme::ACCENT),
    )])];

    if let Some(wl) = worker_load {
        if let Some(ref details) = wl.details {
            if details.loads.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    "No rank data",
                    Style::default().fg(theme::TEXT_MUTED),
                )]));
            } else {
                let bar_width = area.width.saturating_sub(14) as usize;
                let bar_width = bar_width.max(4);

                for snap in &details.loads {
                    let ratio = if snap.max_running_requests > 0 {
                        snap.num_running_reqs as f64 / snap.max_running_requests as f64
                    } else {
                        snap.token_usage
                    }
                    .clamp(0.0, 1.0);

                    let bar_color = theme::severity(ratio);
                    let (filled, empty, pct) = sparkline::gauge_bar(ratio, bar_width);

                    let rank_label = format!("R{}: ", snap.dp_rank);
                    let run_wait = format!(" {}/{}", snap.num_running_reqs, snap.num_waiting_reqs);

                    lines.push(Line::from(vec![
                        Span::styled(rank_label, Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(filled, Style::default().fg(bar_color)),
                        Span::styled(empty, Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(
                            format!(" {:3}%", pct),
                            Style::default().fg(theme::TEXT_MUTED),
                        ),
                        Span::styled(run_wait, Style::default().fg(theme::TEXT)),
                    ]));
                }

                // Summary line
                let total_running: i32 = details.loads.iter().map(|s| s.num_running_reqs).sum();
                let total_waiting: i32 = details.loads.iter().map(|s| s.num_waiting_reqs).sum();
                lines.push(Line::from(vec![
                    Span::styled("Tot: ", Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled(
                        format!("{} running / {} waiting", total_running, total_waiting),
                        Style::default().fg(theme::TEXT),
                    ),
                ]));
            }
        } else {
            // No details — show simple load number
            lines.push(Line::from(vec![
                Span::styled("Load: ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(
                    format!("{}", wl.load),
                    Style::default().fg(theme::TEXT),
                ),
            ]));
        }
    } else {
        // No load data from loads endpoint — fall back to WorkerInfo.load
        lines.push(Line::from(vec![
            Span::styled("Load: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(
                format!("{}", worker.load),
                Style::default().fg(theme::TEXT),
            ),
        ]));
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn render_sparklines(
    f: &mut Frame,
    throughput: Option<&std::collections::VecDeque<f64>>,
    cache_hit: Option<&std::collections::VecDeque<f64>>,
    area: Rect,
) {
    if area.height < 2 {
        return;
    }

    // Split vertically: label+spark for throughput, label+spark for cache hit
    let half = area.height / 2;
    let tp_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: half,
    };
    let ch_area = Rect {
        x: area.x,
        y: area.y + half,
        width: area.width,
        height: area.height - half,
    };

    // Throughput section
    {
        let label = Line::from(vec![Span::styled(
            "Throughput (tok/s)",
            Style::default().fg(theme::ACCENT),
        )]);
        f.render_widget(Paragraph::new(vec![label]), Rect { height: 1, ..tp_area });

        if tp_area.height > 1 {
            let spark_area = Rect {
                y: tp_area.y + 1,
                height: tp_area.height - 1,
                ..tp_area
            };
            if let Some(data) = throughput {
                sparkline::render_sparkline(f, data, theme::ACCENT, spark_area);
            } else {
                f.render_widget(
                    Paragraph::new("no data").style(Style::default().fg(theme::TEXT_MUTED)),
                    spark_area,
                );
            }
        }
    }

    // Cache hit section
    {
        let label = Line::from(vec![Span::styled(
            "Cache Hit Rate",
            Style::default().fg(theme::PURPLE),
        )]);
        f.render_widget(Paragraph::new(vec![label]), Rect { height: 1, ..ch_area });

        if ch_area.height > 1 {
            let spark_area = Rect {
                y: ch_area.y + 1,
                height: ch_area.height - 1,
                ..ch_area
            };
            if let Some(data) = cache_hit {
                sparkline::render_sparkline(f, data, theme::PURPLE, spark_area);
            } else {
                f.render_widget(
                    Paragraph::new("no data").style(Style::default().fg(theme::TEXT_MUTED)),
                    spark_area,
                );
            }
        }
    }
}

fn truncate_url(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
