use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub struct TextInput<'a> {
    pub label: &'a str,
    pub buffer: &'a str,
    pub cursor: usize,
}

impl<'a> TextInput<'a> {
    pub fn new(label: &'a str, buffer: &'a str, cursor: usize) -> Self {
        Self {
            label,
            buffer,
            cursor,
        }
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let chars: Vec<char> = self.buffer.chars().collect();
        let before_cursor: String = chars[..self.cursor.min(chars.len())].iter().collect();
        let cursor_char = chars.get(self.cursor).copied().unwrap_or(' ');
        let after_cursor: String = if self.cursor < chars.len() {
            chars[self.cursor + 1..].iter().collect()
        } else {
            String::new()
        };

        let prompt = format!("{}: ", self.label);
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
}

pub struct Selector<'a> {
    pub label: &'a str,
    pub items: &'a [String],
    pub selected: usize,
}

impl<'a> Selector<'a> {
    pub fn new(label: &'a str, items: &'a [String], selected: usize) -> Self {
        Self {
            label,
            items,
            selected,
        }
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::styled(
            format!("{}: ", self.label),
            Style::default().fg(Color::Yellow),
        )];

        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" | "));
            }
            if i == self.selected {
                spans.push(Span::styled(
                    item.as_str(),
                    Style::default().bg(Color::White).fg(Color::Black),
                ));
            } else {
                spans.push(Span::raw(item.as_str()));
            }
        }

        let paragraph =
            Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::DarkGray));
        frame.render_widget(paragraph, area);
    }
}

pub enum ListItem<'a> {
    Section(&'a str),
    Item {
        key: &'a str,
        value: &'a str,
        selected: bool,
        dimmed: bool,
    },
    BoolItem {
        key: &'a str,
        value: bool,
        selected: bool,
    },
}

pub struct ScrollableList<'a> {
    items: Vec<ListItem<'a>>,
    scroll_offset: usize,
}

impl<'a> Default for ScrollableList<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> ScrollableList<'a> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            scroll_offset: 0,
        }
    }

    pub fn section(mut self, title: &'a str) -> Self {
        self.items.push(ListItem::Section(title));
        self
    }

    pub fn item(mut self, key: &'a str, value: &'a str, selected: bool, dimmed: bool) -> Self {
        self.items.push(ListItem::Item {
            key,
            value,
            selected,
            dimmed,
        });
        self
    }

    pub fn bool_item(mut self, key: &'a str, value: bool, selected: bool) -> Self {
        self.items.push(ListItem::BoolItem {
            key,
            value,
            selected,
        });
        self
    }

    pub fn scroll_to_selected(mut self, visible_height: usize) -> Self {
        let mut visual_index = 0;

        for (i, item) in self.items.iter().enumerate() {
            match item {
                ListItem::Item { selected: true, .. }
                | ListItem::BoolItem { selected: true, .. } => {
                    visual_index = i;
                    break;
                }
                _ => {}
            }
        }

        let total = self.items.len();
        self.scroll_offset = if visual_index < visible_height / 2 {
            0
        } else if visual_index > total.saturating_sub(visible_height / 2) {
            total.saturating_sub(visible_height)
        } else {
            visual_index.saturating_sub(visible_height / 2)
        };

        self
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let visible_height = area.height as usize;
        let total = self.items.len();

        let visible_items: Vec<Line> = self
            .items
            .iter()
            .skip(self.scroll_offset)
            .take(visible_height)
            .map(|item| self.render_item(item))
            .collect();

        let has_more_above = self.scroll_offset > 0;
        let has_more_below = self.scroll_offset + visible_height < total;

        let paragraph = Paragraph::new(visible_items);
        frame.render_widget(paragraph, area);

        if has_more_above {
            let indicator = Paragraph::new("↑").style(Style::default().fg(Color::DarkGray));
            let indicator_area = Rect {
                x: area.x + area.width.saturating_sub(2),
                y: area.y,
                width: 1,
                height: 1,
            };
            frame.render_widget(indicator, indicator_area);
        }

        if has_more_below {
            let indicator = Paragraph::new("↓").style(Style::default().fg(Color::DarkGray));
            let indicator_area = Rect {
                x: area.x + area.width.saturating_sub(2),
                y: area.y + area.height.saturating_sub(1),
                width: 1,
                height: 1,
            };
            frame.render_widget(indicator, indicator_area);
        }
    }

    fn render_item<'b>(&self, item: &ListItem<'b>) -> Line<'b> {
        match item {
            ListItem::Section(title) => Line::from(vec![
                Span::styled("─── ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    *title,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ───", Style::default().fg(Color::DarkGray)),
            ]),
            ListItem::Item {
                key,
                value,
                selected,
                dimmed,
            } => {
                let prefix = if *selected { "▸ " } else { "  " };
                let style = if *selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else if *dimmed {
                    Style::default().fg(Color::Gray)
                } else {
                    Style::default().fg(Color::White)
                };

                Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(format!("{}: ", key), style),
                    Span::styled(*value, Style::default().fg(Color::Green)),
                ])
            }
            ListItem::BoolItem {
                key,
                value,
                selected,
            } => {
                let prefix = if *selected { "▸ " } else { "  " };
                let style = if *selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let display = if *value { "[x]" } else { "[ ]" };

                Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(format!("{}: ", key), style),
                    Span::styled(display, Style::default().fg(Color::Green)),
                ])
            }
        }
    }
}

pub struct Footer<'a> {
    items: Vec<(&'a str, &'a str)>,
}

impl<'a> Default for Footer<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Footer<'a> {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(mut self, key: &'a str, action: &'a str) -> Self {
        self.items.push((key, action));
        self
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let mut spans = Vec::new();
        for (i, (key, action)) in self.items.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(
                format!(" {} ", key),
                Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::raw(*action));
        }

        let paragraph = Paragraph::new(Line::from(spans));
        frame.render_widget(paragraph, area);
    }
}
