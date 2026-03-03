use serde::{Deserialize, Serialize};

use crate::model::Engine;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionData {
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

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
        question_data: Option<Vec<QuestionData>>,
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
        text: String,
    },
    TurnComplete {
        engine: Engine,
        response_text: Option<String>,
    },
    UserMessage,
    Progress,
}
