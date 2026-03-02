use super::events::AgentEvent;
use crate::model::Engine;

pub fn parse_codex_event(line: &str) -> Option<AgentEvent> {
    let obj: serde_json::Value = serde_json::from_str(line).ok()?;
    let event_type = obj.get("type")?.as_str()?;

    match event_type {
        "session_meta" => parse_session_meta(&obj),
        "response_item" => parse_response_item(&obj),
        "event_msg" => parse_event_msg(&obj),
        _ => None,
    }
}

fn parse_session_meta(obj: &serde_json::Value) -> Option<AgentEvent> {
    let session_id = obj
        .pointer("/payload/id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let model = obj
        .pointer("/payload/model")
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(AgentEvent::SessionStarted {
        engine: Engine::Codex,
        session_id,
        model,
    })
}

fn parse_response_item(obj: &serde_json::Value) -> Option<AgentEvent> {
    let payload_type = obj.pointer("/payload/type")?.as_str()?;

    match payload_type {
        "reasoning" => Some(AgentEvent::Thinking {
            engine: Engine::Codex,
        }),
        "message" => {
            let text = obj
                .pointer("/payload/content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(AgentEvent::TextOutput {
                engine: Engine::Codex,
                text,
            })
        }
        "function_call" | "custom_tool_call" => {
            let call_id = obj
                .pointer("/payload/call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = obj
                .pointer("/payload/name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(AgentEvent::ToolProposed {
                engine: Engine::Codex,
                tool_id: call_id,
                tool_name: name,
                tool_context: None,
            })
        }
        "function_call_output" | "custom_tool_call_output" => {
            let call_id = obj
                .pointer("/payload/call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(AgentEvent::ToolCompleted {
                engine: Engine::Codex,
                tool_id: call_id,
                is_error: false,
            })
        }
        "web_search_call" => {
            let call_id = obj
                .pointer("/payload/id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(AgentEvent::ToolProposed {
                engine: Engine::Codex,
                tool_id: call_id,
                tool_name: "web_search".to_string(),
                tool_context: None,
            })
        }
        _ => None,
    }
}

fn parse_event_msg(obj: &serde_json::Value) -> Option<AgentEvent> {
    let payload_type = obj.pointer("/payload/type")?.as_str()?;

    match payload_type {
        "task_complete" => {
            let response_text = obj
                .pointer("/payload/last_agent_message")
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(AgentEvent::TurnComplete {
                engine: Engine::Codex,
                response_text,
            })
        }
        "turn_aborted" => Some(AgentEvent::TurnComplete {
            engine: Engine::Codex,
            response_text: None,
        }),
        "user_message" => Some(AgentEvent::UserMessage),
        "agent_reasoning" | "agent_message" | "token_count" | "item_completed"
        | "context_compacted" => Some(AgentEvent::Progress),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_session_meta() {
        let line =
            r#"{"type":"session_meta","payload":{"id":"thread-123","model":"o3","cwd":"/tmp"}}"#;
        let event = parse_codex_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStarted {
                engine: Engine::Codex,
                session_id: "thread-123".to_string(),
                model: Some("o3".to_string()),
            }
        );
    }

    #[test]
    fn parse_reasoning() {
        let line =
            r#"{"type":"response_item","payload":{"type":"reasoning","content":"thinking..."}}"#;
        let event = parse_codex_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::Thinking {
                engine: Engine::Codex
            }
        );
    }

    #[test]
    fn parse_function_call() {
        let line = r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","call_id":"call_123"}}"#;
        let event = parse_codex_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Codex,
                tool_id: "call_123".to_string(),
                tool_name: "exec_command".to_string(),
                tool_context: None,
            }
        );
    }

    #[test]
    fn parse_function_call_output() {
        let line = r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"call_123","output":"file1\nfile2"}}"#;
        let event = parse_codex_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolCompleted {
                engine: Engine::Codex,
                tool_id: "call_123".to_string(),
                is_error: false,
            }
        );
    }

    #[test]
    fn parse_task_complete() {
        let line = r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"1","last_agent_message":"Done"}}"#;
        let event = parse_codex_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TurnComplete {
                engine: Engine::Codex,
                response_text: Some("Done".into())
            }
        );
    }

    #[test]
    fn parse_turn_aborted() {
        let line =
            r#"{"type":"event_msg","payload":{"type":"turn_aborted","reason":"interrupted"}}"#;
        let event = parse_codex_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TurnComplete {
                engine: Engine::Codex,
                response_text: None
            }
        );
    }

    #[test]
    fn parse_user_message() {
        let line =
            r#"{"type":"event_msg","payload":{"type":"user_message","content":"fix the bug"}}"#;
        let event = parse_codex_event(line).unwrap();
        assert_eq!(event, AgentEvent::UserMessage);
    }

    #[test]
    fn parse_message_output() {
        let line = r#"{"type":"response_item","payload":{"type":"message","content":"Here is the result"}}"#;
        let event = parse_codex_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TextOutput {
                engine: Engine::Codex,
                text: "Here is the result".into()
            }
        );
    }

    #[test]
    fn parse_custom_tool_call() {
        let line = r#"{"type":"response_item","payload":{"type":"custom_tool_call","name":"my_tool","call_id":"call_456"}}"#;
        let event = parse_codex_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Codex,
                tool_id: "call_456".to_string(),
                tool_name: "my_tool".to_string(),
                tool_context: None,
            }
        );
    }

    #[test]
    fn parse_progress_events() {
        let line =
            r#"{"type":"event_msg","payload":{"type":"token_count","input":100,"output":50}}"#;
        assert_eq!(parse_codex_event(line).unwrap(), AgentEvent::Progress);

        let line = r#"{"type":"event_msg","payload":{"type":"agent_reasoning","content":"..."}}"#;
        assert_eq!(parse_codex_event(line).unwrap(), AgentEvent::Progress);
    }

    #[test]
    fn parse_unknown_returns_none() {
        let line = r#"{"type":"compacted","payload":{}}"#;
        assert!(parse_codex_event(line).is_none());
    }

    #[test]
    fn parse_malformed_returns_none() {
        assert!(parse_codex_event("{broken").is_none());
    }
}
