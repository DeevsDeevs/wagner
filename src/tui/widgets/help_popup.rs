use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn draw(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let help_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Navigation", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from("  j/k ↑/↓       Navigate / scroll line"),
        Line::from("  u/f PgUp/Dn   Scroll page up/down"),
        Line::from("  g/G Home/End  Scroll to top/bottom"),
        Line::from("  h/l ←/→      Switch focus sidebar/terminal"),
        Line::from("  o             Switch Tasks/Panes section"),
        Line::from("  Tab           Toggle sidebar"),
        Line::from("  Enter         Select / focus terminal"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Actions", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from("  a             Attach to tmux session"),
        Line::from("  r             Refresh output"),
        Line::from("  n             New task"),
        Line::from("  p             Add pane to task"),
        Line::from("  d             Delete task"),
        Line::from("  s             Send message to pane"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  General", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from("  S             Settings"),
        Line::from("  ?             Toggle this help"),
        Line::from("  q / Esc       Quit"),
        Line::from(""),
    ];

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .style(Style::default().fg(Color::White));

    frame.render_widget(paragraph, area);
}
