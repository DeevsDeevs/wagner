use super::{PaneHandle, SessionHandle, Terminal};
use crate::error::{Result, WagnerError};
use std::path::Path;
use std::process::Command;

const SESSION_PREFIX: &str = "wagner_";

pub struct Tmux;

impl Tmux {
    pub fn new() -> Self {
        Self
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("tmux")
            .args(args)
            .output()
            .map_err(|e| WagnerError::Terminal(format!("Failed to run tmux: {}", e)))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(WagnerError::Terminal(format!("tmux error: {}", stderr)))
        }
    }

    fn session_name(&self, name: &str) -> String {
        format!("{}{}", SESSION_PREFIX, name.replace(['/', '.', ' '], "_"))
    }
}

impl Default for Tmux {
    fn default() -> Self {
        Self::new()
    }
}

impl Terminal for Tmux {
    fn create_session(&self, name: &str, cwd: &Path) -> Result<SessionHandle> {
        let session_name = self.session_name(name);
        let cwd_str = cwd.to_string_lossy();

        self.run(&[
            "new-session",
            "-d",
            "-s",
            &session_name,
            "-c",
            &cwd_str,
        ])?;

        Ok(SessionHandle(session_name))
    }

    fn create_pane(&self, session: &SessionHandle, cwd: &Path) -> Result<PaneHandle> {
        let cwd_str = cwd.to_string_lossy();

        let pane_id = self.run(&[
            "split-window",
            "-t",
            &session.0,
            "-c",
            &cwd_str,
            "-P",
            "-F",
            "#{pane_id}",
        ])?;

        Ok(PaneHandle(pane_id))
    }

    fn capture(&self, pane: &PaneHandle, lines: usize) -> Result<String> {
        self.run(&[
            "capture-pane",
            "-t",
            &pane.0,
            "-p",
            "-e",
            "-S",
            &format!("-{}", lines),
        ])
    }

    fn send_keys(&self, pane: &PaneHandle, keys: &str) -> Result<()> {
        self.run(&["send-keys", "-t", &pane.0, "-l", keys])?;
        self.run(&["send-keys", "-t", &pane.0, "Enter"])?;
        Ok(())
    }

    fn attach(&self, session: &SessionHandle) -> Result<()> {
        let status = Command::new("tmux")
            .args(["attach-session", "-t", &session.0])
            .status()
            .map_err(|e| WagnerError::Terminal(format!("Failed to attach: {}", e)))?;

        if status.success() {
            Ok(())
        } else {
            Err(WagnerError::Terminal("Failed to attach to session".into()))
        }
    }

    fn list_panes(&self, session: &SessionHandle) -> Result<Vec<PaneHandle>> {
        let output = self.run(&[
            "list-panes",
            "-s",
            "-t",
            &session.0,
            "-F",
            "#{pane_id}",
        ])?;

        Ok(output
            .lines()
            .filter(|s| !s.is_empty())
            .map(|s| PaneHandle(s.to_string()))
            .collect())
    }

    fn kill_pane(&self, pane: &PaneHandle) -> Result<()> {
        self.run(&["kill-pane", "-t", &pane.0])?;
        Ok(())
    }

    fn kill_session(&self, session: &SessionHandle) -> Result<()> {
        self.run(&["kill-session", "-t", &session.0])?;
        Ok(())
    }

    fn session_exists(&self, name: &str) -> Result<bool> {
        let session_name = self.session_name(name);
        let result = self.run(&["has-session", "-t", &session_name]);
        Ok(result.is_ok())
    }
}
