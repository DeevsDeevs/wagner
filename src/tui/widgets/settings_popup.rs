use crate::agent::Agent;
use crate::terminal::Terminal;
use crate::tui::app::App;

use super::components::{Footer, ScrollableList};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
};

const GENERAL_SETTINGS_COUNT: usize = 7;

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
        height: inner.height.saturating_sub(1),
    };

    let footer_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };

    let mut list = ScrollableList::new().section("General");

    for (i, (key, value)) in app.settings_items.iter().enumerate() {
        if i == GENERAL_SETTINGS_COUNT {
            list = list.section("Keybindings");
        }

        let is_selected = i == app.settings_index;
        let is_keybinding = key.starts_with("key.");
        let is_bool = key == "show_hints";

        let display_key = if is_keybinding {
            key.strip_prefix("key.").unwrap_or(key)
        } else {
            key.as_str()
        };

        if is_bool {
            list = list.bool_item(display_key, value == "true", is_selected);
        } else {
            list = list.item(display_key, value, is_selected, is_keybinding);
        }
    }

    let visible_height = list_area.height as usize;
    list = list.scroll_to_selected(visible_height);
    list.draw(frame, list_area);

    Footer::new()
        .add("j/k", "Navigate")
        .add("Enter", "Edit")
        .add("Esc", "Close")
        .draw(frame, footer_area);
}
