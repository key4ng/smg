mod dialog;
mod filter;
mod footer;
mod header;
mod help;
mod pulse;
mod workers;

use ratatui::{
    layout::{Constraint, Layout},
    Frame,
};

use crate::{app::App, types::View};

/// Root render function — called once per frame.
pub fn render(f: &mut Frame, app: &App) {
    let [header_area, content_area, footer_area] = Layout::vertical([
        Constraint::Length(7),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .areas(f.area());

    header::render_header(f, app, header_area);

    match app.view {
        View::Pulse => pulse::render_pulse(f, app, content_area),
        View::Workers => workers::render_workers(f, app, content_area),
        View::Models | View::Traffic | View::Mesh => {
            render_placeholder(f, app.view, content_area);
        }
    }

    footer::render_footer(f, app, footer_area);

    // Overlays (rendered last so they draw on top)
    if app.show_help {
        help::render_help(f);
    }
    if app.confirm_delete.is_some() {
        dialog::render_delete_dialog(f, app);
    }
    filter::render_filter(f, app, footer_area);
}

fn render_placeholder(f: &mut Frame, view: View, area: ratatui::layout::Rect) {
    use ratatui::{
        style::{Color, Style},
        widgets::{Block, Borders, Paragraph},
    };

    let text = format!("{} — coming soon", view.label());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(view.label())
        .style(Style::default().fg(Color::DarkGray));
    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}
