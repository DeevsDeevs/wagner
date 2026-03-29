use super::Agent;
use crate::model::Engine;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
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

fn cwd_to_project_id(cwd: &Path) -> String {
    cwd.to_string_lossy().replace(['/', '.'], "-")
}

impl Agent for ClaudeCode {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn engine(&self) -> Engine {
        Engine::ClaudeCode
    }

    fn launch_command(&self, session_id: &str) -> String {
        format!("claude --session-id {session_id}")
    }

    fn predict_jsonl_path(&self, session_id: &str, cwd: &Path) -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let project_id = cwd_to_project_id(cwd);
        Some(
            PathBuf::from(home)
                .join(".claude")
                .join("projects")
                .join(project_id)
                .join(format!("{session_id}.jsonl")),
        )
    }

    fn resume_command(&self, session_id: &str) -> String {
        format!("claude --resume {session_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_to_project_id_basic() {
        let cwd = Path::new("/Users/foo/programming/agents");
        assert_eq!(cwd_to_project_id(cwd), "-Users-foo-programming-agents");
    }

    #[test]
    fn cwd_to_project_id_dotfiles() {
        let cwd = Path::new("/Users/foo/.ssh");
        assert_eq!(cwd_to_project_id(cwd), "-Users-foo--ssh");
    }

    #[test]
    fn cwd_to_project_id_dots_in_path() {
        let cwd = Path::new("/Users/foo/.local/share/chezmoi");
        assert_eq!(cwd_to_project_id(cwd), "-Users-foo--local-share-chezmoi");
    }

    #[test]
    fn predict_jsonl_path_structure() {
        let agent = ClaudeCode::new();
        let cwd = Path::new("/Users/foo/programming/agents");
        let session_id = "abc-123";
        let path = agent.predict_jsonl_path(session_id, cwd).unwrap();
        assert!(path.to_string_lossy().contains(".claude/projects/"));
        assert!(path.to_string_lossy().contains("abc-123.jsonl"));
        assert!(
            path.to_string_lossy()
                .contains("-Users-foo-programming-agents")
        );
    }

    #[test]
    fn launch_command_includes_session_id() {
        let agent = ClaudeCode::new();
        assert_eq!(
            agent.launch_command("my-uuid"),
            "claude --session-id my-uuid"
        );
    }

    #[test]
    fn resume_command_includes_session_id() {
        let agent = ClaudeCode::new();
        assert_eq!(agent.resume_command("my-uuid"), "claude --resume my-uuid");
    }
}
