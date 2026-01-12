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
}
