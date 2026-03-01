use crate::model::Engine;

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    SessionStarted {
        engine: Engine,
        session_id: String,
        model: Option<String>,
    },
    Thinking {
        engine: Engine,
    },
    ToolProposed {
        engine: Engine,
        tool_id: String,
        tool_name: String,
        tool_context: Option<String>,
    },
    ToolCompleted {
        engine: Engine,
        tool_id: String,
        is_error: bool,
    },
    ToolRejected {
        engine: Engine,
        tool_id: String,
        reason: String,
    },
    TextOutput {
        engine: Engine,
    },
    TurnComplete {
        engine: Engine,
    },
    UserMessage,
    Progress,
}
