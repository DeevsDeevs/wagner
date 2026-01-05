use crate::agent::Agent;
use crate::terminal::Terminal;
use crate::tui::app::{App, InputMode};

use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    match app.input_mode {
        InputMode::DiffFileList => draw_file_list(frame, area, app),
        InputMode::DiffContent => draw_diff_content(frame, area, app),
        _ => {}
    }
}

fn draw_file_list<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    frame.render_widget(Clear, area);

    let repo_name = app.diff_repo_name.as_deref().unwrap_or("unknown");
    let base = &app.wagner.config.diff_base;
    let title = format!(" Diff: {} ({}..HEAD) ", repo_name, base);

    let block = Block::default()
        .title(title)
        .title_bottom(" [j/k] navigate  [Enter] view  [q] close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    if app.diff_files.is_empty() {
        let msg = Paragraph::new("No changes")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .diff_files
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let is_selected = i == app.diff_file_index;

            let status_color = match file.status {
                'A' => Color::Green,
                'D' => Color::Red,
                'M' => Color::Yellow,
                'R' => Color::Blue,
                _ => Color::White,
            };

            let style = if is_selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let stats = format!("+{:<4} -{:<4}", file.additions, file.deletions);

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", file.status),
                    Style::default().fg(status_color),
                ),
                Span::styled(&file.path, style),
                Span::raw("  "),
                Span::styled(stats, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::DarkGray));

    frame.render_widget(list, area);
}

fn draw_diff_content<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    frame.render_widget(Clear, area);

    let file_name = app
        .diff_files
        .get(app.diff_file_index)
        .map(|f| f.path.as_str())
        .unwrap_or("unknown");

    let title = format!(" {} ", file_name);

    let block = Block::default()
        .title(title)
        .title_bottom(" [j/k] scroll  [g/G] top/bottom  [q] back ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let visible_lines: Vec<&str> = app
        .diff_content
        .iter()
        .skip(app.diff_scroll)
        .take(inner.height as usize)
        .map(|s| s.as_str())
        .collect();

    let content = visible_lines.join("\n");

    let text = content.into_text().unwrap_or_default();

    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });

    frame.render_widget(paragraph, inner);
}
