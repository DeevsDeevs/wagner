use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PENDING_DISCOVERY: &str = "pending-discovery";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    #[default]
    Managed,
    Attached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    ClaudeCode,
    Codex,
}

impl Engine {
    pub fn resume_command(&self, session_id: &str) -> String {
        match self {
            Engine::ClaudeCode => format!("claude --resume {session_id}"),
            Engine::Codex => "codex".to_string(),
        }
    }

    pub fn process_name(&self) -> &'static str {
        match self {
            Engine::ClaudeCode => "claude",
            Engine::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedPane {
    pub repo_name: String,
    pub engine: Engine,
    pub session_id: String,
    pub pane_id: String,
    pub jsonl_path: PathBuf,
    pub launched_at: DateTime<Utc>,
}

impl TrackedPane {
    pub fn is_discovery_pending(&self) -> bool {
        self.jsonl_path == Path::new(PENDING_DISCOVERY)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub name: String,
    pub path: PathBuf,
    pub repos: Vec<TaskRepo>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub diff_base: Option<String>,
    #[serde(default)]
    pub kind: TaskKind,
    #[serde(default)]
    pub panes: Vec<TrackedPane>,
}

impl Task {
    pub fn new(
        name: impl Into<String>,
        path: PathBuf,
        repos: Vec<TaskRepo>,
        diff_base: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            path,
            repos,
            created_at: Utc::now(),
            diff_base,
            kind: TaskKind::Managed,
            panes: Vec::new(),
        }
    }

    pub fn new_attached(name: impl Into<String>, path: PathBuf, repos: Vec<TaskRepo>) -> Self {
        Self {
            name: name.into(),
            path,
            repos,
            created_at: Utc::now(),
            diff_base: None,
            kind: TaskKind::Attached,
            panes: Vec::new(),
        }
    }

    pub fn is_attached(&self) -> bool {
        matches!(self.kind, TaskKind::Attached)
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
