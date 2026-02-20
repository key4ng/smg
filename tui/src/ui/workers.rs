use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame,
};

use crate::{app::App, client::WorkerInfo};

pub fn render_workers(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Workers ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    let state = app.state.read().unwrap();

    let header_cells = [
        "ID", "URL", "Type", "Mode", "Runtime", "Models", "Health", "Load",
    ]
    .iter()
    .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row> = if let Some(ref wl) = state.workers {
        wl.workers
            .iter()
            .filter(|w| matches_filter(w, &app.active_filter))
            .map(|w| {
                let health_style = if w.is_healthy {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
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
                    Cell::from(truncate(&w.id, 12)),
                    Cell::from(truncate(&w.url, 30)),
                    Cell::from(w.worker_type.as_str()),
                    Cell::from(w.connection_mode.as_str()),
                    Cell::from(w.runtime_type.as_str()),
                    Cell::from(truncate(&models_display, 20)),
                    Cell::from(health_text).style(health_style),
                    Cell::from(w.load.to_string()),
                ])
            })
            .collect()
    } else {
        vec![]
    };

    let row_count = rows.len();

    let widths = [
        ratatui::layout::Constraint::Length(14),
        ratatui::layout::Constraint::Fill(1),
        ratatui::layout::Constraint::Length(10),
        ratatui::layout::Constraint::Length(8),
        ratatui::layout::Constraint::Length(10),
        ratatui::layout::Constraint::Length(22),
        ratatui::layout::Constraint::Length(10),
        ratatui::layout::Constraint::Length(6),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(Color::White),
        );

    // Build TableState from app's selected_index
    let mut table_state = TableState::default();
    if row_count > 0 {
        table_state.select(Some(app.selected_index.min(row_count.saturating_sub(1))));
    }

    f.render_stateful_widget(table, area, &mut table_state);
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
