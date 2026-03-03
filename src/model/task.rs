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
    Terminal,
}

impl Engine {
    pub fn resume_command(&self, session_id: &str) -> String {
        match self {
            Engine::ClaudeCode => format!("claude --resume {session_id}"),
            Engine::Codex => "codex".to_string(),
            Engine::Terminal => String::new(),
        }
    }

    pub fn launch_command(&self, session_id: &str) -> String {
        match self {
            Engine::ClaudeCode => format!("claude --session-id {session_id}"),
            Engine::Codex => "codex".to_string(),
            Engine::Terminal => String::new(),
        }
    }

    pub fn process_name(&self) -> &'static str {
        match self {
            Engine::ClaudeCode => "claude",
            Engine::Codex => "codex",
            Engine::Terminal => "",
        }
    }

    pub fn enter_delay_ms(&self) -> u64 {
        match self {
            Engine::ClaudeCode => 5,
            Engine::Codex => 100,
            Engine::Terminal => 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedPane {
    #[serde(default)]
    pub name: String,
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

    pub fn next_pane_name(&self, base: &str) -> String {
        if !self.panes.iter().any(|p| p.name == base) {
            return base.to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base}-{n}");
            if !self.panes.iter().any(|p| p.name == candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    pub fn find_pane_by_name(&self, name: &str) -> Option<&TrackedPane> {
        self.panes.iter().find(|p| p.name == name)
    }

    pub fn fixup_pane_names(&mut self) {
        let needs_fixup: Vec<usize> = self
            .panes
            .iter()
            .enumerate()
            .filter(|(_, p)| p.name.is_empty())
            .map(|(i, _)| i)
            .collect();

        for idx in needs_fixup {
            let base = self.panes[idx].repo_name.clone();
            let name = self.next_pane_name(&base);
            self.panes[idx].name = name;
        }
    }

    pub fn rename_pane(&mut self, old: &str, new: &str) -> bool {
        if self.panes.iter().any(|p| p.name == new) {
            return false;
        }
        if let Some(pane) = self.panes.iter_mut().find(|p| p.name == old) {
            pane.name = new.to_string();
            true
        } else {
            false
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pane(name: &str, repo: &str) -> TrackedPane {
        TrackedPane {
            name: name.into(),
            repo_name: repo.into(),
            engine: Engine::ClaudeCode,
            session_id: "s1".into(),
            pane_id: "%1".into(),
            jsonl_path: PathBuf::from("pending-discovery"),
            launched_at: Utc::now(),
        }
    }

    fn make_task() -> Task {
        Task::new("test", PathBuf::from("/tmp/test"), vec![], None)
    }

    #[test]
    fn next_pane_name_no_conflict() {
        let task = make_task();
        assert_eq!(task.next_pane_name("wagner"), "wagner");
    }

    #[test]
    fn next_pane_name_with_conflict() {
        let mut task = make_task();
        task.panes.push(make_pane("wagner", "wagner"));
        assert_eq!(task.next_pane_name("wagner"), "wagner-2");

        task.panes.push(make_pane("wagner-2", "wagner"));
        assert_eq!(task.next_pane_name("wagner"), "wagner-3");
    }

    #[test]
    fn find_pane_by_name_found() {
        let mut task = make_task();
        task.panes.push(make_pane("api", "api"));
        assert!(task.find_pane_by_name("api").is_some());
    }

    #[test]
    fn find_pane_by_name_not_found() {
        let task = make_task();
        assert!(task.find_pane_by_name("api").is_none());
    }

    #[test]
    fn fixup_pane_names_fills_empty() {
        let mut task = make_task();
        task.panes.push(make_pane("", "api"));
        task.panes.push(make_pane("", "web"));
        task.fixup_pane_names();
        assert_eq!(task.panes[0].name, "api");
        assert_eq!(task.panes[1].name, "web");
    }

    #[test]
    fn fixup_pane_names_handles_duplicates() {
        let mut task = make_task();
        task.panes.push(make_pane("", "api"));
        task.panes.push(make_pane("", "api"));
        task.fixup_pane_names();
        assert_eq!(task.panes[0].name, "api");
        assert_eq!(task.panes[1].name, "api-2");
    }

    #[test]
    fn fixup_pane_names_skips_already_named() {
        let mut task = make_task();
        task.panes.push(make_pane("custom", "api"));
        task.panes.push(make_pane("", "web"));
        task.fixup_pane_names();
        assert_eq!(task.panes[0].name, "custom");
        assert_eq!(task.panes[1].name, "web");
    }

    #[test]
    fn rename_pane_success() {
        let mut task = make_task();
        task.panes.push(make_pane("api", "api"));
        assert!(task.rename_pane("api", "backend"));
        assert_eq!(task.panes[0].name, "backend");
    }

    #[test]
    fn rename_pane_target_exists() {
        let mut task = make_task();
        task.panes.push(make_pane("api", "api"));
        task.panes.push(make_pane("web", "web"));
        assert!(!task.rename_pane("api", "web"));
        assert_eq!(task.panes[0].name, "api");
    }

    #[test]
    fn rename_pane_source_not_found() {
        let mut task = make_task();
        task.panes.push(make_pane("api", "api"));
        assert!(!task.rename_pane("missing", "new-name"));
    }

    #[test]
    fn engine_launch_command() {
        assert_eq!(
            Engine::ClaudeCode.launch_command("my-uuid"),
            "claude --session-id my-uuid"
        );
        assert_eq!(Engine::Codex.launch_command("my-uuid"), "codex");
    }

    #[test]
    fn tracked_pane_serde_roundtrip() {
        let pane = make_pane("api", "api");
        let json = serde_json::to_string(&pane).unwrap();
        let deserialized: TrackedPane = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "api");
        assert_eq!(deserialized.repo_name, "api");
    }

    #[test]
    fn tracked_pane_backward_compat_no_name() {
        let json = r#"{
            "repo_name": "api",
            "engine": "claude_code",
            "session_id": "s1",
            "pane_id": "%1",
            "jsonl_path": "pending-discovery",
            "launched_at": "2026-01-01T00:00:00Z"
        }"#;
        let pane: TrackedPane = serde_json::from_str(json).unwrap();
        assert_eq!(pane.name, "");
        assert_eq!(pane.repo_name, "api");
    }
}
