use crate::agent::Agent;
use crate::terminal::Terminal;
use crate::tui::app::{App, InputMode};

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let block = Block::default()
        .title(" Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let list_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };

    let footer_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(2),
        width: inner.width,
        height: 2,
    };

    let items: Vec<ListItem> = app
        .settings_items
        .iter()
        .enumerate()
        .map(|(i, (key, value))| {
            let is_selected = i == app.settings_index;
            let is_keybinding = key.starts_with("key.");
            let is_bool = key == "show_hints";

            let prefix = if is_selected { "▸ " } else { "  " };
            let display_key = if is_keybinding {
                key.strip_prefix("key.").unwrap_or(key)
            } else {
                key
            };

            let style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if is_keybinding {
                Style::default().fg(Color::Gray)
            } else {
                Style::default().fg(Color::White)
            };

            let display_value = if is_bool {
                if value == "true" { "[x]" } else { "[ ]" }
            } else {
                value.as_str()
            };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("{}: ", display_key), style),
                Span::styled(display_value, Style::default().fg(Color::Green)),
            ]))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, list_area);

    if app.input_mode == InputMode::EditSetting {
        let edit_area = Rect {
            x: inner.x + 2,
            y: inner.y + inner.height.saturating_sub(4),
            width: inner.width.saturating_sub(4),
            height: 1,
        };

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

        let input_line = Line::from(vec![
            Span::raw(before_cursor),
            Span::styled(
                cursor_char.to_string(),
                Style::default().bg(Color::White).fg(Color::Black),
            ),
            Span::raw(after_cursor),
        ]);

        let paragraph = Paragraph::new(input_line);
        frame.render_widget(paragraph, edit_area);
    }

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" j/k ", Style::default().fg(Color::Cyan)),
        Span::raw("Navigate  "),
        Span::styled("Enter ", Style::default().fg(Color::Cyan)),
        Span::raw("Edit  "),
        Span::styled("Ctrl+s ", Style::default().fg(Color::Cyan)),
        Span::raw("Save  "),
        Span::styled("Esc ", Style::default().fg(Color::Cyan)),
        Span::raw("Close"),
    ]));
    frame.render_widget(footer, footer_area);
}
