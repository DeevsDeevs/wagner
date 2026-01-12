use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Chain {
    pub name: String,
    pub links: Vec<ChainLink>,
    pub source: ChainSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainSource {
    Repo(PathBuf),
    TaskLocal(PathBuf),
}

#[derive(Debug, Clone)]
pub struct ChainLink {
    pub timestamp: String,
    pub slug: String,
    pub file_path: PathBuf,
    pub summary: Option<String>,
    pub next_step: Option<String>,
}

impl Chain {
    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    pub fn latest_link(&self) -> Option<&ChainLink> {
        self.links.last()
    }

    pub fn is_repo_level(&self) -> bool {
        matches!(self.source, ChainSource::Repo(_))
    }
}

#[derive(Debug, Clone)]
pub struct RepoChains {
    pub repo_name: String,
    pub repo_path: PathBuf,
    pub chains: Vec<Chain>,
}

#[derive(Debug, Clone, Default)]
pub struct ChainsData {
    pub repos: Vec<RepoChains>,
    pub task_local: Vec<Chain>,
}

impl ChainsData {
    pub fn total_chains(&self) -> usize {
        self.repos.iter().map(|r| r.chains.len()).sum::<usize>() + self.task_local.len()
    }

    pub fn all_chains(&self) -> impl Iterator<Item = &Chain> {
        self.repos
            .iter()
            .flat_map(|r| r.chains.iter())
            .chain(self.task_local.iter())
    }

    /// Returns chains grouped by task name in alphabetical order.
    /// This matches the display order in the sidebar.
    pub fn chains_in_display_order(&self) -> Vec<&Chain> {
        use std::collections::BTreeMap;

        let mut groups: BTreeMap<String, Vec<&Chain>> = BTreeMap::new();

        for repo in &self.repos {
            for chain in &repo.chains {
                let task_name = extract_task_name(&chain.name)
                    .unwrap_or_else(|| repo.repo_name.clone());
                groups.entry(task_name).or_default().push(chain);
            }
        }

        for chain in &self.task_local {
            let task_name = extract_task_name(&chain.name)
                .unwrap_or_else(|| "local".to_string());
            groups.entry(task_name).or_default().push(chain);
        }

        groups.into_values().flatten().collect()
    }

    pub fn get_chain_at_display_index(&self, idx: usize) -> Option<&Chain> {
        self.chains_in_display_order().into_iter().nth(idx)
    }
}

fn extract_task_name(chain_name: &str) -> Option<String> {
    let parts: Vec<&str> = chain_name.split('/').collect();
    if parts.len() >= 2 {
        Some(parts[0].to_string())
    } else {
        None
    }
}
