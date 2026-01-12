use crate::agent::Agent;
use crate::plugins::chains::ChainSource;
use crate::terminal::Terminal;
use crate::tui::app::{App, AppTab, ChainsViewMode};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    if app.current_tab != AppTab::Chains {
        return;
    }

    match app.chains_view_mode {
        ChainsViewMode::ChainList => draw_chain_list(frame, area, app),
        ChainsViewMode::LinkList => draw_link_list(frame, area, app),
        ChainsViewMode::LinkPreview => draw_link_preview(frame, area, app),
    }
}

fn draw_chain_list<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let block = Block::default()
        .title(" Chains ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let Some(chains_data) = &app.chains_data else {
        let empty_msg = Paragraph::new("No chains found. Use /chain-link to create one.")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(empty_msg, area);
        return;
    };

    let mut items: Vec<ListItem> = Vec::new();
    let selected_idx = app.chains_list_state.selected();
    let mut current_idx = 0;

    for repo in &chains_data.repos {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("▾ ", Style::default().fg(Color::DarkGray)),
            Span::styled(&repo.repo_name, Style::default().fg(Color::Blue)),
            Span::styled(" (repo)", Style::default().fg(Color::DarkGray)),
        ])));

        for chain in &repo.chains {
            let is_selected = selected_idx == Some(current_idx);
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let link_count = chain.link_count();
            let link_label = if link_count == 1 { "link" } else { "links" };

            items.push(ListItem::new(Line::from(vec![
                Span::raw("  ├─ "),
                Span::styled(&chain.name, style),
                Span::styled(
                    format!(" [{} {}]", link_count, link_label),
                    Style::default().fg(Color::DarkGray),
                ),
            ])));
            current_idx += 1;
        }
    }

    if !chains_data.task_local.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("▾ ", Style::default().fg(Color::DarkGray)),
            Span::styled("Task-local", Style::default().fg(Color::Yellow)),
            Span::styled(" (not synced)", Style::default().fg(Color::DarkGray)),
        ])));

        for chain in &chains_data.task_local {
            let is_selected = selected_idx == Some(current_idx);
            let style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let link_count = chain.link_count();
            let link_label = if link_count == 1 { "link" } else { "links" };

            items.push(ListItem::new(Line::from(vec![
                Span::raw("  ├─ "),
                Span::styled(&chain.name, style),
                Span::styled(
                    format!(" [{} {}]", link_count, link_label),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(" local", Style::default().fg(Color::Yellow)),
            ])));
            current_idx += 1;
        }
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "  No chains found",
            Style::default().fg(Color::DarkGray),
        )])));
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "  Use /chain-link to create one",
            Style::default().fg(Color::DarkGray),
        )])));
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::DarkGray));

    frame.render_stateful_widget(list, area, &mut app.chains_list_state.clone());
}

fn draw_link_list<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let Some(chain_idx) = app.selected_chain_idx else {
        return;
    };

    let Some(chain) = app.get_chain_at_index(chain_idx) else {
        return;
    };

    let source_label = match &chain.source {
        ChainSource::Repo(_) => "repo",
        ChainSource::TaskLocal(_) => "local",
    };

    let block = Block::default()
        .title(format!(" {} ({}) ", chain.name, source_label))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let mut items: Vec<ListItem> = Vec::new();
    let selected_idx = app.selected_link_idx;

    for (idx, link) in chain.links.iter().enumerate() {
        let is_selected = selected_idx == Some(idx);
        let style = if is_selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let summary_preview = link
            .summary
            .as_ref()
            .map(|s: &String| {
                let truncated = if s.len() > 50 {
                    format!("{}...", &s[..50])
                } else {
                    s.clone()
                };
                truncated.replace('\n', " ")
            })
            .unwrap_or_default();

        items.push(ListItem::new(Line::from(vec![
            Span::styled(&link.timestamp, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(&link.slug, style),
        ])));

        if !summary_preview.is_empty() {
            items.push(ListItem::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(summary_preview, Style::default().fg(Color::DarkGray)),
            ])));
        }
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "  No links in this chain",
            Style::default().fg(Color::DarkGray),
        )])));
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::DarkGray));

    frame.render_widget(list, area);
}

fn draw_link_preview<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let Some(chain_idx) = app.selected_chain_idx else {
        return;
    };

    let chain = match app.get_chain_at_index(chain_idx) {
        Some(c) => c,
        None => return,
    };

    let link_info = app.selected_link_idx.and_then(|idx| chain.links.get(idx));
    let title = link_info
        .map(|l| format!(" {} - {} ", l.timestamp, l.slug))
        .unwrap_or_else(|| " Chain Link ".to_string());

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let lines: Vec<Line> = app
        .chain_link_content
        .lines()
        .skip(app.chain_link_scroll)
        .take(area.height.saturating_sub(2) as usize)
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
                Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::Green),
                ))
            } else if line.starts_with("- ") || line.starts_with("* ") {
                Line::from(Span::styled(line, Style::default().fg(Color::White)))
            } else if line.starts_with("```") {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::DarkGray),
                ))
            } else {
                Line::from(Span::raw(line))
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}
