use std::path::{Path, PathBuf};

use super::Agent;
use crate::model::Engine;

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

    fn engine(&self) -> Engine {
        Engine::ClaudeCode
    }

    fn launch_command(&self, _session_id: &str) -> String {
        self.command.clone()
    }

    fn predict_jsonl_path(&self, _session_id: &str, _cwd: &Path) -> Option<PathBuf> {
        None
    }

    fn resume_command(&self, _session_id: &str) -> String {
        self.command.clone()
    }
}
