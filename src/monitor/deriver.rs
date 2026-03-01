use std::time::{Duration, Instant};

use crate::model::Engine;
use super::events::AgentEvent;
use super::status::{
    Activity, ActivityKind, AgentStatus, AgentType, ClaudeActivity, CodexActivity, PaneStatus,
    WaitReason,
};

pub struct StatusDeriver {
    engine: Engine,
    state: DerivedState,
    pending_tool: Option<PendingTool>,
    last_event_time: Instant,
    last_context: Option<String>,
    approval_timeout: Duration,
    idle_threshold: Duration,
}

#[derive(Debug, Clone, PartialEq)]
enum DerivedState {
    Idle,
    Active,
    Waiting(WaitReason),
}

struct PendingTool {
    tool_id: String,
    tool_name: String,
    proposed_at: Instant,
}

impl StatusDeriver {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            state: DerivedState::Idle,
            pending_tool: None,
            last_event_time: Instant::now(),
            last_context: None,
            approval_timeout: Duration::from_millis(1000),
            idle_threshold: Duration::from_millis(2000),
        }
    }

    pub fn with_approval_timeout(mut self, timeout: Duration) -> Self {
        self.approval_timeout = timeout;
        self
    }

    pub fn with_idle_threshold(mut self, threshold: Duration) -> Self {
        self.idle_threshold = threshold;
        self
    }

    pub fn process(&mut self, event: &AgentEvent) -> PaneStatus {
        if !matches!(event, AgentEvent::Progress) {
            self.last_event_time = Instant::now();
        }

        match event {
            AgentEvent::UserMessage => {
                self.state = DerivedState::Active;
                self.pending_tool = None;
            }
            AgentEvent::Thinking { .. } => {
                self.state = DerivedState::Active;
            }
            AgentEvent::TextOutput { .. } => {
                self.state = DerivedState::Active;
            }
            AgentEvent::ToolProposed {
                tool_id,
                tool_name,
                tool_context,
                ..
            } => {
                self.state = DerivedState::Active;
                self.last_context = tool_context.clone().or_else(|| Some(tool_name.clone()));
                self.pending_tool = Some(PendingTool {
                    tool_id: tool_id.clone(),
                    tool_name: tool_name.clone(),
                    proposed_at: Instant::now(),
                });
            }
            AgentEvent::ToolCompleted { tool_id, .. } => {
                if self
                    .pending_tool
                    .as_ref()
                    .is_some_and(|p| p.tool_id == *tool_id)
                {
                    self.pending_tool = None;
                }
                self.state = DerivedState::Active;
            }
            AgentEvent::ToolRejected { tool_id, .. } => {
                if self
                    .pending_tool
                    .as_ref()
                    .is_some_and(|p| p.tool_id == *tool_id)
                {
                    self.pending_tool = None;
                }
                self.state = DerivedState::Active;
            }
            AgentEvent::TurnComplete { .. } => {
                self.state = DerivedState::Idle;
                self.pending_tool = None;
                self.last_context = None;
            }
            AgentEvent::SessionStarted { .. } => {
                self.state = DerivedState::Active;
            }
            AgentEvent::Progress => {}
        }

        self.to_pane_status()
    }

    pub fn tick(&mut self) -> PaneStatus {
        if let Some(ref pending) = self.pending_tool {
            if pending.proposed_at.elapsed() >= self.approval_timeout {
                let reason = if pending.tool_name == "AskUserQuestion" {
                    WaitReason::Question
                } else {
                    WaitReason::Approval
                };
                self.state = DerivedState::Waiting(reason);
            }
        }

        if self.state == DerivedState::Active
            && self.last_event_time.elapsed() >= self.idle_threshold
        {
            self.state = DerivedState::Idle;
            self.pending_tool = None;
        }

        self.to_pane_status()
    }

    pub fn last_tool_name(&self) -> Option<&str> {
        self.pending_tool.as_ref().map(|p| p.tool_name.as_str())
    }

    pub fn last_context(&self) -> Option<&str> {
        self.last_context.as_deref()
    }

    fn to_pane_status(&self) -> PaneStatus {
        let agent_type = match self.engine {
            Engine::ClaudeCode => AgentType::ClaudeCode,
            Engine::Codex => AgentType::Codex,
        };

        let status = match &self.state {
            DerivedState::Idle => AgentStatus::Idle,
            DerivedState::Waiting(reason) => AgentStatus::Waiting(*reason),
            DerivedState::Active => {
                let activity = self.derive_activity();
                AgentStatus::Active(activity)
            }
        };

        PaneStatus::Agent { agent_type, status }
    }

    fn derive_activity(&self) -> Activity {
        if let Some(ref pending) = self.pending_tool {
            return tool_name_to_activity(self.engine, &pending.tool_name);
        }
        match self.engine {
            Engine::ClaudeCode => Activity::new(ActivityKind::Claude(ClaudeActivity::Thinking)),
            Engine::Codex => Activity::new(ActivityKind::Codex(CodexActivity::Working)),
        }
    }
}

fn tool_name_to_activity(engine: Engine, tool_name: &str) -> Activity {
    match engine {
        Engine::ClaudeCode => {
            let kind = match tool_name {
                "Bash" => ClaudeActivity::ToolBash,
                "Edit" | "NotebookEdit" => ClaudeActivity::ToolEdit,
                "Write" => ClaudeActivity::ToolWrite,
                "Read" | "Glob" | "Grep" => ClaudeActivity::ToolRead,
                "Agent" => ClaudeActivity::Subagent,
                "WebSearch" => ClaudeActivity::WebSearch,
                "WebFetch" => ClaudeActivity::WebFetch,
                "TodoWrite" | "TaskCreate" | "TaskUpdate" => ClaudeActivity::TodoUpdate,
                _ => ClaudeActivity::Exploring,
            };
            Activity::new(ActivityKind::Claude(kind))
        }
        Engine::Codex => {
            let kind = match tool_name {
                "exec_command" => CodexActivity::Working,
                _ => CodexActivity::Working,
            };
            Activity::new(ActivityKind::Codex(kind))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_deriver() -> StatusDeriver {
        StatusDeriver::new(Engine::ClaudeCode)
            .with_approval_timeout(Duration::from_millis(50))
            .with_idle_threshold(Duration::from_millis(100))
    }

    fn codex_deriver() -> StatusDeriver {
        StatusDeriver::new(Engine::Codex)
            .with_approval_timeout(Duration::from_millis(50))
            .with_idle_threshold(Duration::from_millis(100))
    }

    #[test]
    fn starts_idle() {
        let d = claude_deriver();
        let status = d.to_pane_status();
        assert!(status.is_idle());
    }

    #[test]
    fn user_message_activates() {
        let mut d = claude_deriver();
        let status = d.process(&AgentEvent::UserMessage);
        assert!(status.is_active());
    }

    #[test]
    fn thinking_activates() {
        let mut d = claude_deriver();
        let status = d.process(&AgentEvent::Thinking {
            engine: Engine::ClaudeCode,
        });
        assert!(status.is_active());
    }

    #[test]
    fn turn_complete_idles() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::UserMessage);
        let status = d.process(&AgentEvent::TurnComplete {
            engine: Engine::ClaudeCode,
        });
        assert!(status.is_idle());
    }

    #[test]
    fn tool_proposed_then_completed() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::ToolProposed {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            tool_name: "Bash".into(),
            tool_context: None,
        });
        assert!(d.last_tool_name() == Some("Bash"));

        let status = d.process(&AgentEvent::ToolCompleted {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            is_error: false,
        });
        assert!(status.is_active());
        assert!(d.last_tool_name().is_none());
    }

    #[test]
    fn tool_proposed_timeout_becomes_waiting() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::ToolProposed {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            tool_name: "Bash".into(),
            tool_context: None,
        });

        std::thread::sleep(Duration::from_millis(60));
        let status = d.tick();
        assert!(status.is_waiting());
    }

    #[test]
    fn ask_user_question_becomes_question_wait() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::ToolProposed {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            tool_name: "AskUserQuestion".into(),
            tool_context: Some("Which database?".into()),
        });

        std::thread::sleep(Duration::from_millis(60));
        let status = d.tick();
        match status {
            PaneStatus::Agent {
                status: AgentStatus::Waiting(WaitReason::Question),
                ..
            } => {}
            other => panic!("Expected Waiting(Question), got {:?}", other),
        }
    }

    #[test]
    fn tool_rejected_clears_pending() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::ToolProposed {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            tool_name: "Bash".into(),
            tool_context: None,
        });
        d.process(&AgentEvent::ToolRejected {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            reason: "denied".into(),
        });
        assert!(d.pending_tool.is_none());
    }

    #[test]
    fn idle_timeout_after_active() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::Thinking {
            engine: Engine::ClaudeCode,
        });
        assert!(d.tick().is_active());

        std::thread::sleep(Duration::from_millis(110));
        let status = d.tick();
        assert!(status.is_idle());
    }

    #[test]
    fn progress_does_not_change_state() {
        let mut d = claude_deriver();
        let initial = d.to_pane_status();
        d.process(&AgentEvent::Progress);
        let after = d.to_pane_status();
        assert_eq!(initial.label(), after.label());
    }

    #[test]
    fn codex_tool_proposed() {
        let mut d = codex_deriver();
        let status = d.process(&AgentEvent::ToolProposed {
            engine: Engine::Codex,
            tool_id: "call_1".into(),
            tool_name: "exec_command".into(),
            tool_context: None,
        });
        assert!(status.is_active());
    }

    #[test]
    fn codex_task_complete() {
        let mut d = codex_deriver();
        d.process(&AgentEvent::UserMessage);
        let status = d.process(&AgentEvent::TurnComplete {
            engine: Engine::Codex,
        });
        assert!(status.is_idle());
    }

    #[test]
    fn tool_activity_mapping() {
        let a = tool_name_to_activity(Engine::ClaudeCode, "Bash");
        assert_eq!(a.label(), "Bash");

        let a = tool_name_to_activity(Engine::ClaudeCode, "Edit");
        assert_eq!(a.label(), "Edit");

        let a = tool_name_to_activity(Engine::ClaudeCode, "Agent");
        assert_eq!(a.label(), "Subagent");

        let a = tool_name_to_activity(Engine::ClaudeCode, "WebSearch");
        assert_eq!(a.label(), "Web Search");

        let a = tool_name_to_activity(Engine::ClaudeCode, "SomeUnknownTool");
        assert_eq!(a.label(), "Exploring");
    }

    #[test]
    fn last_context_set_from_tool_context() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::ToolProposed {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            tool_name: "Bash".into(),
            tool_context: Some("cargo test".into()),
        });
        assert_eq!(d.last_context(), Some("cargo test"));
    }

    #[test]
    fn last_context_falls_back_to_tool_name() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::ToolProposed {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            tool_name: "WebSearch".into(),
            tool_context: None,
        });
        assert_eq!(d.last_context(), Some("WebSearch"));
    }

    #[test]
    fn last_context_cleared_on_turn_complete() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::ToolProposed {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            tool_name: "Bash".into(),
            tool_context: Some("ls".into()),
        });
        assert!(d.last_context().is_some());
        d.process(&AgentEvent::TurnComplete {
            engine: Engine::ClaudeCode,
        });
        assert_eq!(d.last_context(), None);
    }

    #[test]
    fn progress_does_not_reset_idle_timer() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::Thinking {
            engine: Engine::ClaudeCode,
        });
        assert!(d.tick().is_active());

        std::thread::sleep(Duration::from_millis(80));
        // Progress events should not extend activity
        d.process(&AgentEvent::Progress);
        d.process(&AgentEvent::Progress);

        std::thread::sleep(Duration::from_millis(30));
        let status = d.tick();
        assert!(status.is_idle(), "Progress should not reset idle timer");
    }

    #[test]
    fn session_started_activates() {
        let mut d = claude_deriver();
        let status = d.process(&AgentEvent::SessionStarted {
            engine: Engine::ClaudeCode,
            session_id: "abc".into(),
            model: None,
        });
        assert!(status.is_active());
    }
}
