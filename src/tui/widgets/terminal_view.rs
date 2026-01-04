use crate::agent::Agent;
use crate::terminal::Terminal;
use crate::tui::app::App;

use ansi_to_tui::IntoText;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

pub fn draw<T: Terminal, A: Agent>(frame: &mut Frame, area: Rect, app: &App<T, A>) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = match app.terminal_output.as_bytes().into_text() {
        Ok(t) => t,
        Err(_) => app.terminal_output.clone().into(),
    };

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

pub fn get_line_count(output: &str) -> usize {
    output.lines().count()
}
