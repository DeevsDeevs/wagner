mod claude;

pub use claude::{ClaudeCode, ClaudeCodeDetector};

use crate::monitor::AgentDetector;

pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn launch_command(&self) -> &str;
    fn detector(&self) -> Box<dyn AgentDetector>;
}
