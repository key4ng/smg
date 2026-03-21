use ratatui::{
    layout::{Constraint, Flex, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::App,
    types::{ActionMenuItem, AddMenuState},
};

use super::theme;

/// Render the worker action menu overlay.
pub fn render_action_menu(f: &mut Frame, app: &App) {
    if !app.show_action_menu {
        return;
    }

    let items = ActionMenuItem::all();
    let height = items.len() as u16 + 4; // borders + title + hint
    let width = 36u16;

    let area = f.area();
    let [_, vert, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [popup] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(vert);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Worker Actions ")
        .title_style(theme::title())
        .border_style(Style::default().fg(theme::BORDER))
        .style(Style::default().bg(theme::PANEL_BG));

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == app.action_menu_index {
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT)
            };
            let label = format!(" {} ", item.label());
            ListItem::new(Line::from(Span::styled(label, style)))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(app.action_menu_index));

    // We need a hint line at the bottom; render the block first, then list inside
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Split inner: list + hint
    let [list_area, hint_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    f.render_stateful_widget(
        List::new(list_items),
        list_area,
        &mut list_state,
    );

    let hint = Paragraph::new(Line::from(vec![
        Span::styled("↑↓/jk", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" navigate  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Enter", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" select  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)),
    ]));
    f.render_widget(hint, hint_area);
}

/// Render the add worker menu overlay.
pub fn render_add_menu(f: &mut Frame, app: &App) {
    let Some(ref state) = app.add_menu_state else {
        return;
    };

    match state {
        AddMenuState::SelectProvider => render_provider_select(f),
        AddMenuState::EnterApiKey { provider, input } => {
            render_api_key_input(f, provider.label(), input);
        }
    }
}

fn render_provider_select(f: &mut Frame) {
    let width = 44u16;
    let height = 14u16;

    let area = f.area();
    let [_, vert, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [popup] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(vert);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Add Worker ")
        .title_style(theme::title())
        .border_style(Style::default().fg(theme::BORDER))
        .style(Style::default().bg(theme::PANEL_BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines = vec![
        Line::from(Span::styled("Select provider:", Style::default().fg(theme::TEXT).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [1] ", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("OpenAI", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled(" [2] ", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("Anthropic", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled(" [3] ", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("xAI (Grok)", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled(" [4] ", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("Gemini", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled(" [5] ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Local SGLang  ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("(coming soon)", Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::ITALIC)),
        ]),
        Line::from(vec![
            Span::styled(" [6] ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Local vLLM    ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("(coming soon)", Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::ITALIC)),
        ]),
        Line::from(vec![
            Span::styled(" [7] ", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("Custom URL", Style::default().fg(theme::TEXT)),
        ]),
        Line::from(""),
        Line::from(Span::styled("Esc to cancel", Style::default().fg(theme::TEXT_MUTED))),
    ];

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
}

fn render_api_key_input(f: &mut Frame, provider_label: &str, input: &str) {
    let width = 50u16;
    let height = 8u16;

    let area = f.area();
    let [_, vert, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [popup] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(vert);

    f.render_widget(Clear, popup);

    let title = format!(" Add {} Worker ", provider_label);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(theme::title())
        .border_style(Style::default().fg(theme::BORDER))
        .style(Style::default().bg(theme::PANEL_BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Masked API key display
    let masked: String = "*".repeat(input.len());
    let display = if input.is_empty() {
        Span::styled("Enter API key...", Style::default().fg(theme::TEXT_MUTED))
    } else {
        Span::styled(masked, Style::default().fg(theme::TEXT))
    };

    let lines = vec![
        Line::from(Span::styled("API Key:", Style::default().fg(theme::TEXT).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(display),
        Line::from(""),
        Line::from(Span::styled("Enter to confirm  Esc to cancel", Style::default().fg(theme::TEXT_MUTED))),
    ];

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
}
