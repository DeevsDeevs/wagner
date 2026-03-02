use ratatui::widgets::ListState;
use std::path::PathBuf;

use super::{Chain, ChainSource, ChainsData};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChainsViewMode {
    #[default]
    ChainList,
    LinkList,
    LinkPreview,
}

#[derive(Debug, Default)]
pub struct ChainsState {
    pub data: Option<ChainsData>,
    pub view_mode: ChainsViewMode,
    pub list_state: ListState,
    pub selected_chain_idx: Option<usize>,
    pub selected_link_idx: Option<usize>,
    pub link_content: String,
    pub link_scroll: usize,
    pub filter: String,
}

impl ChainsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn total_chain_count(&self) -> usize {
        self.data
            .as_ref()
            .map(|d| d.filtered_chain_count(&self.filter))
            .unwrap_or(0)
    }

    pub fn get_chain_at_index(&self, idx: usize) -> Option<&Chain> {
        let data = self.data.as_ref()?;
        data.get_filtered_chain_at_index(idx, &self.filter)
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.list_state.select(if self.total_chain_count() > 0 {
            Some(0)
        } else {
            None
        });
    }

    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.list_state.select(if self.total_chain_count() > 0 {
            Some(0)
        } else {
            None
        });
    }

    pub fn navigate_chain_list_next(&mut self) {
        let total = self.total_chain_count();
        if total == 0 {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let next = if current + 1 >= total { 0 } else { current + 1 };
        self.list_state.select(Some(next));
    }

    pub fn navigate_chain_list_prev(&mut self) {
        let total = self.total_chain_count();
        if total == 0 {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let prev = if current == 0 { total - 1 } else { current - 1 };
        self.list_state.select(Some(prev));
    }

    pub fn navigate_link_list_next(&mut self) {
        if let Some(chain_idx) = self.selected_chain_idx
            && let Some(chain) = self.get_chain_at_index(chain_idx)
        {
            if chain.links.is_empty() {
                return;
            }
            let current = self.selected_link_idx.unwrap_or(0);
            let next = if current + 1 >= chain.links.len() {
                0
            } else {
                current + 1
            };
            self.selected_link_idx = Some(next);
            self.reload_link_content_if_previewing();
        }
    }

    pub fn navigate_link_list_prev(&mut self) {
        if let Some(chain_idx) = self.selected_chain_idx
            && let Some(chain) = self.get_chain_at_index(chain_idx)
        {
            if chain.links.is_empty() {
                return;
            }
            let current = self.selected_link_idx.unwrap_or(0);
            let prev = if current == 0 {
                chain.links.len().saturating_sub(1)
            } else {
                current - 1
            };
            self.selected_link_idx = Some(prev);
            self.reload_link_content_if_previewing();
        }
    }

    fn reload_link_content_if_previewing(&mut self) {
        if self.view_mode != ChainsViewMode::LinkPreview {
            return;
        }
        self.reload_link_content();
    }

    pub fn reload_link_content(&mut self) {
        if let Some(chain_idx) = self.selected_chain_idx
            && let Some(link_idx) = self.selected_link_idx
            && let Some(chain) = self.get_chain_at_index(chain_idx)
            && let Some(link) = chain.links.get(link_idx)
            && let Ok(content) = std::fs::read_to_string(&link.file_path)
        {
            self.link_content = content;
            self.link_scroll = 0;
        }
    }

    pub fn scroll_link_preview_down(&mut self) {
        let total_lines = self.link_content.lines().count();
        let max_scroll = total_lines.saturating_sub(20);
        if self.link_scroll < max_scroll {
            self.link_scroll = self.link_scroll.saturating_add(1);
        }
    }

    pub fn scroll_link_preview_up(&mut self) {
        self.link_scroll = self.link_scroll.saturating_sub(1);
    }

    pub fn select_chain(&mut self) -> bool {
        if let Some(idx) = self.list_state.selected() {
            self.selected_chain_idx = Some(idx);
            self.selected_link_idx = Some(0);
            self.view_mode = ChainsViewMode::LinkList;
            return true;
        }
        false
    }

    pub fn select_link(&mut self) -> Result<(), String> {
        let chain_idx = self.selected_chain_idx.ok_or("No chain selected")?;
        let link_idx = self.selected_link_idx.ok_or("No link selected")?;
        let chain = self
            .get_chain_at_index(chain_idx)
            .ok_or("Chain not found")?;
        let link = chain.links.get(link_idx).ok_or("Link not found")?;

        self.link_content = std::fs::read_to_string(&link.file_path)
            .map_err(|e| format!("Failed to read {}: {}", link.file_path.display(), e))?;
        self.link_scroll = 0;
        self.view_mode = ChainsViewMode::LinkPreview;
        Ok(())
    }

    pub fn back(&mut self) -> bool {
        match self.view_mode {
            ChainsViewMode::ChainList => false,
            ChainsViewMode::LinkList => {
                self.view_mode = ChainsViewMode::ChainList;
                self.selected_chain_idx = None;
                self.selected_link_idx = None;
                true
            }
            ChainsViewMode::LinkPreview => {
                self.view_mode = ChainsViewMode::LinkList;
                self.link_content.clear();
                self.link_scroll = 0;
                true
            }
        }
    }

    pub fn is_in_main_view(&self) -> bool {
        matches!(
            self.view_mode,
            ChainsViewMode::LinkList | ChainsViewMode::LinkPreview
        )
    }

    pub fn promote_chain(&mut self, tasks_root: &std::path::Path) -> Result<String, String> {
        let idx = self.list_state.selected().ok_or("No chain selected")?;
        let chain = self
            .get_chain_at_index(idx)
            .ok_or("Chain not found")?
            .clone();

        let source_path = match &chain.source {
            ChainSource::TaskLocal(p) => p.clone(),
            ChainSource::Repo(_) => {
                return Err("Chain is already at repo level".to_string());
            }
        };

        let chain_name = chain.name.split('/').next_back().unwrap_or(&chain.name);
        let local_chain_dir = source_path.join(".claude").join("chains").join(chain_name);

        if !local_chain_dir.exists() {
            return Err("Chain directory not found".to_string());
        }

        let task_path =
            find_task_root(&source_path, tasks_root).ok_or("Could not find task directory")?;

        let plugins_link = task_path.join(".wagner").join("plugins");
        if !plugins_link.exists() || !plugins_link.is_symlink() {
            return Err(
                "No repo-level plugin storage (task created before plugin enabled?)".to_string(),
            );
        }

        let repo_chains_dir =
            std::fs::read_link(&plugins_link).map_err(|_| "Could not resolve repo directory")?;
        let repo_chains_dir = if repo_chains_dir.is_absolute() {
            repo_chains_dir.join("chains")
        } else {
            plugins_link
                .parent()
                .unwrap()
                .join(&repo_chains_dir)
                .join("chains")
        };

        let target_chain_dir = repo_chains_dir.join(chain_name);
        if target_chain_dir.exists() {
            return Err("Chain already exists at repo level".to_string());
        }

        std::fs::create_dir_all(&repo_chains_dir)
            .map_err(|_| "Could not create chains directory")?;

        std::fs::rename(&local_chain_dir, &target_chain_dir)
            .map_err(|e| format!("Could not move chain: {}", e))?;

        self.data = None;
        self.selected_chain_idx = None;
        self.list_state.select(None);

        Ok(format!("Promoted chain '{}'", chain_name))
    }

    pub fn delete_chain(&mut self) -> Result<String, String> {
        let idx = self.list_state.selected().ok_or("No chain selected")?;
        let chain = self
            .get_chain_at_index(idx)
            .ok_or("Chain not found")?
            .clone();

        let chain_name = chain.name.split('/').next_back().unwrap_or(&chain.name);

        let chain_dir = match &chain.source {
            ChainSource::TaskLocal(p) => p.join(".claude").join("chains").join(chain_name),
            ChainSource::Repo(p) => p
                .join(".wagner")
                .join("plugins")
                .join("chains")
                .join(chain_name),
        };

        if !chain_dir.exists() {
            return Err("Chain directory not found".to_string());
        }

        std::fs::remove_dir_all(&chain_dir)
            .map_err(|e| format!("Could not delete chain: {}", e))?;

        self.data = None;
        self.selected_chain_idx = None;
        self.list_state.select(None);

        Ok(format!("Deleted chain '{}'", chain_name))
    }

    pub fn selected_chain_name(&self) -> Option<String> {
        let idx = self.list_state.selected()?;
        let chain = self.get_chain_at_index(idx)?;
        Some(
            chain
                .name
                .split('/')
                .next_back()
                .unwrap_or(&chain.name)
                .to_string(),
        )
    }
}

fn find_task_root(start_path: &std::path::Path, tasks_root: &std::path::Path) -> Option<PathBuf> {
    let mut current = start_path.to_path_buf();
    loop {
        if current.join(".wagner").join("task.json").exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
        if current == tasks_root {
            break;
        }
    }
    None
}
