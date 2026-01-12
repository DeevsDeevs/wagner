use std::time::Duration;

use crate::monitor::{
    Activity, ActivityKind, AgentDetector, AgentStatus, AgentType, ClaudeActivity, WaitReason,
};

const BRAILLE_SPINNERS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const STAR_SPINNERS: &[char] = &['✢', '✳', '✶', '✻', '✽'];
const IDLE_GRAY: (u8, u8, u8) = (153, 153, 153);
const GRAY_TOLERANCE: u8 = 20;

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
    (
        &[
            "How is Claude doing this session?",
            "Claude is waiting for your input",
        ],
        WaitReason::Input,
    ),
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
    (
        &[
            "Enter plan mode?",
            "Exit plan mode?",
            "Claude wants to enter plan mode",
            "Claude wants to exit plan mode",
            "Yes, enter plan mode",
        ],
        WaitReason::Approval,
    ),
    (
        &[
            "Enter to apply changes",
            "Enter to confirm · Esc to reject",
            "Enter to confirm · Esc to skip",
            "Yes, and auto-accept edits",
        ],
        WaitReason::Approval,
    ),
];

pub struct ClaudeCodeDetector;

impl ClaudeCodeDetector {
    fn has_spinner(output: &str) -> bool {
        let tail: String = output.lines().rev().take(10).collect::<Vec<_>>().join("\n");
        BRAILLE_SPINNERS.iter().any(|&c| tail.contains(c))
    }

    fn has_active_status(output: &str) -> bool {
        let tail: String = output.lines().rev().take(5).collect::<Vec<_>>().join("\n");
        tail.contains("⎿  …") || tail.contains("⎿ …") || tail.contains("Running")
    }

    fn find_star_spinner(line: &str) -> Option<usize> {
        STAR_SPINNERS
            .iter()
            .filter_map(|&c| line.find(c))
            .min()
    }

    fn star_indicator_state(raw_output: &str) -> Option<bool> {
        for line in raw_output.lines().rev().take(15) {
            if let Some(pos) = Self::find_star_spinner(line) {
                if let Some(rgb) = Self::find_last_rgb_color(&line[..pos]) {
                    return Some(!Self::is_gray_color(rgb));
                }
            }
        }
        None
    }

    fn find_last_rgb_color(text: &str) -> Option<(u8, u8, u8)> {
        let mut last_color = None;
        let mut search_from = 0;
        while let Some(pos) = text[search_from..].find("38;2;") {
            let start = search_from + pos + 5;
            if let Some(color) = Self::parse_rgb_at(&text[start..]) {
                last_color = Some(color);
            }
            search_from = search_from + pos + 1;
        }
        last_color
    }

    fn parse_rgb_at(text: &str) -> Option<(u8, u8, u8)> {
        let parts: Vec<&str> = text.split(|c| c == ';' || c == 'm').take(3).collect();
        if parts.len() >= 3 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            return Some((r, g, b));
        }
        None
    }

    fn is_gray_color((r, g, b): (u8, u8, u8)) -> bool {
        let is_uniform = r.abs_diff(g) <= GRAY_TOLERANCE
            && g.abs_diff(b) <= GRAY_TOLERANCE
            && r.abs_diff(b) <= GRAY_TOLERANCE;
        let is_gray_level = r.abs_diff(IDLE_GRAY.0) <= GRAY_TOLERANCE * 2
            && g.abs_diff(IDLE_GRAY.1) <= GRAY_TOLERANCE * 2
            && b.abs_diff(IDLE_GRAY.2) <= GRAY_TOLERANCE * 2;
        is_uniform && is_gray_level
    }

    fn detect_tool(output: &str) -> Option<ClaudeActivity> {
        let tail_lines: Vec<&str> = output.lines().rev().take(15).collect();

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
        let tail: String = output.lines().rev().take(10).collect::<Vec<_>>().join("\n");
        WAIT_PATTERNS
            .iter()
            .find(|(patterns, _)| patterns.iter().any(|p| tail.contains(p)))
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
        raw_output: &str,
        clean_output: &str,
        output_changed: bool,
        _since_change: Duration,
    ) -> AgentStatus {
        let has_spinner = Self::has_spinner(clean_output);
        let star_active = Self::star_indicator_state(raw_output);
        let has_active_text = output_changed && Self::has_active_status(clean_output);

        if has_spinner || star_active == Some(true) || has_active_text {
            let activity = Self::detect_tool(clean_output).unwrap_or(ClaudeActivity::Thinking);
            return AgentStatus::Active(Activity::new(ActivityKind::Claude(activity)));
        }

        if let Some(reason) = Self::detect_wait(clean_output) {
            return AgentStatus::Waiting(reason);
        }

        AgentStatus::Idle
    }
}
