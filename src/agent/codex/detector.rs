use std::time::Duration;

use crate::monitor::{
    Activity, ActivityKind, AgentDetector, AgentStatus, AgentType, CodexActivity, WaitReason,
};

const AGENT_PATTERNS: &[&str] = &[
    "OpenAI Codex",
    "Ask Codex to do anything",
    "You are running Codex in",
];

const ACTIVE_PATTERNS: &[&str] = &[
    "• Working",
    "Working (",
    "• Streaming response.",
    "Streaming response.",
    "esc to interrupt",
];

const WAIT_APPROVAL_PATTERNS: &[&str] = &[
    "Press enter to confirm or esc to cancel",
    "Press enter to confirm or esc to go back",
    "Would you like to run the following command?",
    "Codex wants to edit",
    "No, and tell Codex what to do differently",
];

const WAIT_QUESTION_PATTERNS: &[&str] = &[
    "Press enter to continue",
    "Press Esc to cancel",
    "Press esc to cancel",
    "Press space or enter to toggle; esc to close",
    "Press space to select or enter to save",
    "Press enter to select reasoning effort, or esc to dismiss.",
];

const WAIT_INPUT_PATTERNS: &[&str] = &[
    "Ask Codex to do anything",
    "Explain this codebase",
    "Summarize recent commits",
    "Implement {feature}",
    "Find and fix a bug in @filename",
    "Write tests for @filename",
    "Improve documentation in @filename",
    "Run /review on my current changes",
    "Use /skills to list available skills",
];

pub struct CodexDetector;

impl CodexDetector {
    fn detect_wait(output: &str) -> Option<WaitReason> {
        let tail: String = output.lines().rev().take(15).collect::<Vec<_>>().join("\n");
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
        let tail: String = output.lines().rev().take(20).collect::<Vec<_>>().join("\n");
        if tail.contains("Streaming response.") {
            return Some(CodexActivity::Streaming);
        }
        if ACTIVE_PATTERNS.iter().any(|p| tail.contains(p)) {
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
        if pane_command.contains("codex") {
            return true;
        }
        AGENT_PATTERNS.iter().any(|p| output.contains(p))
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
