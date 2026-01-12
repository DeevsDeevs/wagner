use std::time::Duration;

use super::Agent;
use crate::monitor::{AgentDetector, AgentStatus, AgentType};

pub struct TestAgent {
    command: String,
}

impl TestAgent {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
        }
    }

    pub fn echo() -> Self {
        Self::new("echo 'test agent started'")
    }
}

impl Default for TestAgent {
    fn default() -> Self {
        Self::echo()
    }
}

impl Agent for TestAgent {
    fn name(&self) -> &str {
        "test-agent"
    }

    fn launch_command(&self) -> &str {
        &self.command
    }

    fn detector(&self) -> Box<dyn AgentDetector> {
        Box::new(TestAgentDetector)
    }
}

pub struct TestAgentDetector;

impl AgentDetector for TestAgentDetector {
    fn agent_type(&self) -> AgentType {
        AgentType::ClaudeCode
    }

    fn detect_agent(&self, _pane_command: &str, _output: &str) -> bool {
        true
    }

    fn detect_status(
        &self,
        _raw_output: &str,
        _clean_output: &str,
        _output_changed: bool,
        _since_change: Duration,
    ) -> AgentStatus {
        AgentStatus::Idle
    }
}
