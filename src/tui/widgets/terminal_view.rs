use crate::agent::Agent;
use crate::terminal::Terminal;
use crate::tui::app::App;

use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let header_text = build_header(app);
    let show_header = !header_text.is_empty();

    let (header_area, content_area) = if show_header {
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    if let Some(header) = header_area {
        let header_line = Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                header_text,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        frame.render_widget(Paragraph::new(header_line), header);
    }

    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    let selection = app.get_visual_selection();
    let text = build_text_with_selection(&app.terminal_output, selection);

    let line_count = app.terminal_output.lines().count();
    let visible_height = inner.height as usize;

    let paragraph = Paragraph::new(text).scroll((app.terminal_scroll, 0));

    frame.render_widget(paragraph, inner);

    if line_count > visible_height {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");

        let mut scrollbar_state = ScrollbarState::new(line_count.saturating_sub(visible_height))
            .position(app.terminal_scroll as usize);

        let scrollbar_area = Rect {
            x: inner.x + inner.width.saturating_sub(1),
            y: inner.y,
            width: 1,
            height: inner.height,
        };

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn build_header<T: Terminal, A: Agent>(app: &App<T, A>) -> String {
    let task = app.selected_task.as_ref();
    let repo = app.selected_repo.as_ref();
    match (task, repo) {
        (Some(t), Some(r)) => format!("{} > {}", t, r),
        (Some(t), None) => t.clone(),
        _ => String::new(),
    }
}

fn build_text_with_selection(output: &str, selection: Option<(usize, usize)>) -> Text<'static> {
    let select_style = Style::default().bg(Color::DarkGray).fg(Color::White);

    match selection {
        None => {
            match output.as_bytes().into_text() {
                Ok(t) => t,
                Err(_) => Text::from(output.to_string()),
            }
        }
        Some((start, end)) => {
            let lines: Vec<Line<'static>> = output
                .lines()
                .enumerate()
                .map(|(i, line_str)| {
                    let is_selected = i >= start && i <= end;
                    if is_selected {
                        Line::styled(line_str.to_string(), select_style)
                    } else {
                        match line_str.as_bytes().into_text() {
                            Ok(t) => t.lines.into_iter().next().unwrap_or_else(|| Line::from("")),
                            Err(_) => Line::from(line_str.to_string()),
                        }
                    }
                })
                .collect();
            Text::from(lines)
        }
    }
}
