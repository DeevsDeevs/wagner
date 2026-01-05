use std::time::Duration;

use crate::monitor::{
    Activity, ActivityKind, AgentDetector, AgentStatus, AgentType, ClaudeActivity, IDLE_THRESHOLD,
    WaitReason,
};

const BRAILLE_SPINNERS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const ACTIVITY_SPINNER: char = '✻';

const TOOL_PATTERNS: &[(&[&str], ClaudeActivity)] = &[
    (&["● Bash"], ClaudeActivity::ToolBash),
    (&["● Read"], ClaudeActivity::ToolRead),
    (&["● Edit", "● Update"], ClaudeActivity::ToolEdit),
    (&["● Write"], ClaudeActivity::ToolWrite),
    (&["● Glob", "● Grep", "● Search"], ClaudeActivity::Exploring),
    (&["● WebSearch"], ClaudeActivity::WebSearch),
    (&["● WebFetch"], ClaudeActivity::WebFetch),
    (&["● Plan", "● Task"], ClaudeActivity::Subagent),
    (
        &["● TodoWrite", "● Updated plan"],
        ClaudeActivity::TodoUpdate,
    ),
];

const WAIT_PATTERNS: &[(&[&str], WaitReason)] = &[
    (
        &[
            "Enter to select",
            "Tab/Arrow keys to navigate",
            "Esc to cancel",
        ],
        WaitReason::Question,
    ),
    (&["How is Claude doing this session?"], WaitReason::Input),
    (
        &[
            "Interrupted · What should Claude do instead?",
            "What should Claude do instead?",
        ],
        WaitReason::Input,
    ),
    (
        &["[Y/n]", "[y/N]", "Do you want to proceed"],
        WaitReason::Approval,
    ),
    (
        &["No, and tell Claude what to do differently"],
        WaitReason::Approval,
    ),
    (
        &["Permission denied", "requires permission"],
        WaitReason::Permission,
    ),
];

pub struct ClaudeCodeDetector;

impl ClaudeCodeDetector {
    fn has_spinner(output: &str) -> bool {
        output.contains(ACTIVITY_SPINNER) || BRAILLE_SPINNERS.iter().any(|&c| output.contains(c))
    }

    fn has_active_status(output: &str) -> bool {
        output
            .lines()
            .rev()
            .take(5)
            .any(|line| line.contains("…") || line.contains("tokens)"))
    }

    fn detect_tool(output: &str) -> Option<ClaudeActivity> {
        let tail_lines: Vec<&str> = output.lines().rev().take(20).collect();
        TOOL_PATTERNS
            .iter()
            .find(|(patterns, _)| {
                patterns
                    .iter()
                    .any(|p| tail_lines.iter().any(|line| line.contains(p)))
            })
            .map(|(_, activity)| *activity)
    }

    fn detect_wait(output: &str) -> Option<WaitReason> {
        WAIT_PATTERNS
            .iter()
            .find(|(patterns, _)| patterns.iter().any(|p| output.contains(p)))
            .map(|(_, reason)| *reason)
    }
}

impl Default for ClaudeCodeDetector {
    fn default() -> Self {
        Self
    }
}

impl AgentDetector for ClaudeCodeDetector {
    fn agent_type(&self) -> AgentType {
        AgentType::ClaudeCode
    }

    fn detect_agent(&self, pane_command: &str, output: &str) -> bool {
        pane_command.contains("claude")
            || output.contains("Claude Code")
            || output.contains("╭─")
            || output.contains("Anthropic")
    }

    fn detect_status(
        &self,
        output: &str,
        output_changed: bool,
        since_change: Duration,
    ) -> AgentStatus {
        if let Some(reason) = Self::detect_wait(output) {
            return AgentStatus::Waiting(reason);
        }

        let is_active =
            Self::has_spinner(output) || (output_changed && Self::has_active_status(output));

        if is_active {
            let activity = Self::detect_tool(output).unwrap_or(ClaudeActivity::Thinking);
            return AgentStatus::Active(Activity::new(ActivityKind::Claude(activity)));
        }

        if since_change > IDLE_THRESHOLD {
            AgentStatus::Idle
        } else {
            AgentStatus::Active(Activity::generic_working())
        }
    }
}
