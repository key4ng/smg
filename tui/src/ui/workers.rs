use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table, TableState},
    Frame,
};

use super::{detail, theme};
use crate::{app::App, client::WorkerInfo};

pub fn render_workers(f: &mut Frame, app: &App, area: Rect) {
    let block = theme::panel(" Workers ");

    // Split layout when detail panel is visible
    let (table_area, detail_area) = if app.show_detail {
        let split = Layout::vertical([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(area);
        (split[0], Some(split[1]))
    } else {
        (area, None)
    };

    let state = app.state.read().unwrap();

    let width = table_area.width;

    let filtered: Vec<WorkerInfo> = if let Some(ref wl) = state.workers {
        wl.workers
            .iter()
            .filter(|w| matches_filter(w, &app.active_filter))
            .cloned()
            .collect()
    } else {
        vec![]
    };

    // Build rows and table based on terminal width
    let (header, rows, widths): (Row, Vec<Row>, Vec<Constraint>) = if width < 80 {
        // Narrow: ID, Health, Load (3 columns)
        let header_cells = ["ID", "Health", "Load"]
            .iter()
            .map(|h| {
                Cell::from(*h).style(
                    Style::default()
                        .fg(theme::ACCENT)
                        .bg(theme::PANEL_BG)
                        .add_modifier(Modifier::BOLD),
                )
            });
        let header = Row::new(header_cells).height(1);

        let rows: Vec<Row> = filtered
            .iter()
            .map(|w| {
                let health_style = if w.is_healthy {
                    Style::default().fg(theme::GREEN)
                } else {
                    Style::default().fg(theme::RED)
                };
                let health_text = if w.is_healthy { "healthy" } else { "unhealthy" };

                Row::new(vec![
                    Cell::from(truncate(&w.id, 12)).style(Style::default().fg(theme::TEXT)),
                    Cell::from(health_text).style(health_style),
                    Cell::from(w.load.to_string()).style(Style::default().fg(theme::TEXT)),
                ])
                .style(Style::default().bg(theme::BG))
            })
            .collect();

        let widths = vec![
            Constraint::Fill(1),
            Constraint::Length(10),
            Constraint::Length(6),
        ];

        (header, rows, widths)
    } else if width < 100 {
        // Compact: ID, URL, Health, Load (4 columns)
        let header_cells = ["ID", "URL", "Health", "Load"]
            .iter()
            .map(|h| {
                Cell::from(*h).style(
                    Style::default()
                        .fg(theme::ACCENT)
                        .bg(theme::PANEL_BG)
                        .add_modifier(Modifier::BOLD),
                )
            });
        let header = Row::new(header_cells).height(1);

        let rows: Vec<Row> = filtered
            .iter()
            .map(|w| {
                let health_style = if w.is_healthy {
                    Style::default().fg(theme::GREEN)
                } else {
                    Style::default().fg(theme::RED)
                };
                let health_text = if w.is_healthy { "healthy" } else { "unhealthy" };

                Row::new(vec![
                    Cell::from(truncate(&w.id, 12)).style(Style::default().fg(theme::TEXT)),
                    Cell::from(truncate(&w.url, 30)).style(Style::default().fg(theme::TEXT)),
                    Cell::from(health_text).style(health_style),
                    Cell::from(w.load.to_string()).style(Style::default().fg(theme::TEXT)),
                ])
                .style(Style::default().bg(theme::BG))
            })
            .collect();

        let widths = vec![
            Constraint::Length(14),
            Constraint::Fill(1),
            Constraint::Length(10),
            Constraint::Length(6),
        ];

        (header, rows, widths)
    } else if width < 120 {
        // Medium: ID, URL, Type, Runtime, Health, Load (6 columns)
        let header_cells = ["ID", "URL", "Type", "Runtime", "Health", "Load"]
            .iter()
            .map(|h| {
                Cell::from(*h).style(
                    Style::default()
                        .fg(theme::ACCENT)
                        .bg(theme::PANEL_BG)
                        .add_modifier(Modifier::BOLD),
                )
            });
        let header = Row::new(header_cells).height(1);

        let rows: Vec<Row> = filtered
            .iter()
            .map(|w| {
                let health_style = if w.is_healthy {
                    Style::default().fg(theme::GREEN)
                } else {
                    Style::default().fg(theme::RED)
                };
                let health_text = if w.is_healthy { "healthy" } else { "unhealthy" };

                Row::new(vec![
                    Cell::from(truncate(&w.id, 12)).style(Style::default().fg(theme::TEXT)),
                    Cell::from(truncate(&w.url, 30)).style(Style::default().fg(theme::TEXT)),
                    Cell::from(w.worker_type.as_str())
                        .style(Style::default().fg(theme::TEXT_MUTED)),
                    Cell::from(w.runtime_type.as_str())
                        .style(Style::default().fg(theme::TEXT_MUTED)),
                    Cell::from(health_text).style(health_style),
                    Cell::from(w.load.to_string()).style(Style::default().fg(theme::TEXT)),
                ])
                .style(Style::default().bg(theme::BG))
            })
            .collect();

        let widths = vec![
            Constraint::Length(14),
            Constraint::Fill(1),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(6),
        ];

        (header, rows, widths)
    } else {
        // Full: all 8 columns (current)
        let header_cells = [
            "ID", "URL", "Type", "Mode", "Runtime", "Models", "Health", "Load",
        ]
        .iter()
        .map(|h| {
            Cell::from(*h).style(
                Style::default()
                    .fg(theme::ACCENT)
                    .bg(theme::PANEL_BG)
                    .add_modifier(Modifier::BOLD),
            )
        });
        let header = Row::new(header_cells).height(1);

        let rows: Vec<Row> = filtered
            .iter()
            .map(|w| {
                let health_style = if w.is_healthy {
                    Style::default().fg(theme::GREEN)
                } else {
                    Style::default().fg(theme::RED)
                };
                let health_text = if w.is_healthy { "healthy" } else { "unhealthy" };

                let model_names: String = w
                    .models
                    .iter()
                    .map(|m| m.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let models_display = if model_names.is_empty() {
                    "*".to_string()
                } else {
                    model_names
                };

                Row::new(vec![
                    Cell::from(truncate(&w.id, 12)).style(Style::default().fg(theme::TEXT)),
                    Cell::from(truncate(&w.url, 30)).style(Style::default().fg(theme::TEXT)),
                    Cell::from(w.worker_type.as_str())
                        .style(Style::default().fg(theme::TEXT_MUTED)),
                    Cell::from(w.connection_mode.as_str())
                        .style(Style::default().fg(theme::TEXT_MUTED)),
                    Cell::from(w.runtime_type.as_str())
                        .style(Style::default().fg(theme::TEXT_MUTED)),
                    Cell::from(truncate(&models_display, 20))
                        .style(Style::default().fg(theme::TEXT)),
                    Cell::from(health_text).style(health_style),
                    Cell::from(w.load.to_string()).style(Style::default().fg(theme::TEXT)),
                ])
                .style(Style::default().bg(theme::BG))
            })
            .collect();

        let widths = vec![
            Constraint::Length(14),
            Constraint::Fill(1),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(22),
            Constraint::Length(10),
            Constraint::Length(6),
        ];

        (header, rows, widths)
    };

    let row_count = rows.len();

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .fg(theme::TEXT)
                .bg(theme::BORDER)
                .add_modifier(Modifier::BOLD),
        );

    // Build TableState from app's selected_index
    let mut table_state = TableState::default();
    if row_count > 0 {
        table_state.select(Some(app.selected_index.min(row_count.saturating_sub(1))));
    }

    f.render_stateful_widget(table, table_area, &mut table_state);

    // Drop state before calling detail render (which re-acquires it)
    drop(state);

    if let Some(detail_area) = detail_area {
        if let Some(worker) = filtered.get(app.selected_index) {
            detail::render_detail(f, app, worker, detail_area);
        }
    }
}

fn matches_filter(worker: &WorkerInfo, filter: &Option<String>) -> bool {
    let Some(f) = filter else { return true };
    if f.is_empty() {
        return true;
    }
    let f_lower = f.to_lowercase();
    worker.id.to_lowercase().contains(&f_lower)
        || worker.url.to_lowercase().contains(&f_lower)
        || worker.worker_type.to_lowercase().contains(&f_lower)
        || worker.runtime_type.to_lowercase().contains(&f_lower)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
