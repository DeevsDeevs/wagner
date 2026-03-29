mod control_mode;
mod mock;
mod tmux;

pub use mock::MockTerminal;
pub use tmux::Tmux;

use crate::error::Result;
use std::path::Path;

const SESSION_PREFIX: &str = "wagner_";

pub fn session_name_for_task(task_name: &str) -> String {
    // Hex-encode every character that isn't in the tmux-safe set [a-zA-Z0-9_-].
    // Each unsafe character is replaced with _XX_ where XX is the lowercase hex
    // value of the byte. This encoding is injective: distinct inputs always
    // produce distinct outputs because the hex values uniquely represent the
    // original characters, and literal underscores in the input are also
    // hex-encoded (_5f_) so there's no ambiguity.
    let mut sanitized = String::with_capacity(task_name.len() * 2);
    for b in task_name.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' {
            sanitized.push(b as char);
        } else {
            // Encode as _XX_ (lowercase hex)
            sanitized.push_str(&format!("_{:02x}_", b));
        }
    }
    format!("{}{}", SESSION_PREFIX, sanitized)
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
        // '/' = 0x2f -> _2f_
        assert_eq!(
            session_name_for_task("2026-01-07/hotfixes"),
            "wagner_2026-01-07_2f_hotfixes"
        );
    }

    #[test]
    fn session_name_with_dots() {
        // '.' = 0x2e -> _2e_
        assert_eq!(
            session_name_for_task("feature.v2.0"),
            "wagner_feature_2e_v2_2e_0"
        );
    }

    #[test]
    fn session_name_with_spaces() {
        // ' ' = 0x20 -> _20_
        assert_eq!(
            session_name_for_task("my task name"),
            "wagner_my_20_task_20_name"
        );
    }

    #[test]
    fn session_name_with_multiple_special_chars() {
        assert_eq!(
            session_name_for_task("2026/01/07 feature.fix"),
            "wagner_2026_2f_01_2f_07_20_feature_2e_fix"
        );
    }

    #[test]
    fn session_name_preserves_underscores_via_hex() {
        // '_' = 0x5f -> _5f_ (hex-encoded to maintain injectivity)
        assert_eq!(
            session_name_for_task("my_task_name"),
            "wagner_my_5f_task_5f_name"
        );
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
        // '.' = _2e_, '/' = _2f_, ' ' = _20_
        let result = session_name_for_task("a..b//c  d");
        assert_eq!(result, "wagner_a_2e__2e_b_2f__2f_c_20__20_d");
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
    fn session_name_hex_encodes_non_safe_chars() {
        // Characters like @, #, !, etc. are hex-encoded, not stripped
        // '@' = 0x40, '#' = 0x23, '!' = 0x21
        assert_eq!(
            session_name_for_task("hello@world"),
            "wagner_hello_40_world"
        );
        assert_eq!(session_name_for_task("test#1"), "wagner_test_23_1");
    }

    #[test]
    fn session_name_empty_input() {
        assert_eq!(session_name_for_task(""), "wagner_");
    }

    #[test]
    fn session_name_injective_underscore_vs_encoded() {
        // Ensure that a literal underscore in the input is hex-encoded,
        // so "a_b" and something that could produce "a_b" via encoding
        // don't collide.
        let with_underscore = session_name_for_task("a_b");
        let with_slash = session_name_for_task("a/b");
        assert_ne!(
            with_underscore, with_slash,
            "underscore and slash must produce different names"
        );
        // "a_b" -> "wagner_a_5f_b" (underscore is encoded)
        assert_eq!(with_underscore, "wagner_a_5f_b");
        // "a/b" -> "wagner_a_2f_b"
        assert_eq!(with_slash, "wagner_a_2f_b");
    }
}
