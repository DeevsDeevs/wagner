mod detector;

pub use detector::CodexDetector;

use super::Agent;
use crate::model::Engine;
use crate::monitor::AgentDetector;
use std::path::{Path, PathBuf};

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

    fn engine(&self) -> Engine {
        Engine::Codex
    }

    fn launch_command(&self, _session_id: &str) -> String {
        "codex".to_string()
    }

    fn predict_jsonl_path(&self, _session_id: &str, _cwd: &Path) -> Option<PathBuf> {
        None
    }

    fn resume_command(&self, _session_id: &str) -> String {
        "codex".to_string()
    }

    fn detector(&self) -> Box<dyn AgentDetector> {
        Box::new(CodexDetector::default())
    }
}
