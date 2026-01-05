use crate::agent::Agent;
use crate::monitor::SessionAggregateStatus;
use crate::terminal::Terminal;
use crate::tui::app::{App, Focus, SidebarSection};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let is_active = app.focus == Focus::Sidebar && app.sidebar_section == SidebarSection::Tasks;

    let border_color = if is_active {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let mut block = Block::default()
        .title(" Tasks ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if app.wagner.config.show_hints {
        block = block.title_bottom(" [n]ew [d]el ");
    }

    let mut items: Vec<ListItem> = Vec::new();

    for task in &app.tasks {
        let is_task_selected = app.selected_task.as_ref() == Some(&task.name);
        let is_task_highlighted = is_task_selected && app.selected_repo.is_none();
        let is_expanded = app.expanded_tasks.contains(&task.name);
        let has_repos = !task.repos.is_empty();

        let expand_icon = if !has_repos {
            "  "
        } else if is_expanded {
            "▾ "
        } else {
            "▸ "
        };

        let style = if is_task_highlighted {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if is_task_selected {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };

        let status = app.get_task_status(&task.name);
        let (status_icon, status_color) = match status {
            SessionAggregateStatus::NeedsAttention => ('◉', Color::Yellow),
            SessionAggregateStatus::Working => ('●', Color::Green),
            SessionAggregateStatus::Idle => ('○', Color::DarkGray),
            SessionAggregateStatus::Empty => ('◌', Color::DarkGray),
        };

        items.push(ListItem::new(Line::from(vec![
            Span::raw(expand_icon),
            Span::styled(
                format!("{} ", status_icon),
                Style::default().fg(status_color),
            ),
            Span::styled(&task.name, style),
        ])));

        if is_expanded {
            for repo in &task.repos {
                let is_repo_selected =
                    is_task_selected && app.selected_repo.as_ref() == Some(&repo.name);

                let repo_style = if is_repo_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let stats_str = app
                    .repo_stats
                    .get(&repo.name)
                    .map(|s| {
                        if s.file_count > 0 {
                            format!(" [+{} -{}]", s.additions, s.deletions)
                        } else {
                            String::new()
                        }
                    })
                    .unwrap_or_default();

                items.push(ListItem::new(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(&repo.name, repo_style),
                    Span::styled(stats_str, Style::default().fg(Color::Cyan)),
                ])));
            }
        }
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "  No tasks",
            Style::default().fg(Color::DarkGray),
        )])));
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "  Press 'n' to create",
            Style::default().fg(Color::DarkGray),
        )])));
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
