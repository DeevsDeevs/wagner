use crate::agent::Agent;
use crate::config::Keybindings;
use crate::error::Result;
use crate::plugins::chains::ChainsViewMode;
use crate::terminal::Terminal;

use super::app::{App, AppTab, Focus, InputMode, SidebarSection};

use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;
use std::time::Duration;

fn matches_key(code: KeyCode, binding: &str) -> bool {
    matches_key_with_modifiers(code, KeyModifiers::NONE, binding)
}

fn matches_key_with_modifiers(code: KeyCode, modifiers: KeyModifiers, binding: &str) -> bool {
    if let Some(key) = binding.strip_prefix("C-") {
        if !modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        if let KeyCode::Char(c) = code {
            return key.len() == 1 && key.starts_with(c);
        }
        return false;
    }

    match code {
        KeyCode::Tab => binding == "Tab",
        KeyCode::Esc => binding == "Esc",
        KeyCode::Enter => binding == "Enter",
        KeyCode::Char(c) => {
            if binding.len() == 1 {
                binding.starts_with(c)
            } else {
                false
            }
        }
        _ => false,
    }
}

enum TextInputAction {
    Cancel,
    Submit,
    Edit,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Quit,
    Help,
    NextTab,
    Refresh,
    Attach,
    NewTask,
    AddPane,
    Delete,
    SendMessage,
    Settings,
    SwitchSection,
    OpenDiff,
    CopyMode,
}

fn handle_text_editing<T: Terminal, A: Agent>(
    app: &mut App<T, A>,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> TextInputAction {
    match code {
        KeyCode::Esc => TextInputAction::Cancel,
        KeyCode::Enter => TextInputAction::Submit,
        KeyCode::Backspace => {
            app.input_backspace();
            TextInputAction::Edit
        }
        KeyCode::Delete => {
            app.input_delete();
            TextInputAction::Edit
        }
        KeyCode::Left => {
            app.input_left();
            TextInputAction::Edit
        }
        KeyCode::Right => {
            app.input_right();
            TextInputAction::Edit
        }
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                TextInputAction::Cancel
            } else {
                app.input_char(c);
                TextInputAction::Edit
            }
        }
        _ => TextInputAction::None,
    }
}

pub fn handle_events<T: Terminal, A: Agent>(app: &mut App<T, A>, area: Rect) -> Result<bool> {
    if !event::poll(Duration::from_millis(16))? {
        return Ok(false);
    }

    match event::read()? {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return Ok(false);
            }

            match app.input_mode {
                InputMode::Normal => handle_normal_mode(app, key.code, key.modifiers),
                InputMode::NewTask | InputMode::SendMessage | InputMode::Confirm
                | InputMode::AddPaneName => {
                    handle_input_mode(app, key.code, key.modifiers)
                }
                InputMode::AddPaneAgent => {
                    handle_add_pane_agent_mode(app, key.code)
                }
                InputMode::SelectWorkspace => handle_workspace_select_mode(app, key.code),
                InputMode::Settings => handle_settings_mode(app, key.code),
                InputMode::EditSetting => handle_edit_setting_mode(app, key.code, key.modifiers),
                InputMode::DiffFileList => handle_diff_file_list_mode(app, key.code),
                InputMode::DiffContent => handle_diff_content_mode(app, key.code),
                InputMode::ChainSearch => handle_chain_search_mode(app, key.code, key.modifiers),
                InputMode::VisualSelect => handle_visual_select_mode(app, key.code),
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
        MouseEventKind::Down(MouseButton::Left) => match app.input_mode {
            InputMode::Normal => {
                if app.is_on_sidebar_border(mouse.column) {
                    app.dragging_sidebar = true;
                } else {
                    let sidebar_width = app.wagner.config.sidebar_width;
                    let on_terminal = !app.show_sidebar || mouse.column >= sidebar_width;
                    if on_terminal {
                        app.pending_select_row = Some(mouse.row);
                    } else {
                        app.handle_click(mouse.column, mouse.row, area);
                    }
                }
            }
            InputMode::DiffFileList | InputMode::DiffContent => {
                app.close_diff_view();
            }
            InputMode::Settings | InputMode::EditSetting => {
                app.close_settings();
            }
            InputMode::NewTask | InputMode::SendMessage | InputMode::Confirm
            | InputMode::AddPaneAgent | InputMode::AddPaneName => {
                app.cancel_input();
            }
            InputMode::SelectWorkspace => {
                app.pending_task_name = None;
                app.workspace_list.clear();
                app.workspace_index = 0;
                app.input_mode = InputMode::Normal;
            }
            InputMode::ChainSearch => {
                app.cancel_chain_search();
            }
            InputMode::VisualSelect => {
                app.visual_select_drag(mouse.row);
            }
        },
        MouseEventKind::Up(MouseButton::Left) => {
            if let Some(row) = app.pending_select_row.take()
                && app.input_mode != InputMode::VisualSelect
            {
                app.handle_click(mouse.column, row, area);
            }
            app.dragging_sidebar = false;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.input_mode == InputMode::VisualSelect {
                app.visual_select_drag(mouse.row);
            } else if let Some(start_row) = app.pending_select_row.take() {
                if app.start_visual_select_at_row(start_row) {
                    app.visual_select_drag(mouse.row);
                }
            } else if app.dragging_sidebar {
                app.handle_sidebar_drag(mouse.column, area);
            }
        }
        MouseEventKind::ScrollUp => {
            if app.input_mode == InputMode::Normal {
                let sidebar_width = app.wagner.config.sidebar_width;
                let on_sidebar = app.show_sidebar && mouse.column < sidebar_width;
                if app.current_tab == AppTab::Chains && on_sidebar {
                    app.navigate_chain_list_prev();
                } else if app.current_tab == AppTab::Chains {
                    match app.chains_view_mode() {
                        ChainsViewMode::ChainList => app.scroll_terminal_up(),
                        ChainsViewMode::LinkList => app.navigate_link_list_prev(),
                        ChainsViewMode::LinkPreview => {
                            let main_start = if app.show_sidebar { sidebar_width } else { 0 };
                            let main_width = area.width.saturating_sub(main_start);
                            let split_point = main_start + (main_width * 30 / 100);
                            if mouse.column < split_point {
                                app.navigate_link_list_prev();
                            } else {
                                app.scroll_link_preview_up();
                            }
                        }
                    }
                } else if app.focus == Focus::Terminal {
                    app.scroll_terminal_up();
                }
            }
        }
        MouseEventKind::ScrollDown => {
            if app.input_mode == InputMode::Normal {
                let sidebar_width = app.wagner.config.sidebar_width;
                let on_sidebar = app.show_sidebar && mouse.column < sidebar_width;
                if app.current_tab == AppTab::Chains && on_sidebar {
                    app.navigate_chain_list_next();
                } else if app.current_tab == AppTab::Chains {
                    match app.chains_view_mode() {
                        ChainsViewMode::ChainList => app.scroll_terminal_down(),
                        ChainsViewMode::LinkList => app.navigate_link_list_next(),
                        ChainsViewMode::LinkPreview => {
                            let main_start = if app.show_sidebar { sidebar_width } else { 0 };
                            let main_width = area.width.saturating_sub(main_start);
                            let split_point = main_start + (main_width * 30 / 100);
                            if mouse.column < split_point {
                                app.navigate_link_list_next();
                            } else {
                                app.scroll_link_preview_down();
                            }
                        }
                    }
                } else if app.focus == Focus::Terminal {
                    app.scroll_terminal_down();
                }
            }
        }
        _ => {}
    }
}

fn get_action(code: KeyCode, kb: &Keybindings) -> Option<Action> {
    if code == KeyCode::Esc {
        return Some(Action::Quit);
    }

    let bindings: &[(&str, Action)] = &[
        (&kb.quit, Action::Quit),
        (&kb.help, Action::Help),
        (&kb.next_tab, Action::NextTab),
        (&kb.refresh, Action::Refresh),
        (&kb.attach, Action::Attach),
        (&kb.new_task, Action::NewTask),
        (&kb.add_pane, Action::AddPane),
        (&kb.delete, Action::Delete),
        (&kb.send_message, Action::SendMessage),
        (&kb.settings, Action::Settings),
        (&kb.switch_section, Action::SwitchSection),
        (&kb.open_diff, Action::OpenDiff),
        (&kb.copy_mode, Action::CopyMode),
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

    if app.current_tab == AppTab::Chains {
        let in_chains_main_view = matches!(
            app.chains_view_mode(),
            ChainsViewMode::LinkList | ChainsViewMode::LinkPreview
        );
        if in_chains_main_view
            || (app.chains_view_mode() == ChainsViewMode::ChainList && app.focus == Focus::Sidebar)
        {
            handle_chains_mode(app, code);
            return;
        }
    }

    // Ctrl+E sends Escape to pane, Ctrl+T sends Tab to pane (when in terminal focus)
    if app.focus == Focus::Terminal
        && modifiers.contains(KeyModifiers::CONTROL)
        && let KeyCode::Char(c) = code
    {
        let key_to_send = match c {
            'e' => Some("Escape"),
            't' => Some("Tab"),
            _ => None,
        };
        if let Some(key) = key_to_send {
            if let Some(pane) = app.current_pane() {
                let _ = app.wagner.terminal.send_key(&pane, key);
                let _ = app.refresh_terminal_output();
            }
            return;
        }
    }

    if code == KeyCode::Esc {
        if app.focus == Focus::Terminal {
            app.focus = Focus::Sidebar;
        } else {
            app.should_quit = true;
        }
        return;
    }

    if app.focus == Focus::Terminal {
        send_key_to_pane(app, code, modifiers);
        return;
    }

    if let KeyCode::Char(c) = code
        && let Some(n) = c.to_digit(10)
        && (1..=9).contains(&n)
    {
        app.select_pane((n - 1) as usize);
        return;
    }

    match get_action(code, kb) {
        Some(Action::Quit) => app.should_quit = true,
        Some(Action::Help) => app.toggle_help(),
        Some(Action::NextTab) => app.next_tab(),
        Some(Action::Refresh) => {
            let _ = app.refresh_data();
        }
        Some(Action::Attach) => app.attach_current(),
        Some(Action::NewTask) => app.start_new_task(),
        Some(Action::AddPane) => app.add_pane(),
        Some(Action::Delete) => app.start_delete(),
        Some(Action::SendMessage) => app.start_send_message(),
        Some(Action::Settings) => app.open_settings(),
        Some(Action::SwitchSection) if app.focus == Focus::Sidebar => app.toggle_sidebar_section(),
        Some(Action::OpenDiff) => app.open_diff_view(),
        Some(Action::CopyMode) => app.start_visual_select(),
        _ => handle_navigation(app, code),
    }
}

const TMUX_KEY_MAP: &[(KeyCode, &str)] = &[
    (KeyCode::Enter, "Enter"),
    (KeyCode::Backspace, "BSpace"),
    (KeyCode::Left, "Left"),
    (KeyCode::Right, "Right"),
    (KeyCode::Up, "Up"),
    (KeyCode::Down, "Down"),
    (KeyCode::Home, "Home"),
    (KeyCode::End, "End"),
    (KeyCode::PageUp, "PageUp"),
    (KeyCode::PageDown, "PageDown"),
    (KeyCode::Tab, "Tab"),
    (KeyCode::BackTab, "BTab"),
    (KeyCode::Delete, "DC"),
    (KeyCode::Insert, "IC"),
    (KeyCode::Esc, "Escape"),
];

fn send_key_to_pane<T: Terminal, A: Agent>(
    app: &mut App<T, A>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    let key_str = if let Some((_, tmux_key)) = TMUX_KEY_MAP.iter().find(|(k, _)| *k == code) {
        (*tmux_key).to_string()
    } else {
        match code {
            KeyCode::F(n) => format!("F{}", n),
            KeyCode::Char(c) => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    format!("C-{}", c)
                } else if modifiers.contains(KeyModifiers::ALT) {
                    format!("M-{}", c)
                } else {
                    return send_literal_to_pane(app, c);
                }
            }
            _ => return,
        }
    };

    if let Some(pane) = app.current_pane() {
        let _ = app.wagner.terminal.send_key(&pane, &key_str);
        let _ = app.refresh_terminal_output();
    }
}

fn send_literal_to_pane<T: Terminal, A: Agent>(app: &mut App<T, A>, c: char) {
    if let Some(pane) = app.current_pane() {
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
    match handle_text_editing(app, code, modifiers) {
        TextInputAction::Cancel => app.cancel_input(),
        TextInputAction::Submit => {
            if app.input_mode == InputMode::AddPaneName {
                app.submit_add_pane_name();
            } else {
                app.submit_input();
            }
        }
        TextInputAction::Edit | TextInputAction::None => {}
    }
}

fn handle_add_pane_agent_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    let kb = &app.wagner.config.keybindings;

    if code == KeyCode::Esc {
        app.cancel_input();
    } else if code == KeyCode::Enter {
        app.submit_add_pane_agent();
    } else if matches_key(code, &kb.nav_down) || code == KeyCode::Down {
        if app.add_pane_index + 1 < app.add_pane_options.len() {
            app.add_pane_index += 1;
        }
    } else if matches_key(code, &kb.nav_up) || code == KeyCode::Up {
        app.add_pane_index = app.add_pane_index.saturating_sub(1);
    }
}

fn handle_settings_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    let kb = &app.wagner.config.keybindings;

    if code == KeyCode::Esc || matches_key(code, &kb.quit) {
        app.close_settings();
    } else if matches_key(code, &kb.nav_down) || code == KeyCode::Down {
        app.settings_next();
    } else if matches_key(code, &kb.nav_up) || code == KeyCode::Up {
        app.settings_prev();
    } else if code == KeyCode::Enter {
        app.start_edit_setting();
    }
}

fn handle_edit_setting_mode<T: Terminal, A: Agent>(
    app: &mut App<T, A>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    match handle_text_editing(app, code, modifiers) {
        TextInputAction::Cancel => app.cancel_edit_setting(),
        TextInputAction::Submit => app.apply_setting(),
        TextInputAction::Edit | TextInputAction::None => {}
    }
}

fn handle_diff_file_list_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    let kb = &app.wagner.config.keybindings;

    if code == KeyCode::Esc || matches_key(code, &kb.quit) {
        app.close_diff_view();
    } else if code == KeyCode::Enter {
        app.select_diff_file();
    } else if matches_key(code, &kb.nav_down) || code == KeyCode::Down {
        app.diff_next_file();
    } else if matches_key(code, &kb.nav_up) || code == KeyCode::Up {
        app.diff_prev_file();
    } else if matches_key(code, &kb.scroll_top) || code == KeyCode::Home {
        app.diff_file_index = 0;
    } else if (matches_key(code, &kb.scroll_bottom) || code == KeyCode::End)
        && !app.diff_files.is_empty()
    {
        app.diff_file_index = app.diff_files.len() - 1;
    }
}

fn handle_diff_content_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    let kb = &app.wagner.config.keybindings;
    let page_size = app.wagner.config.page_scroll_lines as usize;

    if code == KeyCode::Esc || matches_key(code, &kb.quit) {
        app.diff_back_to_list();
    } else if matches_key(code, &kb.nav_down) || code == KeyCode::Down {
        app.diff_scroll_down();
    } else if matches_key(code, &kb.nav_up) || code == KeyCode::Up {
        app.diff_scroll_up();
    } else if matches_key(code, &kb.scroll_top) || code == KeyCode::Home {
        app.diff_scroll_top();
    } else if matches_key(code, &kb.scroll_bottom) || code == KeyCode::End {
        app.diff_scroll_bottom();
    } else if matches_key(code, &kb.page_down) || code == KeyCode::PageDown {
        for _ in 0..page_size {
            app.diff_scroll_down();
        }
    } else if matches_key(code, &kb.page_up) || code == KeyCode::PageUp {
        for _ in 0..page_size {
            app.diff_scroll_up();
        }
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

fn handle_chains_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    let kb = &app.wagner.config.keybindings;

    if code == KeyCode::Tab || matches_key(code, &kb.next_tab) {
        app.next_tab();
        return;
    }

    if code == KeyCode::Esc || matches_key(code, &kb.quit) {
        app.chains_back();
        return;
    }

    if matches_key(code, &kb.help) {
        app.toggle_help();
        return;
    }

    if matches_key(code, &kb.settings) {
        app.open_settings();
        return;
    }

    if matches_key(code, &kb.nav_down) || code == KeyCode::Down {
        app.chains_next();
        return;
    }

    if matches_key(code, &kb.nav_up) || code == KeyCode::Up {
        app.chains_prev();
        return;
    }

    if code == KeyCode::Enter {
        app.chains_select();
        return;
    }

    if matches_key(code, &kb.refresh) {
        app.refresh_chains();
        return;
    }

    if code == KeyCode::Char('p') {
        app.promote_selected_chain();
        return;
    }

    if matches_key(code, &kb.delete) {
        app.start_delete_chain();
        return;
    }

    if matches_key(code, &kb.page_down) || code == KeyCode::PageDown {
        app.scroll_link_preview_down();
        return;
    }

    if matches_key(code, &kb.page_up) || code == KeyCode::PageUp {
        app.scroll_link_preview_up();
        return;
    }

    if matches_key(code, &kb.switch_section) {
        app.focus = match app.focus {
            Focus::Sidebar => Focus::Terminal,
            Focus::Terminal => Focus::Sidebar,
        };
        return;
    }

    if matches_key(code, &kb.nav_left) || code == KeyCode::Left {
        app.focus = Focus::Sidebar;
        return;
    }

    if matches_key(code, &kb.nav_right) || code == KeyCode::Right {
        app.focus = Focus::Terminal;
        return;
    }

    if code == KeyCode::Char('/') {
        app.start_chain_search();
        return;
    }

    if code == KeyCode::Esc && !app.plugin_states.chains.filter.is_empty() {
        app.clear_chain_filter();
    }
}

fn handle_chain_search_mode<T: Terminal, A: Agent>(
    app: &mut App<T, A>,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    match handle_text_editing(app, code, modifiers) {
        TextInputAction::Cancel => app.cancel_chain_search(),
        TextInputAction::Submit => app.submit_chain_search(),
        TextInputAction::Edit | TextInputAction::None => {}
    }
}

fn handle_visual_select_mode<T: Terminal, A: Agent>(app: &mut App<T, A>, code: KeyCode) {
    let kb = &app.wagner.config.keybindings;

    if code == KeyCode::Esc || matches_key(code, &kb.copy_mode) {
        app.cancel_visual_select();
    } else if code == KeyCode::Char('y') {
        app.visual_yank();
    } else if matches_key(code, &kb.nav_down) || code == KeyCode::Down {
        app.visual_select_down();
    } else if matches_key(code, &kb.nav_up) || code == KeyCode::Up {
        app.visual_select_up();
    } else if code == KeyCode::Char('G') || matches_key(code, &kb.scroll_bottom) {
        let line_count = app.terminal_output.lines().count();
        app.visual_select_end = line_count.saturating_sub(1);
    } else if code == KeyCode::Char('g') || matches_key(code, &kb.scroll_top) {
        app.visual_select_end = 0;
    } else if matches_key(code, &kb.quit) {
        app.cancel_visual_select();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Keybindings;

    #[test]
    fn matches_key_char() {
        assert!(matches_key(KeyCode::Char('q'), "q"));
        assert!(matches_key(KeyCode::Char('?'), "?"));
        assert!(!matches_key(KeyCode::Char('q'), "Q"));
        assert!(!matches_key(KeyCode::Char('a'), "b"));
    }

    #[test]
    fn matches_key_special() {
        assert!(matches_key(KeyCode::Tab, "Tab"));
        assert!(matches_key(KeyCode::Esc, "Esc"));
        assert!(matches_key(KeyCode::Enter, "Enter"));
        assert!(!matches_key(KeyCode::Tab, "tab"));
        assert!(!matches_key(KeyCode::Backspace, "Backspace"));
    }

    #[test]
    fn get_action_returns_quit_for_esc() {
        let kb = Keybindings::default();
        assert_eq!(get_action(KeyCode::Esc, &kb), Some(Action::Quit));
    }

    #[test]
    fn get_action_maps_default_bindings() {
        let kb = Keybindings::default();

        assert_eq!(get_action(KeyCode::Char('q'), &kb), Some(Action::Quit));
        assert_eq!(get_action(KeyCode::Char('?'), &kb), Some(Action::Help));
        assert_eq!(get_action(KeyCode::Char('r'), &kb), Some(Action::Refresh));
        assert_eq!(get_action(KeyCode::Char('a'), &kb), Some(Action::Attach));
        assert_eq!(get_action(KeyCode::Char('n'), &kb), Some(Action::NewTask));
        assert_eq!(get_action(KeyCode::Char('p'), &kb), Some(Action::AddPane));
        assert_eq!(get_action(KeyCode::Char('d'), &kb), Some(Action::Delete));
        assert_eq!(
            get_action(KeyCode::Char('s'), &kb),
            Some(Action::SendMessage)
        );
        assert_eq!(get_action(KeyCode::Tab, &kb), Some(Action::NextTab));
        assert_eq!(
            get_action(KeyCode::Char('o'), &kb),
            Some(Action::SwitchSection)
        );
        assert_eq!(get_action(KeyCode::Char('S'), &kb), Some(Action::Settings));
        assert_eq!(get_action(KeyCode::Char('c'), &kb), Some(Action::OpenDiff));
    }

    #[test]
    fn get_action_returns_none_for_unbound() {
        let kb = Keybindings::default();
        assert_eq!(get_action(KeyCode::Char('z'), &kb), None);
        assert_eq!(get_action(KeyCode::F(1), &kb), None);
    }

    #[test]
    fn tmux_key_map_contains_expected_keys() {
        let expected = [
            (KeyCode::Enter, "Enter"),
            (KeyCode::Backspace, "BSpace"),
            (KeyCode::Left, "Left"),
            (KeyCode::Right, "Right"),
            (KeyCode::Up, "Up"),
            (KeyCode::Down, "Down"),
            (KeyCode::Tab, "Tab"),
        ];

        for (code, tmux_str) in expected {
            let found = TMUX_KEY_MAP.iter().find(|(k, _)| *k == code);
            assert!(found.is_some(), "Missing {:?} in TMUX_KEY_MAP", code);
            assert_eq!(found.unwrap().1, tmux_str);
        }
    }

    #[test]
    fn tmux_key_map_lookup() {
        let code = KeyCode::PageUp;
        let result = TMUX_KEY_MAP.iter().find(|(k, _)| *k == code);
        assert_eq!(result, Some(&(KeyCode::PageUp, "PageUp")));

        let code = KeyCode::Char('x');
        let result = TMUX_KEY_MAP.iter().find(|(k, _)| *k == code);
        assert!(result.is_none());
    }
}
