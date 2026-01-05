mod claude;

pub use claude::ClaudeCode;

pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn launch_command(&self) -> &str;
}
