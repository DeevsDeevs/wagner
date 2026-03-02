pub mod data;
pub mod parser;
pub mod state;

use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;
use crate::plugins::{Plugin, PluginItem, PluginItemDetail, PluginProvider};

pub use data::{Chain, ChainLink, ChainSource, ChainsData, RepoChains};
pub use parser::load_all_chains;
pub use state::{ChainsState, ChainsViewMode};

pub struct ChainsPlugin;

impl Plugin for ChainsPlugin {
    fn id(&self) -> &'static str {
        "chains"
    }

    fn name(&self) -> &'static str {
        "Chains"
    }

    fn description(&self) -> &'static str {
        "Multi-session workflow chains for maintaining context across conversations"
    }

    fn data_dir(&self) -> &'static str {
        "chains"
    }

    fn is_enabled(&self, config: &Config) -> bool {
        config.plugins.chains.enabled
    }

    fn agent_skills(&self) -> &[&'static str] {
        &["chain-link.md", "chain-load.md", "chain-list.md"]
    }

    fn skill_source_dir(&self) -> Option<&'static str> {
        Some("plugins/chains/skills/claude/commands")
    }
}

pub struct ChainsProvider;

impl PluginProvider for ChainsProvider {
    fn id(&self) -> &str {
        "chains"
    }

    fn name(&self) -> &str {
        "Chains"
    }

    fn is_enabled(&self, config: &Config) -> bool {
        config.plugins.chains.enabled
    }

    fn list_items(
        &self,
        tasks_root: &Path,
        task_name: Option<&str>,
    ) -> crate::Result<Vec<PluginItem>> {
        let data = load_all_chains(tasks_root, task_name)?;
        let mut items = Vec::new();

        for chain in data.all_chains() {
            let latest = chain.latest_link();
            let summary = latest.and_then(|l| l.summary.clone()).unwrap_or_default();

            let mut metadata = HashMap::new();
            metadata.insert("link_count".into(), chain.link_count().to_string());
            if let Some(link) = latest {
                metadata.insert("latest_timestamp".into(), link.timestamp.clone());
                if let Some(next) = &link.next_step {
                    metadata.insert("next_step".into(), next.clone());
                }
            }
            match &chain.source {
                ChainSource::Repo(p) => {
                    metadata.insert("source".into(), "repo".into());
                    metadata.insert("source_path".into(), p.display().to_string());
                }
                ChainSource::TaskLocal(p) => {
                    metadata.insert("source".into(), "task_local".into());
                    metadata.insert("source_path".into(), p.display().to_string());
                }
            }

            items.push(PluginItem {
                id: chain.name.clone(),
                name: chain.name.clone(),
                summary,
                metadata,
            });
        }

        Ok(items)
    }

    fn get_item(
        &self,
        tasks_root: &Path,
        task_name: Option<&str>,
        item_id: &str,
    ) -> crate::Result<Option<PluginItemDetail>> {
        let data = load_all_chains(tasks_root, task_name)?;

        for chain in data.all_chains() {
            if chain.name == item_id {
                let latest = chain.latest_link();
                let content = if let Some(link) = latest {
                    std::fs::read_to_string(&link.file_path).unwrap_or_default()
                } else {
                    String::new()
                };

                let summary = latest.and_then(|l| l.summary.clone()).unwrap_or_default();

                return Ok(Some(PluginItemDetail {
                    item: PluginItem {
                        id: chain.name.clone(),
                        name: chain.name.clone(),
                        summary,
                        metadata: HashMap::new(),
                    },
                    content,
                }));
            }
        }

        Ok(None)
    }
}
