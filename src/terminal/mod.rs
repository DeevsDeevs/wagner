mod control_mode;
mod mock;
mod tmux;

pub use mock::MockTerminal;
pub use tmux::Tmux;

use crate::error::Result;
use std::path::Path;

const SESSION_PREFIX: &str = "wagner_";

pub fn session_name_for_task(task_name: &str) -> String {
    format!(
        "{}{}",
        SESSION_PREFIX,
        task_name.replace(['/', '.', ' '], "_")
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionHandle(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PaneHandle(pub String, pub String); // (pane_id, title)

pub trait Terminal: Send + Sync {
    fn create_session(&self, name: &str, cwd: &Path) -> Result<SessionHandle>;
    fn create_pane(&self, session: &SessionHandle, cwd: &Path) -> Result<PaneHandle>;
    fn capture(&self, pane: &PaneHandle, lines: usize) -> Result<String>;
    fn send_keys(&self, pane: &PaneHandle, keys: &str) -> Result<()>;
    fn send_key(&self, pane: &PaneHandle, key: &str) -> Result<()>;
    fn send_literal(&self, pane: &PaneHandle, text: &str) -> Result<()>;
    fn attach(&self, session: &SessionHandle) -> Result<()>;
    fn select_pane(&self, pane: &PaneHandle) -> Result<()>;
    fn list_panes(&self, session: &SessionHandle) -> Result<Vec<PaneHandle>>;
    fn kill_pane(&self, pane: &PaneHandle) -> Result<()>;
    fn kill_session(&self, session: &SessionHandle) -> Result<()>;
    fn session_exists(&self, name: &str) -> Result<bool>;
    fn get_pane_command(&self, pane: &PaneHandle) -> Result<String>;
    fn resize_pane(&self, pane: &PaneHandle, width: u16, height: u16) -> Result<()>;

    fn send_confirm(&self, pane: &PaneHandle, response: &str) -> Result<()> {
        self.send_key(pane, response)?;
        self.send_key(pane, "Enter")?;
        Ok(())
    }

    fn send_text_enter(&self, pane: &PaneHandle, text: &str, delay_ms: u64) -> Result<()> {
        self.send_literal(pane, text)?;
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        self.send_key(pane, "Enter")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_simple() {
        assert_eq!(session_name_for_task("my-task"), "wagner_my-task");
    }

    #[test]
    fn session_name_with_slashes() {
        assert_eq!(
            session_name_for_task("2026-01-07/hotfixes"),
            "wagner_2026-01-07_hotfixes"
        );
    }

    #[test]
    fn session_name_with_dots() {
        assert_eq!(session_name_for_task("feature.v2.0"), "wagner_feature_v2_0");
    }

    #[test]
    fn session_name_with_spaces() {
        assert_eq!(session_name_for_task("my task name"), "wagner_my_task_name");
    }

    #[test]
    fn session_name_with_multiple_special_chars() {
        assert_eq!(
            session_name_for_task("2026/01/07 feature.fix"),
            "wagner_2026_01_07_feature_fix"
        );
    }

    #[test]
    fn session_name_preserves_underscores() {
        assert_eq!(session_name_for_task("my_task_name"), "wagner_my_task_name");
    }

    #[test]
    fn session_name_preserves_hyphens() {
        assert_eq!(session_name_for_task("my-task-name"), "wagner_my-task-name");
    }
}
