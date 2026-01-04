use crate::error::Result;
use serde::{Deserialize, Serialize};
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
}

fn default_quit() -> String { "q".to_string() }
fn default_help() -> String { "?".to_string() }
fn default_refresh() -> String { "r".to_string() }
fn default_attach() -> String { "a".to_string() }
fn default_new_task() -> String { "n".to_string() }
fn default_add_pane() -> String { "p".to_string() }
fn default_delete() -> String { "d".to_string() }
fn default_send_message() -> String { "s".to_string() }
fn default_toggle_sidebar() -> String { "Tab".to_string() }
fn default_switch_section() -> String { "o".to_string() }
fn default_settings() -> String { "S".to_string() }

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
    #[serde(default)]
    pub keybindings: Keybindings,
}

fn default_refresh_ms() -> u64 {
    100
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tasks_root: home_dir().join("tasks"),
            default_agent: "claude".to_string(),
            refresh_interval_ms: default_refresh_ms(),
            show_hints: false,
            keybindings: Keybindings::default(),
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

    pub fn sessions_path() -> PathBuf {
        Self::config_dir().join("sessions.json")
    }
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}
