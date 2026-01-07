use crate::agent::Agent;
use crate::config::Keybindings;
use crate::error::Result;
use crate::terminal::Terminal;

use super::app::{App, Focus, InputMode, SidebarSection};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use std::time::Duration;

fn matches_key(code: KeyCode, binding: &str) -> bool {
    match code {
        KeyCode::Tab => binding == "Tab",
        KeyCode::Esc => binding == "Esc",
        KeyCode::Enter => binding == "Enter",
        KeyCode::Char(c) => {
            if binding.len() == 1 {
                binding.chars().next() == Some(c)
            } else {
                false
            }
        }
        _ => false,
    }
}

pub fn handle_events<T: Terminal, A: Agent>(app: &mut App<T, A>, area: Rect) -> Result<bool> {
    if !event::poll(Duration::from_millis(100))? {
        return Ok(false);
    }

    match event::read()? {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return Ok(false);
            }

            match app.input_mode {
                InputMode::Normal => handle_normal_mode(app, key.code, key.modifiers),
                InputMode::NewTask | InputMode::SendMessage | InputMode::Confirm => {
                    handle_input_mode(app, key.code, key.modifiers)
                }
                InputMode::SelectWorkspace => handle_workspace_select_mode(app, key.code),
                InputMode::Settings => handle_settings_mode(app, key.code),
                InputMode::EditSetting => handle_edit_setting_mode(app, key.code, key.modifiers),
                InputMode::DiffFileList => handle_diff_file_list_mode(app, key.code),
                InputMode::DiffContent => handle_diff_content_mode(app, key.code),
            }
        }
        Event::Mouse(mouse) => {
            handle_mouse_event(app, mouse, area);
        }
        Event::Resize(width, height) => {
            let new_area = Rect::new(0, 0, width, height);
            app.handle_resize(new_area);
        }
        _ => {}
    }

    Ok(true)
}

fn handle_mouse_event<T: Terminal, A: Agent>(
    app: &mut App<T, A>,
    mouse: crossterm::event::MouseEvent,
    area: Rect,
) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            match app.input_mode {
                InputMode::Normal => {
                    if app.is_on_sidebar_border(mouse.column) {
                        app.dragging_sidebar = true;
                    } else {
                        app.handle_click(mouse.column, mouse.row, area);
                    }
                }
                InputMode::DiffFileList | InputMode::DiffContent => {
                    app.close_diff_view();
                }
                InputMode::Settings | InputMode::EditSetting => {
                    app.close_settings();
                }
                InputMode::NewTask | InputMode::SendMessage | InputMode::Confirm => {
                    app.cancel_input();
                }
                InputMode::SelectWorkspace => {
                    app.pending_task_name = None;
                    app.workspace_list.clear();
                    app.workspace_index = 0;
                    app.input_mode = InputMode::Normal;
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.dragging_sidebar = false;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.dragging_sidebar {
                app.handle_sidebar_drag(mouse.column, area);
            }
        }
        MouseEventKind::ScrollUp => {
            if app.input_mode == InputMode::Normal && app.focus == Focus::Terminal {
                app.scroll_terminal_up();
            }
        }
        MouseEventKind::ScrollDown => {
            if app.input_mode == InputMode::Normal && app.focus == Focus::Terminal {
                app.scroll_terminal_down();
            }
        }
        _ => {}
    }
}

fn get_action(code: KeyCode, kb: &Keybindings) -> Option<&'static str> {
    if code == KeyCode::Esc {
        return Some("quit");
    }

    let bindings: &[(&str, &str)] = &[
        (&kb.quit, "quit"),
        (&kb.help, "help"),
        (&kb.toggle_sidebar, "toggle_sidebar"),
        (&kb.refresh, "refresh"),
        (&kb.attach, "attach"),
        (&kb.new_task, "new_task"),
        (&kb.add_pane, "add_pane"),
        (&kb.delete, "delete"),
        (&kb.send_message, "send_message"),
        (&kb.settings, "settings"),
        (&kb.switch_section, "switch_section"),
        (&kb.open_diff, "open_diff"),
    ];

    bindings
        .iter()
        .find(|(binding, _)| matches_key(code, binding))
        .map(|(_, action)| *action)
}

fn handle_normal_mode<T: Terminal, A: Agent>(
    app: &mut App<T, A>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    let kb = &app.wagner.config.keybindings;

    if app.show_help {
        if code == KeyCode::Esc || matches_key(code, &kb.help) || matches_key(code, &kb.quit) {
            app.toggle_help();
        }
        return;
    }

    if app.focus == Focus::Terminal {
        if matches_key(code, &kb.toggle_sidebar) {
            app.focus = Focus::Sidebar;
            if !app.show_sidebar {
                app.show_sidebar = true;
            }
            return;
        }
        if code == KeyCode::Esc {
            app.focus = Focus::Sidebar;
            return;
        }
        send_key_to_pane(app, code, modifiers);
        return;
    }

    if let KeyCode::Char(c) = code {
        if let Some(n) = c.to_digit(10) {
            if n >= 1 && n <= 9 {
                app.select_pane((n - 1) as usize);
                return;
            }
        }
    }

    match get_action(code, kb) {
        Some("quit") => app.should_quit = true,
        Some("help") => app.toggle_help(),
        Some("toggle_sidebar") => app.toggle_sidebar(),
        Some("refresh") => {
            let _ = app.refresh_data();
        }
        Some("attach") => app.attach_current(),
        Some("new_task") => app.start_new_task(),
        Some("add_pane") => app.add_pane(),
        Some("delete") => app.start_delete(),
        Some("send_message") => app.start_send_message(),
        Some("settings") => app.open_settings(),
        Some("switch_section") if app.focus == Focus::Sidebar => app.toggle_sidebar_section(),
        Some("open_diff") => app.open_diff_view(),
        _ => handle_navigation(app, code),
    }
}

fn send_key_to_pane<T: Terminal, A: Agent>(
    app: &mut App<T, A>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    let key_str = match code {
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Backspace => "BSpace".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Delete => "DC".to_string(),
        KeyCode::Insert => "IC".to_string(),
        KeyCode::F(n) => format!("F{}", n),
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::CONTROL) {
                format!("C-{}", c)
            } else {
                return send_literal_to_pane(app, c);
            }
        }
        _ => return,
    };

    if let Some(pane_id) = &app.selected_pane.clone() {
        let pane = crate::terminal::PaneHandle(pane_id.clone(), String::new());
        let _ = app.wagner.terminal.send_key(&pane, &key_str);
        let _ = app.refresh_terminal_output();
    }
}

fn send_literal_to_pane<T: Terminal, A: Agent>(app: &mut App<T, A>, c: char) {
    if let Some(pane_id) = &app.selected_pane.clone() {
        let pane = crate::terminal::PaneHandle(pane_id.clone(), String::new());
        let _ = app.wagner.terminal.send_literal(&pane, &c.to_string());
        let _ = app.refresh_terminal_output();
    }
}

fn handle_navigation<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    let kb = &app.wagner.config.keybindings;

    if matches_key(code, &kb.scroll_top) || code == KeyCode::Home {
        if app.focus == Focus::Terminal {
            app.scroll_terminal_top();
        }
        return;
    }

    if matches_key(code, &kb.scroll_bottom) || code == KeyCode::End {
        if app.focus == Focus::Terminal {
            app.scroll_terminal_bottom();
        }
        return;
    }

    if matches_key(code, &kb.nav_down) || code == KeyCode::Down {
        match app.focus {
            Focus::Sidebar => match app.sidebar_section {
                SidebarSection::Tasks => app.next_task(),
                SidebarSection::Panes => app.next_pane(),
            },
            Focus::Terminal => app.scroll_terminal_down(),
        }
        return;
    }

    if matches_key(code, &kb.nav_up) || code == KeyCode::Up {
        match app.focus {
            Focus::Sidebar => match app.sidebar_section {
                SidebarSection::Tasks => app.prev_task(),
                SidebarSection::Panes => app.prev_pane(),
            },
            Focus::Terminal => app.scroll_terminal_up(),
        }
        return;
    }

    if matches_key(code, &kb.page_up) || code == KeyCode::PageUp {
        if app.focus == Focus::Terminal {
            app.scroll_terminal_page_up(app.wagner.config.page_scroll_lines);
        }
        return;
    }

    if matches_key(code, &kb.page_down) || code == KeyCode::PageDown {
        if app.focus == Focus::Terminal {
            app.scroll_terminal_page_down(app.wagner.config.page_scroll_lines);
        }
        return;
    }

    if matches_key(code, &kb.nav_left) || code == KeyCode::Left {
        if !app.show_sidebar {
            app.show_sidebar = true;
        }
        app.focus = Focus::Sidebar;
        return;
    }

    if matches_key(code, &kb.nav_right) || code == KeyCode::Right {
        app.focus = Focus::Terminal;
        return;
    }

    if code == KeyCode::Enter && app.focus == Focus::Sidebar {
        match app.sidebar_section {
            SidebarSection::Tasks => app.toggle_task_expand(),
            SidebarSection::Panes => {
                app.focus = Focus::Terminal;
                let _ = app.refresh_terminal_output();
            }
        }
    }
}

fn handle_input_mode<T: Terminal, A: Agent>(
    app: &mut App<T, A>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    match code {
        KeyCode::Esc => {
            app.cancel_input();
        }
        KeyCode::Enter => {
            app.submit_input();
        }
        KeyCode::Backspace => {
            app.input_backspace();
        }
        KeyCode::Delete => {
            app.input_delete();
        }
        KeyCode::Left => {
            app.input_left();
        }
        KeyCode::Right => {
            app.input_right();
        }
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                app.cancel_input();
            } else {
                app.input_char(c);
            }
        }
        _ => {}
    }
}

fn handle_settings_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.close_settings();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.settings_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.settings_prev();
        }
        KeyCode::Enter => {
            app.start_edit_setting();
        }
        _ => {}
    }
}

fn handle_edit_setting_mode<T: Terminal, A: Agent>(
    app: &mut App<T, A>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    match code {
        KeyCode::Esc => {
            app.input_buffer.clear();
            app.editing_setting_key = None;
            app.input_mode = InputMode::Settings;
        }
        KeyCode::Enter => {
            app.apply_setting();
        }
        KeyCode::Backspace => {
            app.input_backspace();
        }
        KeyCode::Delete => {
            app.input_delete();
        }
        KeyCode::Left => {
            app.input_left();
        }
        KeyCode::Right => {
            app.input_right();
        }
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                app.input_buffer.clear();
                app.editing_setting_key = None;
                app.input_mode = InputMode::Settings;
            } else {
                app.input_char(c);
            }
        }
        _ => {}
    }
}

fn handle_diff_file_list_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => app.close_diff_view(),
        KeyCode::Enter => app.select_diff_file(),
        KeyCode::Char('j') | KeyCode::Down => app.diff_next_file(),
        KeyCode::Char('k') | KeyCode::Up => app.diff_prev_file(),
        KeyCode::Char('g') | KeyCode::Home => {
            app.diff_file_index = 0;
        }
        KeyCode::Char('G') | KeyCode::End => {
            if !app.diff_files.is_empty() {
                app.diff_file_index = app.diff_files.len() - 1;
            }
        }
        _ => {}
    }
}

fn handle_diff_content_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => app.diff_back_to_list(),
        KeyCode::Char('j') | KeyCode::Down => app.diff_scroll_down(),
        KeyCode::Char('k') | KeyCode::Up => app.diff_scroll_up(),
        KeyCode::Char('g') | KeyCode::Home => app.diff_scroll_top(),
        KeyCode::Char('G') | KeyCode::End => app.diff_scroll_bottom(),
        KeyCode::PageDown | KeyCode::Char('f') => {
            for _ in 0..20 {
                app.diff_scroll_down();
            }
        }
        KeyCode::PageUp | KeyCode::Char('u') => {
            for _ in 0..20 {
                app.diff_scroll_up();
            }
        }
        _ => {}
    }
}

fn handle_workspace_select_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    let kb = &app.wagner.config.keybindings;

    if code == KeyCode::Esc {
        app.pending_task_name = None;
        app.workspace_list.clear();
        app.workspace_index = 0;
        app.input_mode = InputMode::Normal;
        return;
    }

    if code == KeyCode::Enter {
        app.submit_input();
        return;
    }

    if matches_key(code, &kb.nav_down) || code == KeyCode::Down {
        app.workspace_next();
        return;
    }

    if matches_key(code, &kb.nav_up) || code == KeyCode::Up {
        app.workspace_prev();
    }
}
