use crate::agent::Agent;
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
    let is_active = app.focus == Focus::Sidebar && app.sidebar_section == SidebarSection::Tasks;

    let border_color = if is_active { Color::Cyan } else { Color::DarkGray };

    let mut block = Block::default()
        .title(" Tasks ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if app.wagner.config.show_hints {
        block = block.title_bottom(" [n]ew [d]el ");
    }

    let mut items: Vec<ListItem> = Vec::new();

    for task in &app.tasks {
        let is_selected = app.selected_task.as_ref() == Some(&task.name);
        let prefix = if is_selected { "▸ " } else { "  " };

        let style = if is_selected {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        items.push(ListItem::new(Line::from(vec![
            Span::raw(prefix),
            Span::styled(&task.name, style),
        ])));

        for repo in &task.repos {
            let repo_style = if is_selected {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            items.push(ListItem::new(Line::from(vec![
                Span::raw("    "),
                Span::styled(&repo.name, repo_style),
            ])));
        }
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  No tasks", Style::default().fg(Color::DarkGray)),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  Press 'n' to create", Style::default().fg(Color::DarkGray)),
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

    frame.render_stateful_widget(list, area, &mut app.task_list_state.clone());
}
