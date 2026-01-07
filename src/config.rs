use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keybindings {
    #[serde(default = "default_quit")]
    pub quit: String,
    #[serde(default = "default_help")]
    pub help: String,
    #[serde(default = "default_refresh")]
    pub refresh: String,
    #[serde(default = "default_attach")]
    pub attach: String,
    #[serde(default = "default_new_task")]
    pub new_task: String,
    #[serde(default = "default_add_pane")]
    pub add_pane: String,
    #[serde(default = "default_delete")]
    pub delete: String,
    #[serde(default = "default_send_message")]
    pub send_message: String,
    #[serde(default = "default_toggle_sidebar")]
    pub toggle_sidebar: String,
    #[serde(default = "default_switch_section")]
    pub switch_section: String,
    #[serde(default = "default_settings")]
    pub settings: String,
    #[serde(default = "default_nav_down")]
    pub nav_down: String,
    #[serde(default = "default_nav_up")]
    pub nav_up: String,
    #[serde(default = "default_nav_left")]
    pub nav_left: String,
    #[serde(default = "default_nav_right")]
    pub nav_right: String,
    #[serde(default = "default_scroll_top")]
    pub scroll_top: String,
    #[serde(default = "default_scroll_bottom")]
    pub scroll_bottom: String,
    #[serde(default = "default_page_up")]
    pub page_up: String,
    #[serde(default = "default_page_down")]
    pub page_down: String,
    #[serde(default = "default_open_diff")]
    pub open_diff: String,
}

fn default_open_diff() -> String {
    "c".to_string()
}
fn default_quit() -> String {
    "q".to_string()
}
fn default_help() -> String {
    "?".to_string()
}
fn default_refresh() -> String {
    "r".to_string()
}
fn default_attach() -> String {
    "a".to_string()
}
fn default_new_task() -> String {
    "n".to_string()
}
fn default_add_pane() -> String {
    "p".to_string()
}
fn default_delete() -> String {
    "d".to_string()
}
fn default_send_message() -> String {
    "s".to_string()
}
fn default_toggle_sidebar() -> String {
    "Tab".to_string()
}
fn default_switch_section() -> String {
    "o".to_string()
}
fn default_settings() -> String {
    "S".to_string()
}
fn default_nav_down() -> String {
    "j".to_string()
}
fn default_nav_up() -> String {
    "k".to_string()
}
fn default_nav_left() -> String {
    "h".to_string()
}
fn default_nav_right() -> String {
    "l".to_string()
}
fn default_scroll_top() -> String {
    "g".to_string()
}
fn default_scroll_bottom() -> String {
    "G".to_string()
}
fn default_page_up() -> String {
    "u".to_string()
}
fn default_page_down() -> String {
    "f".to_string()
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            quit: default_quit(),
            help: default_help(),
            refresh: default_refresh(),
            attach: default_attach(),
            new_task: default_new_task(),
            add_pane: default_add_pane(),
            delete: default_delete(),
            send_message: default_send_message(),
            toggle_sidebar: default_toggle_sidebar(),
            switch_section: default_switch_section(),
            settings: default_settings(),
            nav_down: default_nav_down(),
            nav_up: default_nav_up(),
            nav_left: default_nav_left(),
            nav_right: default_nav_right(),
            scroll_top: default_scroll_top(),
            scroll_bottom: default_scroll_bottom(),
            page_up: default_page_up(),
            page_down: default_page_down(),
            open_diff: default_open_diff(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
    #[serde(default, flatten)]
    pub repos: HashMap<String, String>,
}

fn default_base_branch() -> String {
    "main".to_string()
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            base_branch: default_base_branch(),
            repos: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub tasks_root: PathBuf,
    pub default_agent: String,
    #[serde(default = "default_refresh_ms")]
    pub refresh_interval_ms: u64,
    #[serde(default)]
    pub show_hints: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u16,
    #[serde(default = "default_page_scroll")]
    pub page_scroll_lines: u16,
    #[serde(default = "default_capture_lines")]
    pub capture_lines: usize,
    #[serde(default = "default_background_poll_ms")]
    pub background_poll_interval_ms: u64,
    #[serde(default = "default_diff_base")]
    pub diff_base: String,
    #[serde(default)]
    pub keybindings: Keybindings,
    #[serde(default)]
    pub workspaces: HashMap<String, Workspace>,
}

fn default_diff_base() -> String {
    "main".to_string()
}
fn default_refresh_ms() -> u64 {
    100
}
fn default_sidebar_width() -> u16 {
    28
}
fn default_page_scroll() -> u16 {
    20
}
fn default_capture_lines() -> usize {
    500
}
fn default_background_poll_ms() -> u64 {
    2000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tasks_root: home_dir().join("tasks"),
            default_agent: "claude".to_string(),
            refresh_interval_ms: default_refresh_ms(),
            show_hints: false,
            sidebar_width: default_sidebar_width(),
            page_scroll_lines: default_page_scroll(),
            capture_lines: default_capture_lines(),
            background_poll_interval_ms: default_background_poll_ms(),
            diff_base: default_diff_base(),
            keybindings: Keybindings::default(),
            workspaces: HashMap::new(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn config_dir() -> PathBuf {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join(".config"))
            .join("wagner")
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.json")
    }
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}
