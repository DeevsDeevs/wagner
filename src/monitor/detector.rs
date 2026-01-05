use std::time::Duration;

use super::status::{AgentStatus, AgentType};

pub const IDLE_THRESHOLD: Duration = Duration::from_secs(2);

pub trait AgentDetector: Send + Sync {
    fn agent_type(&self) -> AgentType;
    fn detect_agent(&self, pane_command: &str, output: &str) -> bool;
    fn detect_status(
        &self,
        output: &str,
        output_changed: bool,
        since_change: Duration,
    ) -> AgentStatus;
}
