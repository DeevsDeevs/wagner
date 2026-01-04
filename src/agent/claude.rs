use super::Agent;
use crate::error::Result;
use crate::model::SessionStatus;
use std::path::Path;

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

impl Agent for ClaudeCode {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn launch_command(&self) -> &str {
        "claude"
    }

    fn setup_hooks(&self, worktree: &Path) -> Result<()> {
        let claude_dir = worktree.join(".claude");
        std::fs::create_dir_all(&claude_dir)?;

        let settings_path = claude_dir.join("settings.json");

        let hooks = serde_json::json!({
            "hooks": {
                "SessionStart": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": "wagner-hook session-start"
                    }]
                }],
                "PostToolUse": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": "wagner-hook tool-use"
                    }]
                }],
                "Notification": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": "wagner-hook notification"
                    }]
                }],
                "Stop": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": "wagner-hook stop"
                    }]
                }]
            }
        });

        if settings_path.exists() {
            let content = std::fs::read_to_string(&settings_path)?;
            if let Ok(mut existing) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(obj) = existing.as_object_mut() {
                    obj.insert("hooks".to_string(), hooks["hooks"].clone());
                    std::fs::write(&settings_path, serde_json::to_string_pretty(&existing)?)?;
                    return Ok(());
                }
            }
        }

        std::fs::write(&settings_path, serde_json::to_string_pretty(&hooks)?)?;
        Ok(())
    }

    fn parse_hook_event(&self, event: &str) -> Option<SessionStatus> {
        let parts: Vec<&str> = event.split(':').collect();
        if parts.is_empty() {
            return None;
        }

        match parts[0] {
            "session-start" => Some(SessionStatus::Starting),
            "tool-use" => Some(SessionStatus::Running),
            "notification" => Some(SessionStatus::WaitingForInput),
            "stop" => Some(SessionStatus::Stopped),
            _ => None,
        }
    }

    fn detect_status(&self, output: &str) -> SessionStatus {
        let last_lines: String = output.lines().rev().take(10).collect::<Vec<_>>().join("\n");

        if last_lines.contains("[Y/n]") || last_lines.contains("[y/N]") {
            return SessionStatus::WaitingForInput;
        }

        if last_lines.contains("> ") && !last_lines.contains("...") {
            return SessionStatus::Idle;
        }

        if last_lines.contains("...") || last_lines.contains("Thinking") {
            return SessionStatus::Running;
        }

        SessionStatus::Idle
    }
}
