use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub task: String,
    pub repo: String,
    pub pane_id: String,
    pub status: SessionStatus,
    pub last_activity: DateTime<Utc>,
}

impl Session {
    pub fn new(task: impl Into<String>, repo: impl Into<String>, pane_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            task: task.into(),
            repo: repo.into(),
            pane_id: pane_id.into(),
            status: SessionStatus::Starting,
            last_activity: Utc::now(),
        }
    }

    pub fn update_status(&mut self, status: SessionStatus) {
        self.status = status;
        self.last_activity = Utc::now();
    }

    pub fn display_name(&self) -> String {
        format!("{}/{}", self.task, self.repo)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Starting,
    Running,
    WaitingForInput,
    Idle,
    Stopped,
    Stale,
    Error(String),
}

impl SessionStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Starting => "◐",
            Self::Running => "●",
            Self::WaitingForInput => "◉",
            Self::Idle => "○",
            Self::Stopped => "■",
            Self::Stale => "◌",
            Self::Error(_) => "✗",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::WaitingForInput => "waiting",
            Self::Idle => "idle",
            Self::Stopped => "stopped",
            Self::Stale => "stale",
            Self::Error(_) => "error",
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}
