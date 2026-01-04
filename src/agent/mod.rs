mod claude;

pub use claude::ClaudeCode;

use crate::error::Result;
use crate::model::SessionStatus;
use std::path::Path;

pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn launch_command(&self) -> &str;
    fn setup_hooks(&self, worktree: &Path) -> Result<()>;
    fn parse_hook_event(&self, event: &str) -> Option<SessionStatus>;
    fn detect_status(&self, output: &str) -> SessionStatus;
}
