use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::config::Keybindings;

pub fn draw(frame: &mut Frame, area: Rect, keybindings: &Keybindings) {
    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let key_style = Style::default().fg(Color::Yellow);
    let fmt_key = |k: &str| -> String { format!("{:<15}", k) };

    let nav_updown = format!("{}/{} ↑/↓", keybindings.nav_down, keybindings.nav_up);
    let nav_page = format!("{}/{} PgUp/Dn", keybindings.page_up, keybindings.page_down);
    let nav_scroll = format!(
        "{}/{} Home/End",
        keybindings.scroll_top, keybindings.scroll_bottom
    );
    let nav_focus = format!("{}/{} ←/→", keybindings.nav_left, keybindings.nav_right);

    let help_text = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Navigation",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&nav_updown)), key_style),
            Span::raw("Navigate / scroll line"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&nav_page)), key_style),
            Span::raw("Scroll page up/down"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&nav_scroll)), key_style),
            Span::raw("Scroll to top/bottom"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&nav_focus)), key_style),
            Span::raw("Switch focus sidebar/terminal"),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {}", fmt_key(&keybindings.switch_section)),
                key_style,
            ),
            Span::raw("Switch Tasks/Panes section"),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {}", fmt_key(&keybindings.toggle_sidebar)),
                key_style,
            ),
            Span::raw("Toggle sidebar"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key("Enter")), key_style),
            Span::raw("Select / focus terminal"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key("1-9")), key_style),
            Span::raw("Quick switch to pane"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Actions",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&keybindings.attach)), key_style),
            Span::raw("Attach to tmux session"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&keybindings.refresh)), key_style),
            Span::raw("Refresh output"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&keybindings.new_task)), key_style),
            Span::raw("New task"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&keybindings.add_pane)), key_style),
            Span::raw("Add pane to task"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&keybindings.delete)), key_style),
            Span::raw("Delete task"),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {}", fmt_key(&keybindings.send_message)),
                key_style,
            ),
            Span::raw("Send message to pane"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&keybindings.open_diff)), key_style),
            Span::raw("View git diff"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  General",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&keybindings.settings)), key_style),
            Span::raw("Settings"),
        ]),
        Line::from(vec![
            Span::styled(format!("  {}", fmt_key(&keybindings.help)), key_style),
            Span::raw("Toggle this help"),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {}", fmt_key(&format!("{} / Esc", keybindings.quit))),
                key_style,
            ),
            Span::raw("Quit"),
        ]),
        Line::from(""),
    ];

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .style(Style::default().fg(Color::White));

    frame.render_widget(paragraph, area);
}
