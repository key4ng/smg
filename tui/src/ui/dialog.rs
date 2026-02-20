use ratatui::{
    layout::{Constraint, Flex, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::App;

/// Render the delete-confirmation popup.
pub fn render_delete_dialog(f: &mut Frame, app: &App) {
    let Some(ref info) = app.confirm_delete else {
        return;
    };

    let area = f.area();

    // Center a 50×8 popup
    let [_, vert, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [popup] = Layout::horizontal([Constraint::Length(50)])
        .flex(Flex::Center)
        .areas(vert);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm Delete ")
        .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Red));

    let text = format!(
        "Delete worker?\n\nID:  {}\nURL: {}\n\n[y] confirm  [n/Esc] cancel",
        info.0, info.1,
    );

    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    f.render_widget(paragraph, popup);
}
