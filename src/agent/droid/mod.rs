use super::Agent;
use crate::model::Engine;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct Droid;

impl Droid {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Droid {
    fn default() -> Self {
        Self::new()
    }
}

fn cwd_to_project_id(cwd: &Path) -> String {
    cwd.to_string_lossy().replace('/', "-")
}

impl Agent for Droid {
    fn name(&self) -> &str {
        "droid"
    }

    fn engine(&self) -> Engine {
        Engine::Droid
    }

    fn launch_command(&self, _session_id: &str) -> String {
        "droid".to_string()
    }

    fn predict_jsonl_path(&self, session_id: &str, cwd: &Path) -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let project_id = cwd_to_project_id(cwd);
        Some(
            PathBuf::from(home)
                .join(".factory")
                .join("sessions")
                .join(project_id)
                .join(format!("{session_id}.jsonl")),
        )
    }

    fn resume_command(&self, session_id: &str) -> String {
        format!("droid --resume {session_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_to_project_id_basic() {
        let cwd = Path::new("/Users/foo/project");
        assert_eq!(cwd_to_project_id(cwd), "-Users-foo-project");
    }

    #[test]
    fn cwd_to_project_id_deep_path() {
        let cwd = Path::new("/Users/deevs/programming/agents/wagner");
        assert_eq!(
            cwd_to_project_id(cwd),
            "-Users-deevs-programming-agents-wagner"
        );
    }

    #[test]
    fn cwd_to_project_id_root() {
        let cwd = Path::new("/");
        assert_eq!(cwd_to_project_id(cwd), "-");
    }

    #[test]
    fn predict_jsonl_path_structure() {
        let agent = Droid::new();
        let cwd = Path::new("/Users/foo/project");
        let session_id = "abc-123";
        let path = agent.predict_jsonl_path(session_id, cwd).unwrap();
        let path_str = path.to_string_lossy();
        assert!(path_str.contains(".factory/sessions/"));
        assert!(path_str.contains("-Users-foo-project"));
        assert!(path_str.ends_with("abc-123.jsonl"));
    }

    #[test]
    fn predict_jsonl_path_full_structure() {
        let agent = Droid::new();
        let cwd = Path::new("/Users/deevs/programming/agents");
        let session_id = "ses-456";
        let path = agent.predict_jsonl_path(session_id, cwd).unwrap();
        let path_str = path.to_string_lossy();
        // Should be: {HOME}/.factory/sessions/-Users-deevs-programming-agents/ses-456.jsonl
        assert!(path_str.contains(".factory/sessions/"));
        assert!(path_str.contains("-Users-deevs-programming-agents"));
        assert!(path_str.ends_with("ses-456.jsonl"));
    }

    #[test]
    fn name_returns_droid() {
        let agent = Droid::new();
        assert_eq!(agent.name(), "droid");
    }

    #[test]
    fn engine_returns_droid() {
        let agent = Droid::new();
        assert_eq!(agent.engine(), Engine::Droid);
    }

    #[test]
    fn launch_command_droid() {
        let agent = Droid::new();
        assert_eq!(agent.launch_command("my-uuid"), "droid");
    }

    #[test]
    fn resume_command_droid() {
        let agent = Droid::new();
        assert_eq!(agent.resume_command("my-uuid"), "droid --resume my-uuid");
    }
}
