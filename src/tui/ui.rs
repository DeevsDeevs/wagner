use crate::agent::Agent;
use crate::terminal::Terminal;

use super::app::{App, Focus, InputMode};
use super::widgets::{diff_view, help_popup, pane_list, settings_popup, task_tree, terminal_view};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::time::Duration;

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, app: &App<T, A>) {
    let area = frame.area();

    let is_input_mode = matches!(
        app.input_mode,
        InputMode::NewTask | InputMode::SendMessage | InputMode::Confirm | InputMode::EditSetting
    );
    let is_workspace_select = app.input_mode == InputMode::SelectWorkspace;

    let (main_area, bottom_area) = if is_input_mode || is_workspace_select {
        let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        (chunks[0], Some(chunks[1]))
    } else if app.status_message.is_some() {
        let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        draw_status_bar(frame, chunks[1], app);
        (chunks[0], None)
    } else {
        (area, None)
    };

    let (sidebar_area, terminal_area) = if app.show_sidebar {
        let chunks = Layout::horizontal([
            Constraint::Length(app.wagner.config.sidebar_width),
            Constraint::Min(0),
        ])
        .split(main_area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, main_area)
    };

    draw_terminal_view(frame, terminal_area, app);

    if let Some(sidebar) = sidebar_area {
        draw_sidebar(frame, sidebar, app);
    }

    if let Some(input_area) = bottom_area {
        if is_workspace_select {
            draw_workspace_bar(frame, input_area, app);
        } else {
            draw_input_bar(frame, input_area, app);
        }
    }

    if app.show_help {
        draw_help_popup(frame, area, &app.wagner.config.keybindings);
    }

    if app.input_mode == InputMode::Settings || app.input_mode == InputMode::EditSetting {
        draw_settings_popup(frame, area, app);
    }
}

fn draw_settings_popup<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let popup_area = centered_rect(60, 80, area);
    frame.render_widget(Clear, popup_area);
    settings_popup::draw(frame, popup_area, app);
}

fn draw_sidebar<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Percentage(60),
        Constraint::Min(0),
    ])
    .split(area);

    let title_style = if app.focus == Focus::Sidebar {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = Paragraph::new("Wagner")
        .style(title_style)
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(title, chunks[0]);

    task_tree::draw(frame, chunks[1], app);
    pane_list::draw(frame, chunks[2], app);
}

fn draw_terminal_view<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let is_diff_mode =
        app.input_mode == InputMode::DiffFileList || app.input_mode == InputMode::DiffContent;

    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

    let (title, header_style) = if is_diff_mode {
        let repo_name = app.diff_repo_name.as_deref().unwrap_or("unknown");
        let mode_hint = if app.input_mode == InputMode::DiffFileList {
            "files"
        } else {
            "content"
        };
        (
            format!(" Diff: {} ({}) ", repo_name, mode_hint),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )
    } else if let Some(task) = &app.selected_task {
        let style = if app.focus == Focus::Terminal {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        (format!(" {} ", task), style)
    } else {
        (
            " No task selected ".to_string(),
            Style::default().fg(Color::DarkGray),
        )
    };

    let hints = if is_diff_mode {
        " [q] Close [?] Help"
    } else {
        " [?] Help"
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(title, header_style),
        Span::styled(hints, Style::default().fg(Color::DarkGray)),
    ]))
    .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, chunks[0]);

    if is_diff_mode {
        diff_view::draw(frame, chunks[1], app);
    } else {
        terminal_view::draw(frame, chunks[1], app);
    }
}

fn draw_help_popup(frame: &mut Frame, area: Rect, keybindings: &crate::config::Keybindings) {
    let popup_area = centered_rect(60, 70, area);
    frame.render_widget(Clear, popup_area);
    help_popup::draw(frame, popup_area, keybindings);
}

fn draw_input_bar<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let cursor_pos = app.input_cursor;
    let input_text = &app.input_buffer;

    let chars: Vec<char> = input_text.chars().collect();
    let before_cursor: String = chars[..cursor_pos.min(chars.len())].iter().collect();
    let cursor_char = chars.get(cursor_pos).copied().unwrap_or(' ');
    let after_cursor: String = if cursor_pos < chars.len() {
        chars[cursor_pos + 1..].iter().collect()
    } else {
        String::new()
    };

    let prompt = format!("{}: ", app.input_label);
    let input_line = Line::from(vec![
        Span::styled(&prompt, Style::default().fg(Color::Yellow)),
        Span::raw(before_cursor),
        Span::styled(
            cursor_char.to_string(),
            Style::default().bg(Color::White).fg(Color::Black),
        ),
        Span::raw(after_cursor),
    ]);

    let paragraph = Paragraph::new(input_line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

fn draw_workspace_bar<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let task_name = app.pending_task_name.as_deref().unwrap_or("?");
    let mut spans = vec![
        Span::styled(
            format!("Workspace for '{}': ", task_name),
            Style::default().fg(Color::Yellow),
        ),
    ];

    for (i, ws) in app.workspace_list.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" | "));
        }
        if i == app.workspace_index {
            spans.push(Span::styled(
                ws.as_str(),
                Style::default().bg(Color::White).fg(Color::Black),
            ));
        } else {
            spans.push(Span::raw(ws.as_str()));
        }
    }

    let paragraph =
        Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

fn draw_status_bar<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    if let Some((msg, time)) = &app.status_message {
        if time.elapsed() < Duration::from_secs(5) {
            let style = if msg.starts_with("Error") {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };
            let paragraph = Paragraph::new(Span::styled(format!(" {} ", msg), style));
            frame.render_widget(paragraph, area);
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
