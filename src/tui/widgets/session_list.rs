use crate::agent::Agent;
use crate::monitor::PaneStatus;
use crate::terminal::Terminal;
use crate::tui::app::{App, Focus, SidebarSection};

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let is_active = app.focus == Focus::Sidebar && app.sidebar_section == SidebarSection::Sessions;

    let border_color = if is_active { Color::Cyan } else { Color::DarkGray };

    let title = if app.wagner.config.show_hints {
        " Panes [o] "
    } else {
        " Panes "
    };

    let mut block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if app.wagner.config.show_hints {
        block = block.title_bottom(" [p]ane [s]end [a]ttach ");
    }

    let mut items: Vec<ListItem> = Vec::new();

    for pane in app.panes.iter() {
        let is_selected = app.selected_pane.as_ref() == Some(&pane.0);
        let status = app.pane_statuses.get(&pane.0);

        let (icon, status_color) = match status {
            Some(PaneStatus::Agent { status, .. }) if status.is_waiting() => ('◉', Color::Yellow),
            Some(PaneStatus::Agent { status, .. }) if status.is_active() => ('●', Color::Green),
            Some(PaneStatus::Terminal(ts)) if ts.is_active() => ('●', Color::Blue),
            Some(_) => ('○', Color::DarkGray),
            None => ('?', Color::DarkGray),
        };

        let name_style = if is_selected {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let status_label = status.map(|s| s.label()).unwrap_or_default();

        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", icon), Style::default().fg(status_color)),
            Span::styled(&pane.1, name_style),
            Span::styled(format!(" {}", status_label), Style::default().fg(Color::DarkGray)),
        ])));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  No panes", Style::default().fg(Color::DarkGray)),
        ])));
    }

    let highlight_style = if is_active {
        Style::default().bg(Color::DarkGray)
    } else {
        Style::default()
    };

    let list = List::new(items)
        .block(block)
        .highlight_style(highlight_style);

    frame.render_stateful_widget(list, area, &mut app.pane_list_state.clone());
}
