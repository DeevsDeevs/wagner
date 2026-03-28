mod control_mode;
mod mock;
mod tmux;

pub use mock::MockTerminal;
pub use tmux::Tmux;

use crate::error::Result;
use std::path::Path;

const SESSION_PREFIX: &str = "wagner_";

pub fn session_name_for_task(task_name: &str) -> String {
    // Use distinct replacement strings for each special character to avoid collisions.
    // E.g., "my.task", "my/task", and "my task" must produce different session names.
    let sanitized = task_name
        .replace('/', "_s_")
        .replace('.', "_d_")
        .replace(' ', "_w_");

    // Strip any remaining characters that aren't tmux-safe [a-zA-Z0-9_-]
    let safe: String = sanitized
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();

    format!("{}{}", SESSION_PREFIX, safe)
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

    fn shell_init_delay(&self) {}

    fn send_approve(&self, pane: &PaneHandle) -> Result<()> {
        self.send_key(pane, "Enter")?;
        Ok(())
    }

    fn send_reject(&self, pane: &PaneHandle) -> Result<()> {
        self.send_key(pane, "Escape")?;
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
            "wagner_2026-01-07_s_hotfixes"
        );
    }

    #[test]
    fn session_name_with_dots() {
        assert_eq!(
            session_name_for_task("feature.v2.0"),
            "wagner_feature_d_v2_d_0"
        );
    }

    #[test]
    fn session_name_with_spaces() {
        assert_eq!(
            session_name_for_task("my task name"),
            "wagner_my_w_task_w_name"
        );
    }

    #[test]
    fn session_name_with_multiple_special_chars() {
        assert_eq!(
            session_name_for_task("2026/01/07 feature.fix"),
            "wagner_2026_s_01_s_07_w_feature_d_fix"
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

    // --- Uniqueness tests ---

    #[test]
    fn session_name_uniqueness_dot_vs_slash_vs_space() {
        let dot = session_name_for_task("my.task");
        let slash = session_name_for_task("my/task");
        let space = session_name_for_task("my task");
        assert_ne!(dot, slash, "dot and slash must differ");
        assert_ne!(dot, space, "dot and space must differ");
        assert_ne!(slash, space, "slash and space must differ");
    }

    #[test]
    fn session_name_tmux_safe_characters() {
        // Verify output only contains [a-zA-Z0-9_-]
        let names = [
            "my.task",
            "my/task",
            "my task",
            "2026/01/07 feature.fix",
            "hello@world#1!",
            "a/b.c d",
        ];
        for name in &names {
            let result = session_name_for_task(name);
            for ch in result.chars() {
                assert!(
                    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-',
                    "non-tmux-safe char '{}' in session name for '{}'",
                    ch,
                    name
                );
            }
        }
    }

    #[test]
    fn session_name_consecutive_special_chars() {
        let result = session_name_for_task("a..b//c  d");
        assert_eq!(result, "wagner_a_d__d_b_s__s_c_w__w_d");
        // Verify the encoding is reversible / unique
        assert_ne!(
            session_name_for_task("a..b"),
            session_name_for_task("a//b")
        );
    }

    #[test]
    fn session_name_leading_trailing_special_chars() {
        let leading = session_name_for_task(".task");
        let trailing = session_name_for_task("task.");
        assert_ne!(leading, trailing, "leading vs trailing dot must differ");
        assert!(leading.starts_with("wagner_"));
        assert!(trailing.starts_with("wagner_"));
    }

    #[test]
    fn session_name_strips_non_tmux_safe_chars() {
        // Characters like @, #, !, etc. should be stripped
        assert_eq!(session_name_for_task("hello@world"), "wagner_helloworld");
        assert_eq!(session_name_for_task("test#1"), "wagner_test1");
    }

    #[test]
    fn session_name_empty_input() {
        assert_eq!(session_name_for_task(""), "wagner_");
    }
}
