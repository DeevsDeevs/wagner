use crate::agent::Agent;
use crate::terminal::Terminal;
use crate::tui::app::{App, InputMode};

use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
};

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    match app.input_mode {
        InputMode::DiffFileList => draw_file_list(frame, area, app),
        InputMode::DiffContent => draw_diff_content(frame, area, app),
        _ => {}
    }
}

fn draw_file_list<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let base = app.get_diff_base();
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    let info = Paragraph::new(Line::from(vec![
        Span::styled(" Base: ", Style::default().fg(Color::DarkGray)),
        Span::styled(base, Style::default().fg(Color::Cyan)),
        Span::styled(
            "  [j/k] navigate  [Enter] view  [q] close",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    frame.render_widget(info, chunks[0]);

    if app.diff_files.is_empty() {
        let msg = Paragraph::new("  No changes").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, chunks[1]);
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

    frame.render_widget(List::new(items), chunks[1]);
}

fn draw_diff_content<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let file_name = app
        .diff_files
        .get(app.diff_file_index)
        .map(|f| f.path.as_str())
        .unwrap_or("unknown");

    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    let info = Paragraph::new(Line::from(vec![
        Span::styled(" File: ", Style::default().fg(Color::DarkGray)),
        Span::styled(file_name, Style::default().fg(Color::Yellow)),
        Span::styled(
            "  [j/k] scroll  [g/G] top/bottom  [q] back",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    frame.render_widget(info, chunks[0]);

    let content = app.diff_content.join("\n");
    let text = content
        .into_text()
        .unwrap_or_else(|_| ratatui::text::Text::raw(&content));

    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((app.diff_scroll as u16, 0));

    frame.render_widget(paragraph, chunks[1]);
}
