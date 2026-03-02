pub mod chains;
pub mod states;

pub use states::PluginStates;

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginItem {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginItemDetail {
    pub item: PluginItem,
    pub content: String,
}

pub trait PluginProvider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn is_enabled(&self, config: &Config) -> bool;
    fn list_items(&self, tasks_root: &Path, task_name: Option<&str>) -> Result<Vec<PluginItem>>;
    fn get_item(
        &self,
        tasks_root: &Path,
        task_name: Option<&str>,
        item_id: &str,
    ) -> Result<Option<PluginItemDetail>>;
}

pub trait PluginData: Send + Sync {}

pub trait Plugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;

    fn data_dir(&self) -> &'static str;

    fn is_enabled(&self, config: &Config) -> bool;

    fn agent_skills(&self) -> &[&'static str];

    fn skill_source_dir(&self) -> Option<&'static str> {
        None
    }
}

pub fn builtin_plugins() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(chains::ChainsPlugin)]
}

pub fn get_plugin(id: &str) -> Option<Box<dyn Plugin>> {
    builtin_plugins().into_iter().find(|p| p.id() == id)
}

pub fn install_skills(plugin: &dyn Plugin, _config: &Config) -> Result<()> {
    let skills = plugin.agent_skills();
    if skills.is_empty() {
        return Ok(());
    }

    let skill_source = plugin.skill_source_dir();
    if skill_source.is_none() {
        return Ok(());
    }

    let source_base = skill_source.unwrap();
    let target_dir = home_dir().join(".claude").join("commands");

    std::fs::create_dir_all(&target_dir)?;

    let source_dir = find_skill_source_dir(source_base);

    let mut installed = 0;
    let mut skipped = 0;

    for skill in skills {
        let target = target_dir.join(skill);

        if target.exists() {
            tracing::info!(skill = %skill, "Skill already exists, skipping");
            skipped += 1;
            continue;
        }

        if let Some(ref dir) = source_dir {
            let source = dir.join(skill);
            if source.exists() {
                std::fs::copy(&source, &target)?;
                tracing::info!(skill = %skill, "Installed skill");
                installed += 1;
            } else {
                tracing::warn!(source = %source.display(), "Skill source file not found");
            }
        } else {
            tracing::warn!(skill = %skill, "Could not find skill source directory");
        }
    }

    tracing::info!(installed, skipped, "Skill installation complete");
    Ok(())
}

fn find_skill_source_dir(relative_path: &str) -> Option<std::path::PathBuf> {
    let exe_path = std::env::current_exe().ok()?;

    let candidates = [
        exe_path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join(relative_path)),
        exe_path.parent().map(|p| p.join(relative_path)),
        Some(std::path::PathBuf::from(relative_path)),
        std::env::var("CARGO_MANIFEST_DIR")
            .ok()
            .map(|p| std::path::PathBuf::from(p).join(relative_path)),
    ];

    candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate.exists())
}

pub fn uninstall_skills(plugin: &dyn Plugin) -> Result<()> {
    let skills = plugin.agent_skills();
    let target_dir = home_dir().join(".claude").join("commands");

    for skill in skills {
        let target = target_dir.join(skill);
        if target.exists() {
            std::fs::remove_file(&target)?;
            tracing::info!(skill = %skill, "Uninstalled skill");
        }
    }

    Ok(())
}

fn home_dir() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_plugins_contains_chains() {
        let plugins = builtin_plugins();
        assert!(!plugins.is_empty());
        assert!(plugins.iter().any(|p| p.id() == "chains"));
    }

    #[test]
    fn test_get_plugin_chains() {
        let plugin = get_plugin("chains");
        assert!(plugin.is_some());
        let plugin = plugin.unwrap();
        assert_eq!(plugin.id(), "chains");
        assert_eq!(plugin.name(), "Chains");
        assert_eq!(plugin.data_dir(), "chains");
    }

    #[test]
    fn test_get_plugin_unknown() {
        let plugin = get_plugin("nonexistent");
        assert!(plugin.is_none());
    }

    #[test]
    fn test_chains_plugin_skills() {
        let plugin = get_plugin("chains").unwrap();
        let skills = plugin.agent_skills();
        assert!(skills.contains(&"chain-link.md"));
        assert!(skills.contains(&"chain-load.md"));
        assert!(skills.contains(&"chain-list.md"));
    }

    #[test]
    fn test_chains_plugin_enabled_disabled() {
        let mut config = Config::default();

        let plugin = get_plugin("chains").unwrap();
        assert!(!plugin.is_enabled(&config));

        config.plugins.chains.enabled = true;
        assert!(plugin.is_enabled(&config));
    }
}
