use std::time::Duration;

use crate::monitor::{
    Activity, ActivityKind, AgentDetector, AgentStatus, AgentType, CodexActivity, WaitReason,
};

const AGENT_PATTERNS: &[&str] = &[
    "openai codex",
    "ask codex",
    "you are running codex in",
];

const ACTIVE_PATTERNS: &[&str] = &[
    "• working",
    "working (",
    "working...",
    "working…",
    "streaming response",
];

const WAIT_APPROVAL_PATTERNS: &[&str] = &[
    "press enter to confirm or esc to cancel",
    "press enter to confirm or esc to go back",
    "would you like to run the following command?",
    "codex wants to edit",
    "no, and tell codex what to do differently",
];

const WAIT_QUESTION_PATTERNS: &[&str] = &[
    "press enter to continue",
    "press esc to cancel",
    "press space or enter to toggle; esc to close",
    "press space to select or enter to save",
    "press enter to select reasoning effort, or esc to dismiss.",
];

const WAIT_INPUT_PATTERNS: &[&str] = &[
    "ask codex",
    "explain this codebase",
    "summarize recent commits",
    "implement {feature}",
    "find and fix a bug in @filename",
    "write tests for @filename",
    "improve documentation in @filename",
    "run /review on my current changes",
    "use /skills to list available skills",
];

pub struct CodexDetector;

impl CodexDetector {
    fn has_status_timer_line(output: &str) -> bool {
        for line in output.lines() {
            let has_spinner = line.contains('•') || line.contains('◦');
            if !has_spinner {
                continue;
            }
            let Some(start) = line.find('(') else {
                continue;
            };
            let Some(end) = line.rfind(')') else {
                continue;
            };
            if end <= start {
                continue;
            }
            let inside = &line[start + 1..end];
            let has_digit = inside.chars().any(|c| c.is_ascii_digit());
            if has_digit && inside.contains('s') {
                return true;
            }
        }
        false
    }

    fn detect_wait(output: &str) -> Option<WaitReason> {
        let tail: String = output.lines().rev().take(15).collect::<Vec<_>>().join("\n");
        let tail = tail.to_ascii_lowercase();
        if WAIT_APPROVAL_PATTERNS.iter().any(|p| tail.contains(p)) {
            return Some(WaitReason::Approval);
        }
        if WAIT_QUESTION_PATTERNS.iter().any(|p| tail.contains(p)) {
            return Some(WaitReason::Question);
        }
        if WAIT_INPUT_PATTERNS.iter().any(|p| tail.contains(p)) {
            return Some(WaitReason::Input);
        }
        None
    }

    fn detect_active(output: &str) -> Option<CodexActivity> {
        let normalized = output.to_ascii_lowercase();
        if normalized.contains("streaming response") {
            return Some(CodexActivity::Streaming);
        }
        if Self::has_status_timer_line(output) {
            return Some(CodexActivity::Working);
        }
        if ACTIVE_PATTERNS.iter().any(|p| normalized.contains(p)) {
            return Some(CodexActivity::Working);
        }
        None
    }
}

impl Default for CodexDetector {
    fn default() -> Self {
        Self
    }
}

impl AgentDetector for CodexDetector {
    fn agent_type(&self) -> AgentType {
        AgentType::Codex
    }

    fn detect_agent(&self, pane_command: &str, output: &str) -> bool {
        if pane_command.to_ascii_lowercase().contains("codex") {
            return true;
        }
        let normalized = output.to_ascii_lowercase();
        AGENT_PATTERNS.iter().any(|p| normalized.contains(p))
    }

    fn detect_status(
        &self,
        _raw_output: &str,
        clean_output: &str,
        _output_changed: bool,
        _since_change: Duration,
    ) -> AgentStatus {
        if let Some(activity) = Self::detect_active(clean_output) {
            return AgentStatus::Active(Activity::new(ActivityKind::Codex(activity)));
        }

        if let Some(reason) = Self::detect_wait(clean_output) {
            return AgentStatus::Waiting(reason);
        }

        AgentStatus::Idle
    }
}
