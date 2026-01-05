use crate::monitor::detector::{ActivityPattern, AgentDetector, WaitPattern};
use crate::monitor::status::{ActivityKind, AgentType, ClaudeActivity, WaitReason};

pub struct ClaudeCodeDetector {
    activity_patterns: Vec<ActivityPattern>,
    waiting_patterns: Vec<WaitPattern>,
}

impl ClaudeCodeDetector {
    pub fn new() -> Self {
        Self {
            activity_patterns: vec![
                ActivityPattern::any_of(
                    &["Task tool", "Spawning", "subagent"],
                    ActivityKind::Claude(ClaudeActivity::Subagent),
                ),
                ActivityPattern::any_of(
                    &["WebSearch", "Searching web"],
                    ActivityKind::Claude(ClaudeActivity::WebSearch),
                ),
                ActivityPattern::any_of(
                    &["WebFetch", "Fetching"],
                    ActivityKind::Claude(ClaudeActivity::WebFetch),
                ),
                ActivityPattern::any_of(
                    &["Glob", "Grep", "Searching", "Finding files"],
                    ActivityKind::Claude(ClaudeActivity::Exploring),
                ),
                ActivityPattern::any_of(
                    &["Read", "Reading file"],
                    ActivityKind::Claude(ClaudeActivity::ToolRead),
                ),
                ActivityPattern::any_of(
                    &["Edit", "Editing"],
                    ActivityKind::Claude(ClaudeActivity::ToolEdit),
                ),
                ActivityPattern::any_of(
                    &["Write", "Writing file", "Creating file"],
                    ActivityKind::Claude(ClaudeActivity::ToolWrite),
                ),
                ActivityPattern::any_of(
                    &["Bash", "Running command", "$ "],
                    ActivityKind::Claude(ClaudeActivity::ToolBash),
                ),
                ActivityPattern::any_of(
                    &["TodoWrite", "todo"],
                    ActivityKind::Claude(ClaudeActivity::TodoUpdate),
                ),
                ActivityPattern::any_of(
                    &["EnterPlanMode", "Planning"],
                    ActivityKind::Claude(ClaudeActivity::Planning),
                ),
                ActivityPattern::any_of(
                    &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "...", "Thinking"],
                    ActivityKind::Claude(ClaudeActivity::Thinking),
                ),
            ],
            waiting_patterns: vec![
                WaitPattern::any_of(
                    &["[Y/n]", "[y/N]", "Do you want to proceed"],
                    WaitReason::Approval,
                ),
                WaitPattern::contains(
                    "No, and tell Claude what to do differently",
                    WaitReason::Approval,
                ),
                WaitPattern::any_of(
                    &["Permission denied", "requires permission"],
                    WaitReason::Permission,
                ),
                WaitPattern::contains("?", WaitReason::Question),
            ],
        }
    }
}

impl Default for ClaudeCodeDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentDetector for ClaudeCodeDetector {
    fn agent_type(&self) -> AgentType {
        AgentType::ClaudeCode
    }

    fn launch_command(&self) -> &'static str {
        "claude"
    }

    fn detect_agent(&self, pane_command: &str, output: &str) -> bool {
        pane_command.contains("claude")
            || output.contains("Claude Code")
            || output.contains("╭─")
            || output.contains("Anthropic")
    }

    fn activity_patterns(&self) -> &[ActivityPattern] {
        &self.activity_patterns
    }

    fn waiting_patterns(&self) -> &[WaitPattern] {
        &self.waiting_patterns
    }
}
