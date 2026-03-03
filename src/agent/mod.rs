mod claude;
mod codex;
mod test;

pub use claude::ClaudeCode;
pub use codex::Codex;
pub use test::TestAgent;

use crate::error::{Result, WagnerError};
use crate::model::Engine;
use std::path::{Path, PathBuf};

pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn engine(&self) -> Engine;
    fn launch_command(&self, session_id: &str) -> String;
    fn predict_jsonl_path(&self, session_id: &str, cwd: &Path) -> Option<PathBuf>;
    fn resume_command(&self, session_id: &str) -> String;
}

#[derive(Debug, Clone)]
pub enum AgentChoice {
    Claude(ClaudeCode),
    Codex(Codex),
}

impl AgentChoice {
    pub fn from_key(key: &str) -> Result<Self> {
        match key {
            "claude" | "claude-code" => Ok(Self::Claude(ClaudeCode::new())),
            "codex" => Ok(Self::Codex(Codex::new())),
            _ => Err(WagnerError::InvalidAgent(key.to_string())),
        }
    }
}

impl Agent for AgentChoice {
    fn name(&self) -> &str {
        match self {
            Self::Claude(agent) => agent.name(),
            Self::Codex(agent) => agent.name(),
        }
    }

    fn engine(&self) -> Engine {
        match self {
            Self::Claude(agent) => agent.engine(),
            Self::Codex(agent) => agent.engine(),
        }
    }

    fn launch_command(&self, session_id: &str) -> String {
        match self {
            Self::Claude(agent) => agent.launch_command(session_id),
            Self::Codex(agent) => agent.launch_command(session_id),
        }
    }

    fn predict_jsonl_path(&self, session_id: &str, cwd: &Path) -> Option<PathBuf> {
        match self {
            Self::Claude(agent) => agent.predict_jsonl_path(session_id, cwd),
            Self::Codex(agent) => agent.predict_jsonl_path(session_id, cwd),
        }
    }

    fn resume_command(&self, session_id: &str) -> String {
        match self {
            Self::Claude(agent) => agent.resume_command(session_id),
            Self::Codex(agent) => agent.resume_command(session_id),
        }
    }
}
