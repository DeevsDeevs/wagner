use std::time::{Duration, Instant};

use super::events::{AgentEvent, QuestionData};
use super::status::{
    Activity, ActivityKind, AgentStatus, AgentType, ClaudeActivity, CodexActivity, DroidActivity,
    GenericActivity, PaneStatus, TerminalStatus, WaitReason,
};
use crate::model::Engine;

#[derive(Debug, Clone)]
pub struct CompletedStep {
    pub tool_name: String,
    pub context: Option<String>,
    pub ok: bool,
}

pub struct StatusDeriver {
    engine: Engine,
    state: DerivedState,
    pending_tool: Option<PendingTool>,
    is_thinking: bool,
    last_event_time: Instant,
    last_context: Option<String>,
    approval_timeout: Duration,
    idle_threshold: Duration,
    completed_steps: Vec<CompletedStep>,
    response_text: Option<String>,
    accumulated_text: Option<String>,
    action_seq: u64,
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
    context: Option<String>,
    question_data: Option<Vec<QuestionData>>,
    proposed_at: Instant,
}

impl StatusDeriver {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            state: DerivedState::Idle,
            pending_tool: None,
            is_thinking: false,
            last_event_time: Instant::now(),
            last_context: None,
            approval_timeout: Duration::from_millis(1000),
            idle_threshold: Duration::from_millis(2000),
            completed_steps: Vec::new(),
            response_text: None,
            accumulated_text: None,
            action_seq: 0,
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
                self.is_thinking = false;
                self.completed_steps.clear();
                self.response_text = None;
                self.accumulated_text = None;
                self.action_seq = 0;
            }
            AgentEvent::Thinking { .. } => {
                self.state = DerivedState::Active;
                self.is_thinking = true;
            }
            AgentEvent::TextOutput { text, .. } => {
                self.state = DerivedState::Active;
                self.is_thinking = false;
                if !text.is_empty() {
                    self.accumulated_text = Some(text.clone());
                }
            }
            AgentEvent::ToolProposed {
                tool_id,
                tool_name,
                tool_context,
                question_data,
                ..
            } => {
                self.state = DerivedState::Active;
                self.is_thinking = false;
                self.last_context = tool_context.clone().or_else(|| Some(tool_name.clone()));
                self.pending_tool = Some(PendingTool {
                    tool_id: tool_id.clone(),
                    tool_name: tool_name.clone(),
                    context: tool_context.clone(),
                    question_data: question_data.clone(),
                    proposed_at: Instant::now(),
                });
                self.action_seq += 1;
            }
            AgentEvent::ToolCompleted {
                tool_id, is_error, ..
            } => {
                if let Some(pending) = self.pending_tool.take() {
                    if pending.tool_id == *tool_id {
                        self.completed_steps.push(CompletedStep {
                            tool_name: pending.tool_name,
                            context: pending.context,
                            ok: !is_error,
                        });
                        self.action_seq += 1;
                    } else {
                        self.pending_tool = Some(pending);
                    }
                }
                self.state = DerivedState::Active;
            }
            AgentEvent::ToolRejected { tool_id, .. } => {
                if let Some(pending) = self.pending_tool.take() {
                    if pending.tool_id == *tool_id {
                        self.completed_steps.push(CompletedStep {
                            tool_name: pending.tool_name,
                            context: pending.context,
                            ok: false,
                        });
                        self.action_seq += 1;
                    } else {
                        self.pending_tool = Some(pending);
                    }
                }
                self.state = DerivedState::Active;
            }
            AgentEvent::TurnComplete { response_text, .. } => {
                self.state = DerivedState::Idle;
                self.pending_tool = None;
                self.is_thinking = false;
                self.last_context = None;
                if let Some(text) = response_text {
                    self.response_text = Some(text.clone());
                } else if let Some(text) = self.accumulated_text.take() {
                    self.response_text = Some(text);
                }
                self.accumulated_text = None;
            }
            AgentEvent::SessionStarted { .. } => {
                self.state = DerivedState::Active;
                self.is_thinking = false;
            }
            AgentEvent::Progress => {}
        }

        self.to_pane_status()
    }

    pub fn tick(&mut self) -> PaneStatus {
        if let Some(ref pending) = self.pending_tool
            && pending.proposed_at.elapsed() >= self.approval_timeout
        {
            let reason = if pending.tool_name == "AskUserQuestion" || pending.tool_name == "AskUser"
            {
                WaitReason::Question
            } else {
                WaitReason::Approval
            };
            self.state = DerivedState::Waiting(reason);
        }

        if self.state == DerivedState::Active
            && self.last_event_time.elapsed() >= self.idle_threshold
        {
            self.state = DerivedState::Idle;
            self.pending_tool = None;
            self.is_thinking = false;
            if self.response_text.is_none()
                && let Some(text) = self.accumulated_text.take()
            {
                self.response_text = Some(text);
            }
        }

        self.to_pane_status()
    }

    pub fn last_tool_name(&self) -> Option<&str> {
        self.pending_tool.as_ref().map(|p| p.tool_name.as_str())
    }

    pub fn last_context(&self) -> Option<&str> {
        self.last_context.as_deref()
    }

    pub fn completed_steps(&self) -> &[CompletedStep] {
        &self.completed_steps
    }

    pub fn pending_tool_info(&self) -> Option<(&str, Option<&str>)> {
        self.pending_tool
            .as_ref()
            .map(|p| (p.tool_name.as_str(), p.context.as_deref()))
    }

    pub fn pending_question_data(&self) -> Option<&[QuestionData]> {
        self.pending_tool.as_ref()?.question_data.as_deref()
    }

    pub fn action_seq(&self) -> u64 {
        self.action_seq
    }

    pub fn response_text(&self) -> Option<&str> {
        self.response_text.as_deref()
    }

    pub fn take_response_text(&mut self) -> Option<String> {
        self.response_text.take()
    }

    pub fn clear_steps(&mut self) {
        self.completed_steps.clear();
        self.action_seq = 0;
    }

    pub fn reset(&mut self) {
        self.state = DerivedState::Idle;
        self.pending_tool = None;
        self.is_thinking = false;
        self.last_event_time = Instant::now();
        self.last_context = None;
        self.clear_steps();
        self.response_text = None;
        self.accumulated_text = None;
    }

    fn to_pane_status(&self) -> PaneStatus {
        let agent_type = match self.engine {
            Engine::ClaudeCode => AgentType::ClaudeCode,
            Engine::Codex => AgentType::Codex,
            Engine::Droid => AgentType::Droid,
            Engine::Terminal => return PaneStatus::Terminal(TerminalStatus::Active),
        };

        let status = match &self.state {
            DerivedState::Idle => AgentStatus::Idle,
            DerivedState::Waiting(reason) => AgentStatus::Waiting(*reason),
            DerivedState::Active => AgentStatus::Active(self.derive_activity()),
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
            Engine::Droid => {
                if self.is_thinking {
                    Activity::new(ActivityKind::Droid(DroidActivity::Thinking))
                } else {
                    Activity::new(ActivityKind::Droid(DroidActivity::Exploring))
                }
            }
            Engine::Terminal => Activity::new(ActivityKind::Generic(GenericActivity::Working)),
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
        Engine::Codex => Activity::new(ActivityKind::Codex(CodexActivity::Working)),
        Engine::Droid => {
            let kind = match tool_name {
                "Execute" => DroidActivity::ToolBash,
                "Edit" | "MultiEdit" => DroidActivity::ToolEdit,
                "Create" => DroidActivity::ToolCreate,
                "Read" | "LS" => DroidActivity::ToolRead,
                "Grep" => DroidActivity::ToolGrep,
                "Glob" => DroidActivity::ToolGlob,
                "Task" => DroidActivity::Subagent,
                "WebSearch" => DroidActivity::WebSearch,
                "FetchUrl" => DroidActivity::WebFetch,
                "TodoWrite" => DroidActivity::TodoUpdate,
                "AskUser" | "AskUserQuestion" => DroidActivity::AskUser,
                "Skill" => DroidActivity::SkillInvoke,
                "GenerateDroid" => DroidActivity::ToolCreate,
                "ExitSpecMode" => DroidActivity::Working,
                _ => DroidActivity::Exploring,
            };
            Activity::new(ActivityKind::Droid(kind))
        }
        Engine::Terminal => Activity::new(ActivityKind::Generic(GenericActivity::Working)),
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
            response_text: None,
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
            question_data: None,
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
            question_data: None,
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
            question_data: None,
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
            question_data: None,
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
            question_data: None,
        });
        assert!(status.is_active());
    }

    #[test]
    fn codex_task_complete() {
        let mut d = codex_deriver();
        d.process(&AgentEvent::UserMessage);
        let status = d.process(&AgentEvent::TurnComplete {
            engine: Engine::Codex,
            response_text: None,
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
            question_data: None,
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
            question_data: None,
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
            question_data: None,
        });
        assert!(d.last_context().is_some());
        d.process(&AgentEvent::TurnComplete {
            engine: Engine::ClaudeCode,
            response_text: None,
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

    #[test]
    fn text_output_captured_on_idle_timeout() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::TextOutput {
            engine: Engine::ClaudeCode,
            text: "**10 * 10 = 100**".into(),
        });
        assert!(d.response_text().is_none());

        std::thread::sleep(Duration::from_millis(110));
        d.tick();
        assert!(d.to_pane_status().is_idle());
        assert_eq!(d.response_text(), Some("**10 * 10 = 100**"));
    }

    #[test]
    fn text_output_captured_on_turn_complete_without_response() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::TextOutput {
            engine: Engine::ClaudeCode,
            text: "Here is the answer".into(),
        });
        d.process(&AgentEvent::TurnComplete {
            engine: Engine::ClaudeCode,
            response_text: None,
        });
        assert_eq!(d.response_text(), Some("Here is the answer"));
    }

    #[test]
    fn turn_complete_response_takes_priority_over_accumulated() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::TextOutput {
            engine: Engine::ClaudeCode,
            text: "streaming chunk".into(),
        });
        d.process(&AgentEvent::TurnComplete {
            engine: Engine::ClaudeCode,
            response_text: Some("final response".into()),
        });
        assert_eq!(d.response_text(), Some("final response"));
    }

    #[test]
    fn accumulated_text_cleared_on_user_message() {
        let mut d = claude_deriver();
        d.process(&AgentEvent::TextOutput {
            engine: Engine::ClaudeCode,
            text: "old response".into(),
        });
        d.process(&AgentEvent::UserMessage);
        d.process(&AgentEvent::TurnComplete {
            engine: Engine::ClaudeCode,
            response_text: None,
        });
        assert_eq!(d.response_text(), None);
    }

    #[test]
    fn pending_question_data_available_after_ask_user_question() {
        use crate::monitor::events::{QuestionData, QuestionOption};

        let mut d = claude_deriver();
        let qd = QuestionData {
            question: "Which DB?".into(),
            options: vec![
                QuestionOption {
                    label: "Postgres".into(),
                    description: Some("SQL".into()),
                },
                QuestionOption {
                    label: "Mongo".into(),
                    description: None,
                },
            ],
            multi_select: false,
        };
        d.process(&AgentEvent::ToolProposed {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            tool_name: "AskUserQuestion".into(),
            tool_context: Some("Which DB?".into()),
            question_data: Some(vec![qd.clone()]),
        });
        assert_eq!(d.pending_question_data(), Some([qd].as_slice()));

        // Cleared after tool completion
        d.process(&AgentEvent::ToolCompleted {
            engine: Engine::ClaudeCode,
            tool_id: "t1".into(),
            is_error: false,
        });
        assert_eq!(d.pending_question_data(), None);
    }

    // --- Droid activity mapping tests ---

    fn droid_deriver() -> StatusDeriver {
        StatusDeriver::new(Engine::Droid)
            .with_approval_timeout(Duration::from_millis(50))
            .with_idle_threshold(Duration::from_millis(100))
    }

    #[test]
    fn droid_thinking_event_shows_thinking() {
        let mut d = droid_deriver();
        d.process(&AgentEvent::Thinking {
            engine: Engine::Droid,
        });
        match d.to_pane_status() {
            PaneStatus::Agent {
                status: AgentStatus::Active(a),
                agent_type: AgentType::Droid,
            } => assert_eq!(a.label(), "Thinking"),
            other => panic!("Expected Droid Active(Thinking), got {:?}", other),
        }
    }

    #[test]
    fn droid_non_thinking_active_shows_exploring() {
        let mut d = droid_deriver();
        // UserMessage makes it active but not thinking
        d.process(&AgentEvent::UserMessage);
        match d.to_pane_status() {
            PaneStatus::Agent {
                status: AgentStatus::Active(a),
                agent_type: AgentType::Droid,
            } => assert_eq!(a.label(), "Exploring"),
            other => panic!("Expected Droid Active(Exploring), got {:?}", other),
        }
    }

    #[test]
    fn droid_thinking_cleared_by_text_output() {
        let mut d = droid_deriver();
        d.process(&AgentEvent::Thinking {
            engine: Engine::Droid,
        });
        assert_eq!(d.to_pane_status().label(), "Thinking");

        // TextOutput clears thinking state
        d.process(&AgentEvent::TextOutput {
            engine: Engine::Droid,
            text: "some output".into(),
        });
        match d.to_pane_status() {
            PaneStatus::Agent {
                status: AgentStatus::Active(a),
                ..
            } => assert_eq!(a.label(), "Exploring"),
            other => panic!("Expected Active(Exploring), got {:?}", other),
        }
    }

    #[test]
    fn droid_thinking_cleared_by_tool_proposed() {
        let mut d = droid_deriver();
        d.process(&AgentEvent::Thinking {
            engine: Engine::Droid,
        });
        assert_eq!(d.to_pane_status().label(), "Thinking");

        // ToolProposed clears thinking state and shows specific tool activity
        d.process(&AgentEvent::ToolProposed {
            engine: Engine::Droid,
            tool_id: "t1".into(),
            tool_name: "Execute".into(),
            tool_context: Some("cargo test".into()),
            question_data: None,
        });
        assert_eq!(d.to_pane_status().label(), "Bash");
    }

    #[test]
    fn droid_session_started_shows_exploring() {
        let mut d = droid_deriver();
        d.process(&AgentEvent::SessionStarted {
            engine: Engine::Droid,
            session_id: "abc".into(),
            model: None,
        });
        match d.to_pane_status() {
            PaneStatus::Agent {
                status: AgentStatus::Active(a),
                agent_type: AgentType::Droid,
            } => assert_eq!(a.label(), "Exploring"),
            other => panic!("Expected Droid Active(Exploring), got {:?}", other),
        }
    }

    #[test]
    fn droid_tool_activity_mapping() {
        let cases = vec![
            ("Execute", "Bash"),
            ("Edit", "Edit"),
            ("MultiEdit", "Edit"),
            ("Create", "Create"),
            ("Read", "Read"),
            ("LS", "Read"),
            ("Grep", "Grep"),
            ("Glob", "Glob"),
            ("Task", "Subagent"),
            ("WebSearch", "Web Search"),
            ("FetchUrl", "Web Fetch"),
            ("TodoWrite", "Todo"),
            ("AskUser", "Ask User"),
            ("AskUserQuestion", "Ask User"),
            ("Skill", "Skill"),
            ("GenerateDroid", "Create"),
            ("ExitSpecMode", "Working"),
            ("SomeUnknownTool", "Exploring"),
        ];
        for (tool_name, expected_label) in cases {
            let a = tool_name_to_activity(Engine::Droid, tool_name);
            assert_eq!(
                a.label(),
                expected_label,
                "Droid tool '{}' should map to '{}', got '{}'",
                tool_name,
                expected_label,
                a.label()
            );
        }
    }

    #[test]
    fn droid_tool_proposed_shows_specific_activity() {
        let mut d = droid_deriver();
        let status = d.process(&AgentEvent::ToolProposed {
            engine: Engine::Droid,
            tool_id: "t1".into(),
            tool_name: "Execute".into(),
            tool_context: Some("cargo build".into()),
            question_data: None,
        });
        match status {
            PaneStatus::Agent {
                status: AgentStatus::Active(a),
                agent_type: AgentType::Droid,
            } => assert_eq!(a.label(), "Bash"),
            other => panic!("Expected Droid Active(Bash), got {:?}", other),
        }
    }

    #[test]
    fn droid_grep_shows_grep_activity() {
        let mut d = droid_deriver();
        let status = d.process(&AgentEvent::ToolProposed {
            engine: Engine::Droid,
            tool_id: "t1".into(),
            tool_name: "Grep".into(),
            tool_context: Some("fn main".into()),
            question_data: None,
        });
        match status {
            PaneStatus::Agent {
                status: AgentStatus::Active(a),
                ..
            } => assert_eq!(a.label(), "Grep"),
            other => panic!("Expected Active(Grep), got {:?}", other),
        }
    }

    #[test]
    fn droid_subagent_shows_subagent_activity() {
        let mut d = droid_deriver();
        let status = d.process(&AgentEvent::ToolProposed {
            engine: Engine::Droid,
            tool_id: "t1".into(),
            tool_name: "Task".into(),
            tool_context: Some("research codebase".into()),
            question_data: None,
        });
        match status {
            PaneStatus::Agent {
                status: AgentStatus::Active(a),
                ..
            } => assert_eq!(a.label(), "Subagent"),
            other => panic!("Expected Active(Subagent), got {:?}", other),
        }
    }
}
