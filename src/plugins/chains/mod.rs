pub mod data;
pub mod parser;
pub mod state;

use crate::config::Config;
use crate::plugins::Plugin;

pub use data::{Chain, ChainLink, ChainSource, ChainsData, RepoChains};
pub use state::{ChainsState, ChainsViewMode};
pub use parser::load_all_chains;

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
