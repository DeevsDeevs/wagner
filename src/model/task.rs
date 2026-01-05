use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub name: String,
    pub path: PathBuf,
    pub repos: Vec<TaskRepo>,
    pub created_at: DateTime<Utc>,
}

impl Task {
    pub fn new(name: impl Into<String>, path: PathBuf, repos: Vec<TaskRepo>) -> Self {
        Self {
            name: name.into(),
            path,
            repos,
            created_at: Utc::now(),
        }
    }

    pub fn metadata_dir(&self) -> PathBuf {
        self.path.join(".wagner")
    }

    pub fn metadata_path(&self) -> PathBuf {
        self.metadata_dir().join("task.json")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRepo {
    pub name: String,
    pub source: RepoSource,
    pub worktree: PathBuf,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RepoSource {
    Local(PathBuf),
    Remote(String),
}

impl RepoSource {
    pub fn parse(s: &str) -> Self {
        if s.starts_with("git@") || s.starts_with("https://") || s.starts_with("git://") {
            Self::Remote(s.to_string())
        } else {
            let expanded = shellexpand::tilde(s);
            Self::Local(PathBuf::from(expanded.as_ref()))
        }
    }
}

impl std::fmt::Display for RepoSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(path) => write!(f, "{}", path.display()),
            Self::Remote(url) => write!(f, "{}", url),
        }
    }
}
