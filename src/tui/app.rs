use crate::agent::Agent;
use crate::error::Result;
use crate::git::{DiffFile, RepoStats};
use crate::model::Task;
use crate::monitor::{PaneStatus, SessionAggregateStatus, StatusMonitor};
use crate::plugins::chains::ChainsData;
use crate::terminal::{PaneHandle, SessionHandle, Terminal, session_name_for_task};
use crate::wagner::{RepoSpec, Wagner, default_branch_for_task};

use ratatui::layout::Rect;
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppTab {
    #[default]
    Tasks,
    Chains,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChainsViewMode {
    #[default]
    ChainList,
    LinkList,
    LinkPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Tasks,
    Panes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    NewTask,
    SelectWorkspace,
    SendMessage,
    Confirm,
    Settings,
    EditSetting,
    DiffFileList,
    DiffContent,
}

pub struct App<T: Terminal, A: Agent> {
    pub wagner: Wagner<T, A>,
    pub should_quit: bool,
    pub pending_attach: Option<(String, Option<String>)>,
    pub show_sidebar: bool,
    pub show_help: bool,
    pub focus: Focus,
    pub sidebar_section: SidebarSection,
    pub input_mode: InputMode,

    pub tasks: Vec<Task>,
    pub panes: Vec<PaneHandle>,
    pub pane_statuses: HashMap<String, PaneStatus>,
    pub expanded_tasks: HashSet<String>,
    pub task_list_state: ListState,
    pub pane_list_state: ListState,

    pub selected_task: Option<String>,
    pub selected_repo: Option<String>,
    pub selected_pane: Option<String>,
    pub terminal_output: String,
    pub terminal_scroll: u16,
    pane_scroll_positions: HashMap<String, u16>,

    pub last_refresh: Instant,
    pub refresh_interval: Duration,
    pub auto_refresh: bool,
    status_monitor: StatusMonitor,

    pub input_buffer: String,
    pub input_cursor: usize,
    pub input_label: String,
    pub confirm_action: Option<String>,
    pub status_message: Option<(String, Instant)>,

    pub settings_items: Vec<(String, String)>,
    pub settings_index: usize,
    pub editing_setting_key: Option<String>,

    pub diff_repo_path: Option<std::path::PathBuf>,
    pub diff_repo_name: Option<String>,
    pub diff_files: Vec<DiffFile>,
    pub diff_file_index: usize,
    pub diff_content: Vec<String>,
    pub diff_scroll: usize,
    pub repo_stats: HashMap<String, RepoStats>,

    last_click: Option<(u16, u16, Instant)>,
    terminal_view_size: Option<(u16, u16)>,
    pub dragging_sidebar: bool,

    pub pending_task_name: Option<String>,
    pub workspace_list: Vec<String>,
    pub workspace_index: usize,

    pub current_tab: AppTab,
    pub chains_data: Option<ChainsData>,
    pub chains_view_mode: ChainsViewMode,
    pub chains_list_state: ListState,
    pub selected_chain_idx: Option<usize>,
    pub selected_link_idx: Option<usize>,
    pub chain_link_content: String,
    pub chain_link_scroll: usize,
}

impl<T: Terminal, A: Agent> App<T, A> {
    pub fn new(wagner: Wagner<T, A>) -> Self {
        let tasks = wagner.list_tasks().unwrap_or_default();
        let first_task = tasks.first().map(|t| t.name.clone());
        let refresh_interval_ms = wagner.config.refresh_interval_ms;
        let detector = wagner.agent.detector();

        let mut task_list_state = ListState::default();
        if !tasks.is_empty() {
            task_list_state.select(Some(0));
        }

        Self {
            wagner,
            should_quit: false,
            pending_attach: None,
            show_sidebar: true,
            show_help: false,
            focus: Focus::Sidebar,
            sidebar_section: SidebarSection::Tasks,
            input_mode: InputMode::Normal,

            tasks,
            panes: Vec::new(),
            pane_statuses: HashMap::new(),
            expanded_tasks: HashSet::new(),
            task_list_state,
            pane_list_state: ListState::default(),

            selected_task: first_task,
            selected_repo: None,
            selected_pane: None,
            terminal_output: String::new(),
            terminal_scroll: 0,
            pane_scroll_positions: HashMap::new(),

            last_refresh: Instant::now(),
            refresh_interval: Duration::from_millis(refresh_interval_ms),
            auto_refresh: true,
            status_monitor: StatusMonitor::new(detector),

            input_buffer: String::new(),
            input_cursor: 0,
            input_label: String::new(),
            confirm_action: None,
            status_message: None,

            settings_items: Vec::new(),
            settings_index: 0,
            editing_setting_key: None,

            diff_repo_path: None,
            diff_repo_name: None,
            diff_files: Vec::new(),
            diff_file_index: 0,
            diff_content: Vec::new(),
            diff_scroll: 0,
            repo_stats: HashMap::new(),

            last_click: None,
            terminal_view_size: None,
            dragging_sidebar: false,

            pending_task_name: None,
            workspace_list: Vec::new(),
            workspace_index: 0,

            current_tab: AppTab::default(),
            chains_data: None,
            chains_view_mode: ChainsViewMode::default(),
            chains_list_state: ListState::default(),
            selected_chain_idx: None,
            selected_link_idx: None,
            chain_link_content: String::new(),
            chain_link_scroll: 0,
        }
    }

    pub fn handle_click(&mut self, col: u16, row: u16, area: Rect) {
        let is_double_click = self
            .last_click
            .map(|(lc, lr, t)| lc == col && lr == row && t.elapsed() < Duration::from_millis(300))
            .unwrap_or(false);
        self.last_click = Some((col, row, Instant::now()));
        let sidebar_width = self.wagner.config.sidebar_width;

        let main_height = if self.status_message.is_some() {
            area.height.saturating_sub(1)
        } else {
            area.height
        };

        if !self.show_sidebar {
            self.focus = Focus::Terminal;
            return;
        }

        if col < sidebar_width {
            self.focus = Focus::Sidebar;
            let sidebar_chunks = ratatui::layout::Layout::vertical([
                ratatui::layout::Constraint::Length(1),
                ratatui::layout::Constraint::Percentage(60),
                ratatui::layout::Constraint::Min(0),
            ])
            .split(Rect::new(0, 0, sidebar_width, main_height));

            let task_area = sidebar_chunks[1];
            let pane_area = sidebar_chunks[2];

            let task_inner_start = task_area.y + 1;
            let task_inner_end = task_area.y + task_area.height.saturating_sub(1);
            if row >= task_inner_start && row < task_inner_end {
                self.sidebar_section = SidebarSection::Tasks;
                let clicked_row = (row - task_inner_start) as usize;
                self.select_task_by_row(clicked_row, is_double_click);
            }

            let pane_inner_start = pane_area.y + 1;
            let pane_inner_end = pane_area.y + pane_area.height.saturating_sub(1);
            if row >= pane_inner_start && row < pane_inner_end {
                self.sidebar_section = SidebarSection::Panes;
                let clicked_row = (row - pane_inner_start) as usize;
                if is_double_click {
                    self.focus = Focus::Terminal;
                } else {
                    self.select_pane(clicked_row);
                }
            }
        } else {
            self.focus = Focus::Terminal;
        }
    }

    pub fn handle_resize(&mut self, area: Rect) {
        let terminal_width = if self.show_sidebar {
            area.width.saturating_sub(self.wagner.config.sidebar_width)
        } else {
            area.width
        };
        let terminal_height = area.height.saturating_sub(2);

        let new_size = (terminal_width, terminal_height);
        let size_changed = self.terminal_view_size != Some(new_size);
        self.terminal_view_size = Some(new_size);

        if size_changed {
            self.resize_current_pane();
        }
    }

    fn resize_current_pane(&mut self) {
        let _ = self.refresh_terminal_output();
    }

    pub fn handle_sidebar_drag(&mut self, col: u16, area: Rect) {
        let min_width = 20u16;
        let max_width = area.width.saturating_sub(20);
        let new_width = col.clamp(min_width, max_width);
        self.wagner.config.sidebar_width = new_width;
        self.handle_resize(area);
    }

    pub fn is_on_sidebar_border(&self, col: u16) -> bool {
        if !self.show_sidebar {
            return false;
        }
        let border_col = self.wagner.config.sidebar_width.saturating_sub(1);
        col == border_col || col == border_col + 1
    }

    pub fn select_task_by_row(&mut self, row: usize, toggle_expand: bool) {
        let mut current_row = 0;
        let tasks_snapshot: Vec<_> = self
            .tasks
            .iter()
            .map(|t| {
                (
                    t.name.clone(),
                    t.repos.iter().map(|r| r.name.clone()).collect::<Vec<_>>(),
                )
            })
            .collect();

        for (task_name, repo_names) in tasks_snapshot {
            if current_row == row {
                self.save_pane_scroll();
                self.selected_task = Some(task_name.clone());
                self.selected_repo = None;
                self.selected_pane = None;
                self.update_task_list_selection();
                self.refresh_panes();
                let _ = self.refresh_terminal_output();
                self.restore_pane_scroll();
                if toggle_expand {
                    self.toggle_task_expand();
                }
                return;
            }
            current_row += 1;

            if self.expanded_tasks.contains(&task_name) {
                for repo_name in &repo_names {
                    if current_row == row {
                        self.save_pane_scroll();
                        self.selected_task = Some(task_name.clone());
                        self.selected_repo = Some(repo_name.clone());
                        self.update_task_list_selection();
                        self.restore_pane_scroll();
                        if toggle_expand {
                            self.open_diff_for_repo(repo_name);
                        }
                        return;
                    }
                    current_row += 1;
                }
            }
        }
    }

    pub fn get_diff_base(&self) -> String {
        if let Some(task_name) = &self.selected_task {
            if let Ok(task) = self.wagner.get_task(task_name) {
                if let Some(base) = task.diff_base {
                    return base;
                }
            }
        }
        self.wagner.config.diff_base.clone()
    }

    pub fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<impl ratatui::backend::Backend + std::io::Write>,
    ) -> Result<()> {
        use super::event::handle_events;
        use super::ui::draw;
        use crate::error::WagnerError;
        use crossterm::{
            event::{DisableMouseCapture, EnableMouseCapture},
            execute,
            terminal::{
                EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
            },
        };

        let size = terminal
            .size()
            .map_err(|e| WagnerError::Terminal(e.to_string()))?;
        let initial_area = Rect::new(0, 0, size.width, size.height);
        self.handle_resize(initial_area);
        self.refresh_data()?;

        while !self.should_quit {
            if let Some((task_name, pane_id)) = self.pending_attach.take() {
                execute!(
                    terminal.backend_mut(),
                    LeaveAlternateScreen,
                    DisableMouseCapture
                )
                .map_err(|e| WagnerError::Terminal(e.to_string()))?;
                disable_raw_mode().map_err(|e| WagnerError::Terminal(e.to_string()))?;

                let _ = self.wagner.attach(&task_name, pane_id.as_deref());

                enable_raw_mode().map_err(|e| WagnerError::Terminal(e.to_string()))?;
                execute!(
                    terminal.backend_mut(),
                    EnterAlternateScreen,
                    EnableMouseCapture
                )
                .map_err(|e| WagnerError::Terminal(e.to_string()))?;
                terminal
                    .clear()
                    .map_err(|e| WagnerError::Terminal(e.to_string()))?;

                self.refresh_data()?;
                continue;
            }

            let size = terminal
                .size()
                .map_err(|e| WagnerError::Terminal(e.to_string()))?;
            let area = Rect::new(0, 0, size.width, size.height);

            self.handle_resize(area);

            terminal
                .draw(|frame| draw(frame, self))
                .map_err(|e| WagnerError::Terminal(e.to_string()))?;

            if handle_events(self, area)? {
                continue;
            }

            if self.auto_refresh && self.last_refresh.elapsed() >= self.refresh_interval {
                self.refresh_data()?;
            }
        }

        Ok(())
    }

    pub fn refresh_data(&mut self) -> Result<()> {
        self.tasks = self.wagner.list_tasks().unwrap_or_default();
        self.refresh_panes();
        self.refresh_terminal_output()?;
        self.last_refresh = Instant::now();
        Ok(())
    }

    pub fn refresh_panes(&mut self) {
        self.panes.clear();
        if let Some(task_name) = &self.selected_task {
            let session_name = session_name_for_task(task_name);
            if let Ok(panes) = self
                .wagner
                .terminal
                .list_panes(&SessionHandle(session_name.clone()))
            {
                self.panes = panes;
                if self.selected_pane.is_none() && !self.panes.is_empty() {
                    self.pane_list_state.select(Some(0));
                    self.selected_pane = Some(self.panes[0].0.clone());
                } else if let Some(pane_id) = &self.selected_pane {
                    let idx = self.panes.iter().position(|p| &p.0 == pane_id);
                    self.pane_list_state.select(idx);
                }

                let updates = self.status_monitor.poll_active(
                    &self.wagner.terminal,
                    &session_name,
                    &self.panes,
                );
                for update in updates {
                    self.pane_statuses
                        .insert(update.pane.0.clone(), update.status);
                }

                for pane in &self.panes {
                    if !self.pane_statuses.contains_key(&pane.0) {
                        if let Some(status) =
                            self.status_monitor.get_pane_status(&session_name, &pane.0)
                        {
                            self.pane_statuses.insert(pane.0.clone(), status.clone());
                        }
                    }
                }

                self.poll_background_sessions(&session_name);
            }
        }
    }

    fn poll_background_sessions(&mut self, active_session: &str) {
        let all_sessions: Vec<_> = self
            .tasks
            .iter()
            .filter_map(|task| {
                let session_name = session_name_for_task(&task.name);
                self.wagner
                    .terminal
                    .list_panes(&SessionHandle(session_name.clone()))
                    .ok()
                    .map(|panes| (session_name, panes))
            })
            .collect();

        self.status_monitor.poll_background(
            &self.wagner.terminal,
            &all_sessions,
            Some(active_session),
        );
    }

    pub fn get_task_status(&self, task_name: &str) -> SessionAggregateStatus {
        let session_name = session_name_for_task(task_name);
        self.status_monitor.get_session_status(&session_name)
    }

    pub fn refresh_terminal_output(&mut self) -> Result<()> {
        let old_len = self.terminal_output.len();
        let max_scroll_before = self.get_max_scroll();
        let was_near_bottom = self.terminal_scroll >= max_scroll_before.saturating_sub(3);

        if let Some(pane) = self.current_pane() {
            self.capture_pane(&pane);
        } else if let Some(task_name) = &self.selected_task.clone() {
            if let Ok(task) = self.wagner.get_task(task_name) {
                if !task.repos.is_empty() {
                    let session_name = session_name_for_task(task_name);
                    if let Ok(panes) = self
                        .wagner
                        .terminal
                        .list_panes(&SessionHandle(session_name))
                    {
                        if let Some(first_pane) = panes.first() {
                            self.selected_pane = Some(first_pane.0.clone());
                            self.capture_pane(first_pane);
                        }
                    }
                }
            }
        } else {
            self.terminal_output =
                String::from("No task selected. Press 'n' to create a new task.");
        }

        if self.terminal_output.len() > old_len && was_near_bottom {
            self.scroll_terminal_bottom();
        }
        Ok(())
    }

    pub fn toggle_sidebar(&mut self) {
        self.show_sidebar = !self.show_sidebar;
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn toggle_task_expand(&mut self) {
        if let Some(ref name) = self.selected_task {
            if self.expanded_tasks.contains(name) {
                self.expanded_tasks.remove(name);
                self.selected_repo = None;
                self.update_task_list_selection();
            } else {
                self.expanded_tasks.insert(name.clone());
                self.refresh_repo_stats();
            }
        }
    }

    pub fn next_task(&mut self) {
        if self.tasks.is_empty() {
            return;
        }

        let Some(task_name) = &self.selected_task else {
            self.selected_task = self.tasks.first().map(|t| t.name.clone());
            self.task_list_state.select(Some(0));
            return;
        };

        let task_idx = self
            .tasks
            .iter()
            .position(|t| &t.name == task_name)
            .unwrap_or(0);
        let task = &self.tasks[task_idx];
        let is_expanded = self.expanded_tasks.contains(&task.name);

        if is_expanded && !task.repos.is_empty() {
            if let Some(repo_name) = &self.selected_repo {
                let repo_idx = task
                    .repos
                    .iter()
                    .position(|r| &r.name == repo_name)
                    .unwrap_or(0);
                if repo_idx + 1 < task.repos.len() {
                    self.selected_repo = Some(task.repos[repo_idx + 1].name.clone());
                    self.update_task_list_selection();
                    return;
                }
            } else {
                self.selected_repo = Some(task.repos[0].name.clone());
                self.update_task_list_selection();
                return;
            }
        }

        let next_idx = if task_idx + 1 >= self.tasks.len() {
            0
        } else {
            task_idx + 1
        };
        self.save_pane_scroll();
        self.selected_task = Some(self.tasks[next_idx].name.clone());
        self.selected_repo = None;
        self.selected_pane = None;
        self.update_task_list_selection();
        self.refresh_panes();
        let _ = self.refresh_terminal_output();
        self.restore_pane_scroll();
    }

    pub fn prev_task(&mut self) {
        if self.tasks.is_empty() {
            return;
        }

        let Some(task_name) = &self.selected_task else {
            self.selected_task = self.tasks.last().map(|t| t.name.clone());
            self.update_task_list_selection();
            return;
        };

        let task_idx = self
            .tasks
            .iter()
            .position(|t| &t.name == task_name)
            .unwrap_or(0);
        let task = &self.tasks[task_idx];
        let is_expanded = self.expanded_tasks.contains(&task.name);

        if is_expanded && !task.repos.is_empty() {
            if let Some(repo_name) = &self.selected_repo {
                let repo_idx = task
                    .repos
                    .iter()
                    .position(|r| &r.name == repo_name)
                    .unwrap_or(0);
                if repo_idx > 0 {
                    self.selected_repo = Some(task.repos[repo_idx - 1].name.clone());
                    self.update_task_list_selection();
                    return;
                } else {
                    self.selected_repo = None;
                    self.update_task_list_selection();
                    return;
                }
            }
        }

        let prev_idx = if task_idx == 0 {
            self.tasks.len() - 1
        } else {
            task_idx - 1
        };
        let prev_task = &self.tasks[prev_idx];
        let prev_task_name = prev_task.name.clone();
        let prev_task_expanded = self.expanded_tasks.contains(&prev_task.name);
        let prev_task_last_repo = prev_task.repos.last().map(|r| r.name.clone());

        self.save_pane_scroll();
        self.selected_task = Some(prev_task_name);

        if prev_task_expanded && prev_task_last_repo.is_some() {
            self.selected_repo = prev_task_last_repo;
        } else {
            self.selected_repo = None;
        }

        self.selected_pane = None;
        self.update_task_list_selection();
        self.refresh_panes();
        let _ = self.refresh_terminal_output();
        self.restore_pane_scroll();
    }

    fn update_task_list_selection(&mut self) {
        let mut pos = 0;
        for task in &self.tasks {
            if Some(&task.name) == self.selected_task.as_ref() {
                if self.selected_repo.is_none() {
                    self.task_list_state.select(Some(pos));
                    return;
                }
                pos += 1;
                if self.expanded_tasks.contains(&task.name) {
                    for repo in &task.repos {
                        if Some(&repo.name) == self.selected_repo.as_ref() {
                            self.task_list_state.select(Some(pos));
                            return;
                        }
                        pos += 1;
                    }
                }
                return;
            }
            pos += 1;
            if self.expanded_tasks.contains(&task.name) {
                pos += task.repos.len();
            }
        }
    }

    pub fn next_pane(&mut self) {
        self.navigate_pane(true);
    }

    pub fn prev_pane(&mut self) {
        self.navigate_pane(false);
    }

    fn navigate_pane(&mut self, forward: bool) {
        if self.panes.is_empty() {
            return;
        }
        let len = self.panes.len();
        let i = match self.pane_list_state.selected() {
            Some(i) if forward => {
                if i >= len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            Some(i) => {
                if i == 0 {
                    len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.select_pane_at(i);
    }

    pub fn select_pane(&mut self, index: usize) {
        if index < self.panes.len() {
            self.select_pane_at(index);
            self.sidebar_section = SidebarSection::Panes;
        }
    }

    fn select_pane_at(&mut self, index: usize) {
        self.save_pane_scroll();
        self.pane_list_state.select(Some(index));
        self.selected_pane = self.panes.get(index).map(|p| p.0.clone());
        self.resize_current_pane();
        self.restore_pane_scroll();
    }

    pub fn toggle_sidebar_section(&mut self) {
        self.sidebar_section = match self.sidebar_section {
            SidebarSection::Tasks => SidebarSection::Panes,
            SidebarSection::Panes => SidebarSection::Tasks,
        };
    }

    pub fn scroll_terminal_up(&mut self) {
        self.terminal_scroll = self.terminal_scroll.saturating_sub(1);
    }

    pub fn scroll_terminal_down(&mut self) {
        let max_scroll = self.get_max_scroll();
        if self.terminal_scroll < max_scroll {
            self.terminal_scroll = self.terminal_scroll.saturating_add(1);
        }
    }

    pub fn scroll_terminal_page_up(&mut self, page_size: u16) {
        self.terminal_scroll = self.terminal_scroll.saturating_sub(page_size);
    }

    pub fn scroll_terminal_page_down(&mut self, page_size: u16) {
        let max_scroll = self.get_max_scroll();
        self.terminal_scroll = (self.terminal_scroll + page_size).min(max_scroll);
    }

    pub fn scroll_terminal_top(&mut self) {
        self.terminal_scroll = 0;
    }

    pub fn scroll_terminal_bottom(&mut self) {
        self.terminal_scroll = self.get_max_scroll();
    }

    fn get_max_scroll(&self) -> u16 {
        let line_count = self.terminal_output.lines().count();
        let viewport_height = self
            .terminal_view_size
            .map(|(_, h)| h as usize)
            .unwrap_or(20);
        line_count.saturating_sub(viewport_height) as u16
    }

    fn save_pane_scroll(&mut self) {
        if let Some(pane_id) = &self.selected_pane {
            self.pane_scroll_positions
                .insert(pane_id.clone(), self.terminal_scroll);
        }
    }

    fn restore_pane_scroll(&mut self) {
        let saved = self
            .selected_pane
            .as_ref()
            .and_then(|id| self.pane_scroll_positions.get(id))
            .copied()
            .unwrap_or(0);
        let max = self.get_max_scroll();
        self.terminal_scroll = saved.min(max);
    }

    pub fn current_pane(&self) -> Option<PaneHandle> {
        self.selected_pane
            .as_ref()
            .map(|id| PaneHandle(id.clone(), String::new()))
    }

    fn capture_pane(&mut self, pane: &PaneHandle) {
        if let Some((w, h)) = self.terminal_view_size {
            if w > 0 && h > 0 {
                let _ = self.wagner.terminal.resize_pane(pane, w, h);
            }
        }

        let capture_lines = self.wagner.config.capture_lines;
        match self.wagner.terminal.capture(pane, capture_lines) {
            Ok(output) => self.terminal_output = output,
            Err(e) => {
                tracing::warn!(pane = %pane.0, error = %e, "Capture failed");
                self.terminal_output = "[No output captured]".into();
            }
        }
    }

    pub fn attach_current(&mut self) {
        if let Some(task_name) = &self.selected_task {
            self.pending_attach = Some((task_name.clone(), self.selected_pane.clone()));
        }
    }

    pub fn start_new_task(&mut self) {
        if self.wagner.config.workspaces.is_empty() {
            self.set_status("No workspaces configured. Use: wagner workspace add <name> repo:path");
            return;
        }
        self.input_mode = InputMode::NewTask;
        self.input_buffer.clear();
        self.input_cursor = 0;
        self.input_label = "Task name".to_string();
    }

    pub fn start_send_message(&mut self) {
        if self.selected_pane.is_some() {
            self.input_mode = InputMode::SendMessage;
            self.input_buffer.clear();
            self.input_cursor = 0;
            self.input_label = "Message".to_string();
        } else {
            self.set_status("No pane selected");
        }
    }

    pub fn start_delete(&mut self) {
        if let Some(task_name) = &self.selected_task {
            self.input_mode = InputMode::Confirm;
            self.confirm_action = Some(task_name.clone());
            self.input_label = format!("Delete '{}'? [y/n]", task_name);
        } else {
            self.set_status("No task selected");
        }
    }

    pub fn add_pane(&mut self) {
        if let Some(task_name) = &self.selected_task.clone() {
            match self.wagner.add_pane(task_name, None) {
                Ok(pane) => {
                    self.set_status(&format!("Added pane: {}", pane.0));
                    let _ = self.refresh_data();
                }
                Err(e) => {
                    self.set_status(&format!("Error: {}", e));
                }
            }
        } else {
            self.set_status("No task selected");
        }
    }

    pub fn cancel_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.confirm_action = None;
    }

    pub fn submit_input(&mut self) {
        match self.input_mode {
            InputMode::NewTask => {
                self.handle_task_name_input();
                return;
            }
            InputMode::SelectWorkspace => {
                self.create_task_from_workspace();
            }
            InputMode::SendMessage => {
                self.send_message_from_input();
            }
            InputMode::Confirm => {
                self.confirm_delete();
            }
            InputMode::Normal
            | InputMode::Settings
            | InputMode::EditSetting
            | InputMode::DiffFileList
            | InputMode::DiffContent => {}
        }
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.confirm_action = None;
    }

    fn handle_task_name_input(&mut self) {
        let name = self.input_buffer.trim().to_string();
        if name.is_empty() {
            self.set_status("Task name cannot be empty");
            self.input_mode = InputMode::Normal;
            return;
        }

        let workspaces: Vec<String> = self.wagner.config.workspaces.keys().cloned().collect();
        if workspaces.is_empty() {
            self.input_mode = InputMode::Normal;
            self.input_buffer.clear();
            return;
        }

        self.pending_task_name = Some(name);
        self.workspace_list = workspaces;
        self.workspace_index = 0;
        self.input_mode = InputMode::SelectWorkspace;
        self.input_buffer.clear();
    }

    fn create_task_from_workspace(&mut self) {
        let Some(task_name) = self.pending_task_name.take() else {
            return;
        };

        let Some(ws_name) = self.workspace_list.get(self.workspace_index) else {
            return;
        };

        let Some(workspace) = self.wagner.config.workspaces.get(ws_name) else {
            self.set_status(&format!("Workspace '{}' not found", ws_name));
            return;
        };

        let default_branch = default_branch_for_task(&task_name);
        let specs: Vec<RepoSpec> = workspace
            .repos
            .iter()
            .map(|(name, path)| {
                let expanded = shellexpand::tilde(path).into_owned();
                RepoSpec {
                    name: name.clone(),
                    source: crate::model::RepoSource::Local(std::path::PathBuf::from(expanded)),
                    branch: default_branch.clone(),
                }
            })
            .collect();

        if specs.is_empty() {
            self.set_status("Workspace has no repos");
            return;
        }

        match self.wagner.create_task(&task_name, &specs, None) {
            Ok(task) => {
                self.set_status(&format!("Created task: {}", task.name));
                self.selected_task = Some(task.name);
                let _ = self.refresh_data();
            }
            Err(e) => {
                self.set_status(&format!("Error: {}", e));
            }
        }

        self.workspace_list.clear();
        self.workspace_index = 0;
    }

    pub fn workspace_next(&mut self) {
        if !self.workspace_list.is_empty() {
            self.workspace_index = (self.workspace_index + 1) % self.workspace_list.len();
        }
    }

    pub fn workspace_prev(&mut self) {
        if !self.workspace_list.is_empty() {
            self.workspace_index = if self.workspace_index == 0 {
                self.workspace_list.len() - 1
            } else {
                self.workspace_index - 1
            };
        }
    }

    fn send_message_from_input(&mut self) {
        let Some(pane) = self.current_pane() else {
            return;
        };

        match self.wagner.terminal.send_keys(&pane, &self.input_buffer) {
            Ok(_) => {
                self.set_status("Message sent");
                let _ = self.refresh_terminal_output();
            }
            Err(e) => {
                self.set_status(&format!("Error: {}", e));
            }
        }
    }

    fn confirm_delete(&mut self) {
        if self.input_buffer.trim().eq_ignore_ascii_case("y") {
            if let Some(task_name) = &self.confirm_action.clone() {
                match self.wagner.delete_task(task_name, true) {
                    Ok(_) => {
                        self.set_status(&format!("Deleted task: {}", task_name));
                        self.selected_task = None;
                        self.selected_pane = None;
                        self.restore_pane_scroll();
                        let _ = self.refresh_data();
                    }
                    Err(e) => {
                        self.set_status(&format!("Error: {}", e));
                    }
                }
            }
        } else {
            self.set_status("Cancelled");
        }
    }

    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some((msg.to_string(), Instant::now()));
    }

    pub fn open_settings(&mut self) {
        self.settings_items = self.build_settings_items();
        self.settings_index = 0;
        self.input_mode = InputMode::Settings;
    }

    pub fn close_settings(&mut self) {
        self.input_mode = InputMode::Normal;
        self.editing_setting_key = None;
    }

    pub fn cancel_edit_setting(&mut self) {
        self.input_buffer.clear();
        self.editing_setting_key = None;
        self.input_mode = InputMode::Settings;
    }

    pub fn settings_next(&mut self) {
        if !self.settings_items.is_empty() {
            self.settings_index = (self.settings_index + 1) % self.settings_items.len();
        }
    }

    pub fn settings_prev(&mut self) {
        if !self.settings_items.is_empty() {
            self.settings_index = self
                .settings_index
                .checked_sub(1)
                .unwrap_or(self.settings_items.len() - 1);
        }
    }

    pub fn start_edit_setting(&mut self) {
        if let Some((key, value)) = self.settings_items.get(self.settings_index).cloned() {
            if key == "show_hints" {
                let new_val = value != "true";
                self.apply_setting_value(&key, if new_val { "true" } else { "false" });
                self.settings_items = self.build_settings_items();
            } else {
                self.editing_setting_key = Some(key.clone());
                self.input_buffer = value.clone();
                self.input_cursor = value.chars().count();
                self.input_label = format!("Edit: {}", key);
                self.input_mode = InputMode::EditSetting;
            }
        }
    }

    pub fn apply_setting(&mut self) {
        if let Some(key) = self.editing_setting_key.take() {
            let value = self.input_buffer.clone();
            self.apply_setting_value(&key, &value);
            self.settings_items = self.build_settings_items();
            self.input_mode = InputMode::Settings;
            self.input_buffer.clear();
            if let Err(e) = self.wagner.config.save() {
                self.set_status(&format!("Error saving: {}", e));
            }
        }
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = self.wagner.config.save() {
            self.set_status(&format!("Error saving: {}", e));
        } else {
            self.set_status("Settings saved");
        }
        self.close_settings();
    }

    fn build_settings_items(&self) -> Vec<(String, String)> {
        let cfg = &self.wagner.config;
        let kb = &cfg.keybindings;
        vec![
            (
                "tasks_root".to_string(),
                cfg.tasks_root.display().to_string(),
            ),
            (
                "refresh_interval_ms".to_string(),
                cfg.refresh_interval_ms.to_string(),
            ),
            ("default_agent".to_string(), cfg.default_agent.clone()),
            ("show_hints".to_string(), cfg.show_hints.to_string()),
            ("sidebar_width".to_string(), cfg.sidebar_width.to_string()),
            (
                "page_scroll_lines".to_string(),
                cfg.page_scroll_lines.to_string(),
            ),
            ("diff_base".to_string(), cfg.diff_base.clone()),
            ("key.quit".to_string(), kb.quit.clone()),
            ("key.help".to_string(), kb.help.clone()),
            ("key.refresh".to_string(), kb.refresh.clone()),
            ("key.attach".to_string(), kb.attach.clone()),
            ("key.new_task".to_string(), kb.new_task.clone()),
            ("key.add_pane".to_string(), kb.add_pane.clone()),
            ("key.delete".to_string(), kb.delete.clone()),
            ("key.send_message".to_string(), kb.send_message.clone()),
            ("key.toggle_sidebar".to_string(), kb.toggle_sidebar.clone()),
            ("key.switch_section".to_string(), kb.switch_section.clone()),
            ("key.settings".to_string(), kb.settings.clone()),
            ("key.nav_down".to_string(), kb.nav_down.clone()),
            ("key.nav_up".to_string(), kb.nav_up.clone()),
            ("key.nav_left".to_string(), kb.nav_left.clone()),
            ("key.nav_right".to_string(), kb.nav_right.clone()),
            ("key.scroll_top".to_string(), kb.scroll_top.clone()),
            ("key.scroll_bottom".to_string(), kb.scroll_bottom.clone()),
            ("key.page_up".to_string(), kb.page_up.clone()),
            ("key.page_down".to_string(), kb.page_down.clone()),
            ("key.open_diff".to_string(), kb.open_diff.clone()),
        ]
    }

    fn apply_setting_value(&mut self, key: &str, value: &str) {
        let cfg = &mut self.wagner.config;
        match key {
            "tasks_root" => cfg.tasks_root = std::path::PathBuf::from(value),
            "refresh_interval_ms" => {
                if let Ok(v) = value.parse() {
                    cfg.refresh_interval_ms = v;
                    self.refresh_interval = Duration::from_millis(v);
                }
            }
            "default_agent" => cfg.default_agent = value.to_string(),
            "show_hints" => cfg.show_hints = value == "true",
            "sidebar_width" => {
                if let Ok(v) = value.parse() {
                    cfg.sidebar_width = v;
                }
            }
            "page_scroll_lines" => {
                if let Ok(v) = value.parse() {
                    cfg.page_scroll_lines = v;
                }
            }
            "diff_base" => cfg.diff_base = value.to_string(),
            "key.quit" => cfg.keybindings.quit = value.to_string(),
            "key.help" => cfg.keybindings.help = value.to_string(),
            "key.refresh" => cfg.keybindings.refresh = value.to_string(),
            "key.attach" => cfg.keybindings.attach = value.to_string(),
            "key.new_task" => cfg.keybindings.new_task = value.to_string(),
            "key.add_pane" => cfg.keybindings.add_pane = value.to_string(),
            "key.delete" => cfg.keybindings.delete = value.to_string(),
            "key.send_message" => cfg.keybindings.send_message = value.to_string(),
            "key.toggle_sidebar" => cfg.keybindings.toggle_sidebar = value.to_string(),
            "key.switch_section" => cfg.keybindings.switch_section = value.to_string(),
            "key.settings" => cfg.keybindings.settings = value.to_string(),
            "key.nav_down" => cfg.keybindings.nav_down = value.to_string(),
            "key.nav_up" => cfg.keybindings.nav_up = value.to_string(),
            "key.nav_left" => cfg.keybindings.nav_left = value.to_string(),
            "key.nav_right" => cfg.keybindings.nav_right = value.to_string(),
            "key.scroll_top" => cfg.keybindings.scroll_top = value.to_string(),
            "key.scroll_bottom" => cfg.keybindings.scroll_bottom = value.to_string(),
            "key.page_up" => cfg.keybindings.page_up = value.to_string(),
            "key.page_down" => cfg.keybindings.page_down = value.to_string(),
            "key.open_diff" => cfg.keybindings.open_diff = value.to_string(),
            _ => {}
        }
    }

    pub fn input_char(&mut self, c: char) {
        let byte_pos = self.char_to_byte_pos(self.input_cursor);
        self.input_buffer.insert(byte_pos, c);
        self.input_cursor += 1;
    }

    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
            self.delete_char_at_cursor();
        }
    }

    pub fn input_delete(&mut self) {
        if self.input_cursor < self.input_buffer.chars().count() {
            self.delete_char_at_cursor();
        }
    }

    fn delete_char_at_cursor(&mut self) {
        let byte_pos = self.char_to_byte_pos(self.input_cursor);
        if let Some(c) = self.input_buffer.chars().nth(self.input_cursor) {
            self.input_buffer
                .replace_range(byte_pos..byte_pos + c.len_utf8(), "");
        }
    }

    pub fn input_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
    }

    pub fn input_right(&mut self) {
        let char_count = self.input_buffer.chars().count();
        if self.input_cursor < char_count {
            self.input_cursor += 1;
        }
    }

    fn char_to_byte_pos(&self, char_pos: usize) -> usize {
        self.input_buffer
            .char_indices()
            .nth(char_pos)
            .map(|(i, _)| i)
            .unwrap_or(self.input_buffer.len())
    }

    pub fn open_diff_view(&mut self) {
        let repo_name = self.selected_repo.clone();
        if !self.open_diff_for_repo_impl(repo_name.as_deref(), true) {
            if self.selected_task.is_none() {
                self.set_status("No task selected");
            }
        }
    }

    pub fn open_diff_for_repo(&mut self, repo_name: &str) {
        self.open_diff_for_repo_impl(Some(repo_name), false);
    }

    fn open_diff_for_repo_impl(&mut self, repo_name: Option<&str>, show_errors: bool) -> bool {
        let Some(task_name) = &self.selected_task else {
            return false;
        };

        let Ok(task) = self.wagner.get_task(task_name) else {
            if show_errors {
                self.set_status("Task not found");
            }
            return false;
        };

        let repo = match repo_name {
            Some(name) => task.repos.iter().find(|r| r.name == name),
            None => task.repos.first(),
        };

        let Some(repo) = repo else {
            if show_errors {
                self.set_status("No repos in task");
            }
            return false;
        };

        self.diff_repo_path = Some(repo.worktree.clone());
        self.diff_repo_name = Some(repo.name.clone());
        self.load_diff_files();
        self.input_mode = InputMode::DiffFileList;
        true
    }

    fn load_diff_files(&mut self) {
        let Some(repo_path) = &self.diff_repo_path else {
            return;
        };

        let base = self.get_diff_base();
        self.diff_files = crate::git::get_diff_files(repo_path, &base);
        self.diff_file_index = 0;
        self.diff_content.clear();
        self.diff_scroll = 0;
    }

    pub fn select_diff_file(&mut self) {
        let Some(repo_path) = &self.diff_repo_path else {
            return;
        };

        let Some(file) = self.diff_files.get(self.diff_file_index) else {
            return;
        };

        let base = self.get_diff_base();
        self.diff_content = crate::git::get_diff_content(repo_path, &base, &file.path);
        self.diff_scroll = 0;
        self.input_mode = InputMode::DiffContent;
    }

    pub fn close_diff_view(&mut self) {
        self.input_mode = InputMode::Normal;
        self.diff_files.clear();
        self.diff_content.clear();
        self.diff_file_index = 0;
        self.diff_scroll = 0;
        self.diff_repo_path = None;
        self.diff_repo_name = None;
    }

    pub fn diff_back_to_list(&mut self) {
        self.input_mode = InputMode::DiffFileList;
        self.diff_content.clear();
        self.diff_scroll = 0;
    }

    pub fn diff_next_file(&mut self) {
        if !self.diff_files.is_empty() {
            self.diff_file_index = (self.diff_file_index + 1) % self.diff_files.len();
        }
    }

    pub fn diff_prev_file(&mut self) {
        if !self.diff_files.is_empty() {
            self.diff_file_index = self
                .diff_file_index
                .checked_sub(1)
                .unwrap_or(self.diff_files.len() - 1);
        }
    }

    pub fn diff_scroll_down(&mut self) {
        if self.diff_scroll < self.diff_content.len().saturating_sub(1) {
            self.diff_scroll += 1;
        }
    }

    pub fn diff_scroll_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(1);
    }

    pub fn diff_scroll_top(&mut self) {
        self.diff_scroll = 0;
    }

    pub fn diff_scroll_bottom(&mut self) {
        self.diff_scroll = self.diff_content.len().saturating_sub(1);
    }

    pub fn refresh_repo_stats(&mut self) {
        let Some(task_name) = &self.selected_task else {
            return;
        };

        let Ok(task) = self.wagner.get_task(task_name) else {
            return;
        };

        let base = task
            .diff_base
            .as_deref()
            .unwrap_or(&self.wagner.config.diff_base);
        for repo in &task.repos {
            let stats = crate::git::get_repo_stats(&repo.worktree, base);
            let key = repo.worktree.to_string_lossy().to_string();
            self.repo_stats.insert(key, stats);
        }
    }

    pub fn switch_tab(&mut self, tab: AppTab) {
        if self.current_tab != tab {
            self.current_tab = tab;
            if tab == AppTab::Chains {
                self.refresh_chains();
            }
        }
    }

    pub fn next_tab(&mut self) {
        self.current_tab = match self.current_tab {
            AppTab::Tasks => AppTab::Chains,
            AppTab::Chains => AppTab::Tasks,
        };
        if self.current_tab == AppTab::Chains {
            self.refresh_chains();
        }
    }

    pub fn refresh_chains(&mut self) {
        use crate::plugins::chains::load_all_chains;

        match load_all_chains(&self.wagner.config.tasks_root, None) {
            Ok(data) => {
                self.chains_data = Some(data);
                if self.selected_chain_idx.is_none() {
                    self.chains_list_state.select(Some(0));
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load chains");
                self.chains_data = None;
            }
        }
    }

    pub fn chains_next(&mut self) {
        match self.chains_view_mode {
            ChainsViewMode::ChainList => {
                let total = self.total_chain_count();
                if total == 0 {
                    return;
                }
                let current = self.chains_list_state.selected().unwrap_or(0);
                let next = if current + 1 >= total { 0 } else { current + 1 };
                self.chains_list_state.select(Some(next));
            }
            ChainsViewMode::LinkList => {
                if let Some(chain_idx) = self.selected_chain_idx {
                    if let Some(chain) = self.get_chain_at_index(chain_idx) {
                        let current = self.selected_link_idx.unwrap_or(0);
                        let next = if current + 1 >= chain.links.len() {
                            0
                        } else {
                            current + 1
                        };
                        self.selected_link_idx = Some(next);
                    }
                }
            }
            ChainsViewMode::LinkPreview => {
                self.chain_link_scroll = self.chain_link_scroll.saturating_add(1);
            }
        }
    }

    pub fn chains_prev(&mut self) {
        match self.chains_view_mode {
            ChainsViewMode::ChainList => {
                let total = self.total_chain_count();
                if total == 0 {
                    return;
                }
                let current = self.chains_list_state.selected().unwrap_or(0);
                let prev = if current == 0 { total - 1 } else { current - 1 };
                self.chains_list_state.select(Some(prev));
            }
            ChainsViewMode::LinkList => {
                if let Some(chain_idx) = self.selected_chain_idx {
                    if let Some(chain) = self.get_chain_at_index(chain_idx) {
                        let current = self.selected_link_idx.unwrap_or(0);
                        let prev = if current == 0 {
                            chain.links.len().saturating_sub(1)
                        } else {
                            current - 1
                        };
                        self.selected_link_idx = Some(prev);
                    }
                }
            }
            ChainsViewMode::LinkPreview => {
                self.chain_link_scroll = self.chain_link_scroll.saturating_sub(1);
            }
        }
    }

    pub fn chains_select(&mut self) {
        match self.chains_view_mode {
            ChainsViewMode::ChainList => {
                if let Some(idx) = self.chains_list_state.selected() {
                    self.selected_chain_idx = Some(idx);
                    self.selected_link_idx = Some(0);
                    self.chains_view_mode = ChainsViewMode::LinkList;
                }
            }
            ChainsViewMode::LinkList => {
                if let Some(chain_idx) = self.selected_chain_idx {
                    if let Some(link_idx) = self.selected_link_idx {
                        if let Some(chain) = self.get_chain_at_index(chain_idx) {
                            if let Some(link) = chain.links.get(link_idx) {
                                if let Ok(content) = std::fs::read_to_string(&link.file_path) {
                                    self.chain_link_content = content;
                                    self.chain_link_scroll = 0;
                                    self.chains_view_mode = ChainsViewMode::LinkPreview;
                                }
                            }
                        }
                    }
                }
            }
            ChainsViewMode::LinkPreview => {}
        }
    }

    pub fn chains_back(&mut self) {
        match self.chains_view_mode {
            ChainsViewMode::ChainList => {
                self.switch_tab(AppTab::Tasks);
            }
            ChainsViewMode::LinkList => {
                self.chains_view_mode = ChainsViewMode::ChainList;
                self.selected_chain_idx = None;
                self.selected_link_idx = None;
            }
            ChainsViewMode::LinkPreview => {
                self.chains_view_mode = ChainsViewMode::LinkList;
                self.chain_link_content.clear();
                self.chain_link_scroll = 0;
            }
        }
    }

    fn total_chain_count(&self) -> usize {
        self.chains_data
            .as_ref()
            .map(|d| d.total_chains())
            .unwrap_or(0)
    }

    pub fn get_chain_at_index(&self, idx: usize) -> Option<&crate::plugins::chains::Chain> {
        let data = self.chains_data.as_ref()?;
        let mut current = 0;

        for repo in &data.repos {
            for chain in &repo.chains {
                if current == idx {
                    return Some(chain);
                }
                current += 1;
            }
        }

        for chain in &data.task_local {
            if current == idx {
                return Some(chain);
            }
            current += 1;
        }

        None
    }

    pub fn promote_selected_chain(&mut self) {
        use crate::plugins::chains::ChainSource;

        let idx = match self.chains_list_state.selected() {
            Some(i) => i,
            None => return,
        };

        let chain = match self.get_chain_at_index(idx) {
            Some(c) => c.clone(),
            None => return,
        };

        let task_path = match &chain.source {
            ChainSource::TaskLocal(p) => p.clone(),
            ChainSource::Repo(_) => {
                self.status_message = Some((
                    "Chain is already at repo level".to_string(),
                    std::time::Instant::now(),
                ));
                return;
            }
        };

        let chain_name = chain.name.split('/').last().unwrap_or(&chain.name);
        let local_chain_dir = task_path.join(".claude").join("chains").join(chain_name);

        if !local_chain_dir.exists() {
            self.status_message = Some((
                format!("Error: Chain directory not found"),
                std::time::Instant::now(),
            ));
            return;
        }

        let plugins_link = task_path.join(".wagner").join("plugins");
        if !plugins_link.exists() || !plugins_link.is_symlink() {
            self.status_message = Some((
                "Error: No repo-level plugin storage".to_string(),
                std::time::Instant::now(),
            ));
            return;
        }

        let repo_chains_dir = match std::fs::read_link(&plugins_link) {
            Ok(target) => {
                if target.is_absolute() {
                    target.join("chains")
                } else {
                    plugins_link.parent().unwrap().join(&target).join("chains")
                }
            }
            Err(_) => {
                self.status_message = Some((
                    "Error: Could not resolve repo directory".to_string(),
                    std::time::Instant::now(),
                ));
                return;
            }
        };

        let target_chain_dir = repo_chains_dir.join(chain_name);
        if target_chain_dir.exists() {
            self.status_message = Some((
                format!("Error: Chain already exists at repo level"),
                std::time::Instant::now(),
            ));
            return;
        }

        if let Err(_) = std::fs::create_dir_all(&repo_chains_dir) {
            self.status_message = Some((
                "Error: Could not create chains directory".to_string(),
                std::time::Instant::now(),
            ));
            return;
        }

        if let Err(_) = std::fs::rename(&local_chain_dir, &target_chain_dir) {
            self.status_message = Some((
                "Error: Could not move chain".to_string(),
                std::time::Instant::now(),
            ));
            return;
        }

        self.status_message = Some((
            format!("Promoted chain '{}'", chain_name),
            std::time::Instant::now(),
        ));

        self.refresh_chains();
    }
}
