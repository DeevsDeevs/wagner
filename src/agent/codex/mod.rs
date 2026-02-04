mod detector;

pub use detector::CodexDetector;

use super::Agent;
use crate::monitor::AgentDetector;

#[derive(Debug, Clone, Copy)]
pub struct Codex;

impl Codex {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Codex {
    fn default() -> Self {
        Self::new()
    }
}

impl Agent for Codex {
    fn name(&self) -> &str {
        "codex"
    }

    fn launch_command(&self) -> &str {
        "codex"
    }

    fn detector(&self) -> Box<dyn AgentDetector> {
        Box::new(CodexDetector::default())
    }
}
