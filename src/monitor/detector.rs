use std::time::Duration;

use super::status::{Activity, ActivityKind, AgentStatus, AgentType, WaitReason};

pub trait AgentDetector: Send + Sync {
    fn agent_type(&self) -> AgentType;

    fn launch_command(&self) -> &'static str;

    fn detect_agent(&self, pane_command: &str, output: &str) -> bool;

    fn activity_patterns(&self) -> &[ActivityPattern];

    fn waiting_patterns(&self) -> &[WaitPattern];

    fn detect_status(
        &self,
        output: &str,
        output_changed: bool,
        since_change: Duration,
    ) -> AgentStatus {
        for wp in self.waiting_patterns() {
            if wp.matches(output) {
                return AgentStatus::Waiting(wp.reason.clone());
            }
        }

        if output_changed {
            for ap in self.activity_patterns() {
                if ap.matches(output) {
                    return AgentStatus::Active(Activity::new(ap.activity.clone()));
                }
            }
            return AgentStatus::Active(Activity::generic_working());
        }

        if since_change > Duration::from_secs(2) {
            return AgentStatus::Idle;
        }

        AgentStatus::Active(Activity::generic_working())
    }
}

pub struct ActivityPattern {
    pub matcher: PatternMatcher,
    pub activity: ActivityKind,
}

impl ActivityPattern {
    pub fn contains(pattern: &'static str, activity: ActivityKind) -> Self {
        Self {
            matcher: PatternMatcher::Contains(pattern),
            activity,
        }
    }

    pub fn any_of(patterns: &'static [&'static str], activity: ActivityKind) -> Self {
        Self {
            matcher: PatternMatcher::AnyOf(patterns),
            activity,
        }
    }

    pub fn matches(&self, output: &str) -> bool {
        self.matcher.matches(output)
    }
}

pub struct WaitPattern {
    pub matcher: PatternMatcher,
    pub reason: WaitReason,
}

impl WaitPattern {
    pub fn contains(pattern: &'static str, reason: WaitReason) -> Self {
        Self {
            matcher: PatternMatcher::Contains(pattern),
            reason,
        }
    }

    pub fn any_of(patterns: &'static [&'static str], reason: WaitReason) -> Self {
        Self {
            matcher: PatternMatcher::AnyOf(patterns),
            reason,
        }
    }

    pub fn matches(&self, output: &str) -> bool {
        self.matcher.matches(output)
    }
}

pub enum PatternMatcher {
    Contains(&'static str),
    AnyOf(&'static [&'static str]),
    StartsWith(&'static str),
    EndsWith(&'static str),
}

impl PatternMatcher {
    pub fn matches(&self, output: &str) -> bool {
        match self {
            Self::Contains(p) => output.contains(p),
            Self::AnyOf(patterns) => patterns.iter().any(|p| output.contains(p)),
            Self::StartsWith(p) => output.starts_with(p),
            Self::EndsWith(p) => output.ends_with(p),
        }
    }
}
