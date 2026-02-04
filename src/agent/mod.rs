mod claude;
mod codex;
mod test;

pub use claude::{ClaudeCode, ClaudeCodeDetector};
pub use codex::{Codex, CodexDetector};
pub use test::TestAgent;

use crate::error::{Result, WagnerError};
use crate::monitor::AgentDetector;

pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn launch_command(&self) -> &str;
    fn detector(&self) -> Box<dyn AgentDetector>;
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

    fn launch_command(&self) -> &str {
        match self {
            Self::Claude(agent) => agent.launch_command(),
            Self::Codex(agent) => agent.launch_command(),
        }
    }

    fn detector(&self) -> Box<dyn AgentDetector> {
        match self {
            Self::Claude(agent) => agent.detector(),
            Self::Codex(agent) => agent.detector(),
        }
    }
}
