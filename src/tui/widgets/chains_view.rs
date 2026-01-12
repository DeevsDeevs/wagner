use crate::agent::Agent;
use crate::plugins::chains::{Chain, ChainSource, ChainsData, ChainsViewMode};
use crate::terminal::Terminal;
use crate::tui::app::{App, AppTab, Focus};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn draw_sidebar_tree<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let is_active = app.focus == Focus::Sidebar && app.current_tab == AppTab::Chains;

    let border_color = if is_active {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let mut block = Block::default()
        .title(" Chains ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    if app.wagner.config.show_hints {
        block = block.title_bottom(" [Enter] open ");
    }

    let Some(chains_data) = &app.plugin_states.chains.data else {
        let empty_msg = Paragraph::new("No chains found")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(empty_msg, area);
        return;
    };

    let mut items: Vec<ListItem> = Vec::new();
    let selected_idx = app.plugin_states.chains.list_state.selected();
    let mut current_idx = 0;

    let grouped = group_chains_by_task(chains_data);

    for (task_name, chains) in &grouped {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("▾ ", Style::default().fg(Color::DarkGray)),
            Span::styled(task_name.as_str(), Style::default().fg(Color::Yellow)),
        ])));

        for chain in chains {
            let is_selected = selected_idx == Some(current_idx);
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let link_count = chain.link_count();
            let chain_display_name = chain.name.split('/').last().unwrap_or(&chain.name);

            items.push(ListItem::new(Line::from(vec![
                Span::raw("  ├─ "),
                Span::styled(chain_display_name, style),
                Span::styled(
                    format!(" [{}]", link_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ])));
            current_idx += 1;
        }
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "  No chains",
            Style::default().fg(Color::DarkGray),
        )])));
    }

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn group_chains_by_task(data: &ChainsData) -> Vec<(String, Vec<&Chain>)> {
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<String, Vec<&Chain>> = BTreeMap::new();

    for repo in &data.repos {
        for chain in &repo.chains {
            let task_name =
                extract_task_name(&chain.name).unwrap_or_else(|| repo.repo_name.clone());
            groups.entry(task_name).or_default().push(chain);
        }
    }

    for chain in &data.task_local {
        let task_name = extract_task_name(&chain.name).unwrap_or_else(|| "local".to_string());
        groups.entry(task_name).or_default().push(chain);
    }

    groups.into_iter().collect()
}

fn extract_task_name(chain_name: &str) -> Option<String> {
    let parts: Vec<&str> = chain_name.split('/').collect();
    if parts.len() >= 2 {
        Some(parts[0].to_string())
    } else {
        None
    }
}

pub fn draw_main<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    match app.plugin_states.chains.view_mode {
        ChainsViewMode::ChainList => {}
        ChainsViewMode::LinkList => draw_link_list(frame, area, app),
        ChainsViewMode::LinkPreview => {
            let chunks = Layout::horizontal([
                Constraint::Percentage(30),
                Constraint::Percentage(70),
            ])
            .split(area);
            draw_link_list(frame, chunks[0], app);
            draw_link_preview(frame, chunks[1], app);
        }
    }
}

fn draw_link_list<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let cs = &app.plugin_states.chains;
    let Some(chain_idx) = cs.selected_chain_idx else {
        return;
    };

    let Some(chain) = cs.get_chain_at_index(chain_idx) else {
        return;
    };

    let source_label = match &chain.source {
        ChainSource::Repo(_) => "repo",
        ChainSource::TaskLocal(_) => "local",
    };

    let chain_display_name = chain.name.split('/').last().unwrap_or(&chain.name);
    let block = Block::default()
        .title(format!(
            " {} ({}) - {} links ",
            chain_display_name,
            source_label,
            chain.links.len()
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let mut items: Vec<ListItem> = Vec::new();
    let selected_idx = cs.selected_link_idx;

    for (idx, link) in chain.links.iter().enumerate() {
        let is_selected = selected_idx == Some(idx);
        let style = if is_selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let mut spans = vec![
            Span::styled(&link.timestamp, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(&link.slug, style),
        ];

        if let Some(summary) = &link.summary {
            let preview: String = summary.chars().take(50).collect();
            spans.push(Span::styled(
                format!(" - {}", preview),
                Style::default().fg(Color::DarkGray),
            ));
        }

        items.push(ListItem::new(Line::from(spans)));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "No links",
            Style::default().fg(Color::DarkGray),
        )])));
    }

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn draw_link_preview<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let cs = &app.plugin_states.chains;
    let Some(chain_idx) = cs.selected_chain_idx else {
        return;
    };

    let chain = match cs.get_chain_at_index(chain_idx) {
        Some(c) => c,
        None => return,
    };

    let link_info = cs.selected_link_idx.and_then(|idx| chain.links.get(idx));
    let total_lines = cs.link_content.lines().count();
    let visible_lines = area.height.saturating_sub(2) as usize;
    let scroll_pos = cs.link_scroll;

    let title = link_info
        .map(|l| {
            if total_lines > visible_lines {
                format!(
                    " {} - {} [{}/{}] ",
                    l.timestamp,
                    l.slug,
                    scroll_pos + 1,
                    total_lines.saturating_sub(visible_lines) + 1
                )
            } else {
                format!(" {} - {} ", l.timestamp, l.slug)
            }
        })
        .unwrap_or_else(|| " Chain Link ".to_string());

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let lines: Vec<Line> = cs
        .link_content
        .lines()
        .skip(scroll_pos)
        .take(visible_lines)
        .map(|line| {
            if line.starts_with("# ") {
                Line::from(Span::styled(
                    line,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("## ") {
                Line::from(Span::styled(
                    line,
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("### ") {
                Line::from(Span::styled(line, Style::default().fg(Color::Green)))
            } else if line.starts_with("- ") || line.starts_with("* ") {
                Line::from(Span::styled(line, Style::default().fg(Color::White)))
            } else if line.starts_with("```") {
                Line::from(Span::styled(line, Style::default().fg(Color::DarkGray)))
            } else {
                Line::from(Span::raw(line))
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    if app.current_tab != AppTab::Chains {
        return;
    }
    draw_main(frame, area, app);
}
