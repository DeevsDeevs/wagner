mod detector;

pub use detector::ClaudeCodeDetector;

use super::Agent;
use crate::monitor::AgentDetector;

pub struct ClaudeCode;

impl ClaudeCode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeCode {
    fn default() -> Self {
        Self::new()
    }
}

impl Agent for ClaudeCode {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn launch_command(&self) -> &str {
        "claude"
    }

    fn detector(&self) -> Box<dyn AgentDetector> {
        Box::new(ClaudeCodeDetector::default())
    }
}
