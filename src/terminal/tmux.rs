use super::control_mode::TmuxControlMode;
use super::{PaneHandle, SessionHandle, Terminal, session_name_for_task};
use crate::config::TerminalConfig;
use crate::error::{Result, WagnerError};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{debug, warn};

pub fn quote_arg_for_control_mode(arg: &str) -> String {
    let needs_quoting = arg.contains(char::is_whitespace)
        || arg.contains('"')
        || arg.contains('#')
        || arg.contains('{')
        || arg.contains('}')
        || arg.contains('$')
        || arg.contains(';')
        || arg.contains('\'')
        || arg.contains('`')
        || arg.contains('\\');
    if needs_quoting {
        format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

pub fn build_control_mode_command(args: &[&str]) -> String {
    args.iter()
        .map(|arg| quote_arg_for_control_mode(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct Tmux {
    control_mode: Mutex<Option<Arc<TmuxControlMode>>>,
    config: TerminalConfig,
}

impl Tmux {
    pub fn new() -> Self {
        Self::with_config(TerminalConfig::default())
    }

    pub fn with_config(config: TerminalConfig) -> Self {
        Self {
            control_mode: Mutex::new(None),
            config,
        }
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let start = Instant::now();
        let cmd_str = args.join(" ");

        let result = if self.config.use_control_mode {
            if let Some(result) = self.try_control_mode(args) {
                let elapsed = start.elapsed();
                if elapsed.as_millis() > 50 {
                    warn!(cmd = %cmd_str, elapsed_ms = %elapsed.as_millis(), "slow control_mode");
                } else {
                    debug!(cmd = %cmd_str, elapsed_ms = %elapsed.as_millis(), "control_mode");
                }
                return result;
            }
            self.run_spawn(args)
        } else {
            self.run_spawn(args)
        };

        let elapsed = start.elapsed();
        if elapsed.as_millis() > 50 {
            warn!(cmd = %cmd_str, elapsed_ms = %elapsed.as_millis(), "slow spawn");
        } else {
            debug!(cmd = %cmd_str, elapsed_ms = %elapsed.as_millis(), "spawn");
        }
        result
    }

    fn try_control_mode(&self, args: &[&str]) -> Option<Result<String>> {
        let cm = {
            let lock_start = Instant::now();
            let mut cm_guard = self.control_mode.lock().ok()?;
            let lock_elapsed = lock_start.elapsed();
            if lock_elapsed.as_millis() > 10 {
                warn!(elapsed_ms = %lock_elapsed.as_millis(), "slow mutex lock");
            }

            if cm_guard.is_none() {
                let connect_start = Instant::now();
                match TmuxControlMode::connect_with_timeout(self.config.control_mode_timeout_ms) {
                    Ok(cm) => {
                        let elapsed = connect_start.elapsed();
                        if elapsed.as_millis() > 100 {
                            warn!(elapsed_ms = %elapsed.as_millis(), "slow control_mode connect");
                        }
                        *cm_guard = Some(Arc::new(cm));
                    }
                    Err(e) => {
                        warn!(error = %e, "control_mode connect failed");
                        return None;
                    }
                }
            }

            let cm = cm_guard.as_ref()?;
            if !cm.is_alive() {
                warn!("control_mode not alive, clearing");
                *cm_guard = None;
                return None;
            }

            Arc::clone(cm)
        };

        let command = build_control_mode_command(args);
        let exec_start = Instant::now();
        let result = cm.execute(&command);
        let exec_elapsed = exec_start.elapsed();
        if exec_elapsed.as_millis() > 50 {
            warn!(cmd = %command, elapsed_ms = %exec_elapsed.as_millis(), "slow execute");
        }

        match result {
            Ok(output) => Some(Ok(output)),
            Err(e) => {
                if !cm.is_alive() {
                    if let Ok(mut guard) = self.control_mode.lock() {
                        *guard = None;
                    }
                    None
                } else {
                    Some(Err(e))
                }
            }
        }
    }

    fn run_spawn(&self, args: &[&str]) -> Result<String> {
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
}

impl Default for Tmux {
    fn default() -> Self {
        Self::new()
    }
}

impl Tmux {
    fn ensure_server_running(&self) {
        let _ = Command::new("tmux").arg("start-server").output();
    }
}

impl Terminal for Tmux {
    fn create_session(&self, name: &str, cwd: &Path) -> Result<SessionHandle> {
        self.ensure_server_running();
        let session_name = session_name_for_task(name);
        let cwd_str = cwd.to_string_lossy();

        self.run(&["new-session", "-d", "-s", &session_name, "-c", &cwd_str])?;

        Ok(SessionHandle(session_name))
    }

    fn create_pane(&self, session: &SessionHandle, cwd: &Path) -> Result<PaneHandle> {
        self.ensure_server_running();
        let cwd_str = cwd.to_string_lossy();

        let pane_id = self.run(&[
            "new-window",
            "-t",
            &session.0,
            "-c",
            &cwd_str,
            "-P",
            "-F",
            "#{pane_id}",
        ])?;

        let title = cwd
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| cwd_str.to_string());

        Ok(PaneHandle(pane_id, title))
    }

    fn shell_init_delay(&self) {
        let ms = self.config.shell_init_delay_ms;
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
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
        // TUI agents (e.g. Codex) need time to render input before accepting Enter.
        std::thread::sleep(std::time::Duration::from_millis(100));
        self.run(&["send-keys", "-t", &pane.0, "Enter"])?;
        Ok(())
    }

    fn send_key(&self, pane: &PaneHandle, key: &str) -> Result<()> {
        self.run(&["send-keys", "-t", &pane.0, key])?;
        Ok(())
    }

    fn send_literal(&self, pane: &PaneHandle, text: &str) -> Result<()> {
        self.run(&["send-keys", "-t", &pane.0, "-l", text])?;
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

    fn select_pane(&self, pane: &PaneHandle) -> Result<()> {
        self.run(&["select-window", "-t", &pane.0])?;
        self.run(&["select-pane", "-t", &pane.0])?;
        Ok(())
    }

    fn list_panes(&self, session: &SessionHandle) -> Result<Vec<PaneHandle>> {
        let output = self.run(&[
            "list-panes",
            "-s",
            "-t",
            &session.0,
            "-F",
            "#{pane_id}\t#{pane_current_path}",
        ])?;

        Ok(output
            .lines()
            .filter(|s| !s.is_empty())
            .map(|line| {
                let mut parts = line.splitn(2, '\t');
                let id = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("");
                let title = std::path::Path::new(path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string());
                PaneHandle(id, title)
            })
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
        let session_name = session_name_for_task(name);
        let result = self.run(&["has-session", "-t", &session_name]);
        Ok(result.is_ok())
    }

    fn get_pane_command(&self, pane: &PaneHandle) -> Result<String> {
        self.run(&["display", "-p", "-t", &pane.0, "#{pane_current_command}"])
    }

    fn resize_pane(&self, pane: &PaneHandle, width: u16, height: u16) -> Result<()> {
        self.run(&[
            "resize-window",
            "-t",
            &pane.0,
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
        ])?;
        Ok(())
    }

    fn send_text_enter(&self, pane: &PaneHandle, text: &str, delay_ms: u64) -> Result<()> {
        self.run(&["send-keys", "-t", &pane.0, "-l", text])?;
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        self.run(&["send-keys", "-t", &pane.0, "Enter"])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_arg_simple_string() {
        assert_eq!(quote_arg_for_control_mode("simple"), "simple");
    }

    #[test]
    fn quote_arg_with_space() {
        assert_eq!(
            quote_arg_for_control_mode("path with spaces"),
            "\"path with spaces\""
        );
    }

    #[test]
    fn quote_arg_format_string_with_braces() {
        assert_eq!(quote_arg_for_control_mode("#{pane_id}"), "\"#{pane_id}\"");
    }

    #[test]
    fn quote_arg_format_string_with_hash() {
        assert_eq!(quote_arg_for_control_mode("#S:#I.#P"), "\"#S:#I.#P\"");
    }

    #[test]
    fn quote_arg_dollar_sign() {
        assert_eq!(quote_arg_for_control_mode("$HOME"), "\"$HOME\"");
    }

    #[test]
    fn quote_arg_semicolon() {
        assert_eq!(quote_arg_for_control_mode("cmd1; cmd2"), "\"cmd1; cmd2\"");
    }

    #[test]
    fn quote_arg_single_quote() {
        assert_eq!(quote_arg_for_control_mode("it's"), "\"it's\"");
    }

    #[test]
    fn quote_arg_backtick() {
        assert_eq!(quote_arg_for_control_mode("`cmd`"), "\"`cmd`\"");
    }

    #[test]
    fn quote_arg_backslash_escaped() {
        assert_eq!(quote_arg_for_control_mode("path\\file"), "\"path\\\\file\"");
    }

    #[test]
    fn quote_arg_double_quote_escaped() {
        assert_eq!(
            quote_arg_for_control_mode("say \"hello\""),
            "\"say \\\"hello\\\"\""
        );
    }

    #[test]
    fn quote_arg_multiple_special_chars() {
        assert_eq!(
            quote_arg_for_control_mode("#{pane_id}\t${var}"),
            "\"#{pane_id}\t${var}\""
        );
    }

    #[test]
    fn quote_arg_tab_character() {
        assert_eq!(quote_arg_for_control_mode("col1\tcol2"), "\"col1\tcol2\"");
    }

    #[test]
    fn quote_arg_newline_character() {
        assert_eq!(
            quote_arg_for_control_mode("line1\nline2"),
            "\"line1\nline2\""
        );
    }

    #[test]
    fn quote_arg_hyphen_no_quoting() {
        assert_eq!(quote_arg_for_control_mode("-t"), "-t");
    }

    #[test]
    fn quote_arg_tmux_special_keys_no_quoting() {
        assert_eq!(quote_arg_for_control_mode("C-c"), "C-c");
        assert_eq!(quote_arg_for_control_mode("M-x"), "M-x");
        assert_eq!(quote_arg_for_control_mode("BTab"), "BTab");
        assert_eq!(quote_arg_for_control_mode("Enter"), "Enter");
    }

    #[test]
    fn quote_arg_pane_id_no_quoting() {
        assert_eq!(quote_arg_for_control_mode("%42"), "%42");
    }

    #[test]
    fn quote_arg_session_name_no_quoting() {
        assert_eq!(
            quote_arg_for_control_mode("wagner_my-task"),
            "wagner_my-task"
        );
    }

    #[test]
    fn quote_arg_empty_string() {
        assert_eq!(quote_arg_for_control_mode(""), "");
    }

    #[test]
    fn quote_arg_path_no_spaces() {
        assert_eq!(
            quote_arg_for_control_mode("/home/user/project"),
            "/home/user/project"
        );
    }

    #[test]
    fn quote_arg_path_with_spaces() {
        assert_eq!(
            quote_arg_for_control_mode("/home/user/my project"),
            "\"/home/user/my project\""
        );
    }

    #[test]
    fn build_command_simple() {
        let cmd = build_control_mode_command(&["list-sessions"]);
        assert_eq!(cmd, "list-sessions");
    }

    #[test]
    fn build_command_with_options() {
        let cmd = build_control_mode_command(&["new-session", "-d", "-s", "mysession"]);
        assert_eq!(cmd, "new-session -d -s mysession");
    }

    #[test]
    fn build_command_with_format_string() {
        let cmd =
            build_control_mode_command(&["list-panes", "-F", "#{pane_id}\t#{pane_current_path}"]);
        assert_eq!(cmd, "list-panes -F \"#{pane_id}\t#{pane_current_path}\"");
    }

    #[test]
    fn build_command_with_path_spaces() {
        let cmd = build_control_mode_command(&["new-window", "-c", "/home/user/my project"]);
        assert_eq!(cmd, "new-window -c \"/home/user/my project\"");
    }

    #[test]
    fn build_command_create_pane() {
        let cmd = build_control_mode_command(&[
            "new-window",
            "-t",
            "wagner_test",
            "-c",
            "/home/user/project",
            "-P",
            "-F",
            "#{pane_id}",
        ]);
        assert_eq!(
            cmd,
            "new-window -t wagner_test -c /home/user/project -P -F \"#{pane_id}\""
        );
    }

    #[test]
    fn build_command_capture_pane() {
        let cmd =
            build_control_mode_command(&["capture-pane", "-t", "%42", "-p", "-e", "-S", "-500"]);
        assert_eq!(cmd, "capture-pane -t %42 -p -e -S -500");
    }

    #[test]
    fn build_command_send_keys() {
        let cmd = build_control_mode_command(&["send-keys", "-t", "%0", "-l", "hello world"]);
        assert_eq!(cmd, "send-keys -t %0 -l \"hello world\"");
    }

    #[test]
    fn build_command_empty_args() {
        let cmd = build_control_mode_command(&[]);
        assert_eq!(cmd, "");
    }

    #[test]
    fn build_command_preserves_backslash_in_path() {
        let cmd = build_control_mode_command(&["send-keys", "-l", "C:\\Users\\test"]);
        assert_eq!(cmd, "send-keys -l \"C:\\\\Users\\\\test\"");
    }
}
