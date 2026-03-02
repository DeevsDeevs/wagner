use super::events::AgentEvent;
use crate::model::Engine;

pub fn parse_claude_event(line: &str) -> Option<AgentEvent> {
    let obj: serde_json::Value = serde_json::from_str(line).ok()?;
    let event_type = obj.get("type")?.as_str()?;

    match event_type {
        "user" => parse_user_event(&obj),
        "assistant" => parse_assistant_event(&obj),
        "system" => parse_system_event(&obj),
        "progress" => Some(AgentEvent::Progress),
        _ => None,
    }
}

fn parse_user_event(obj: &serde_json::Value) -> Option<AgentEvent> {
    let content = obj.pointer("/message/content")?;

    if let Some(arr) = content.as_array() {
        for block in arr {
            if block.get("type")?.as_str()? == "tool_result" {
                let tool_id = block
                    .get("tool_use_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let is_error = block
                    .get("is_error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if is_error {
                    let reason = extract_tool_result_text(block);
                    return Some(AgentEvent::ToolRejected {
                        engine: Engine::ClaudeCode,
                        tool_id,
                        reason,
                    });
                }

                return Some(AgentEvent::ToolCompleted {
                    engine: Engine::ClaudeCode,
                    tool_id,
                    is_error: false,
                });
            }
        }
    }

    Some(AgentEvent::UserMessage)
}

fn parse_assistant_event(obj: &serde_json::Value) -> Option<AgentEvent> {
    let stop_reason = obj.pointer("/message/stop_reason").and_then(|v| v.as_str());
    let content = obj.pointer("/message/content")?.as_array()?;

    match stop_reason {
        Some("end_turn") => {
            let response_text = extract_text_content(content);
            Some(AgentEvent::TurnComplete {
                engine: Engine::ClaudeCode,
                response_text: if response_text.is_empty() {
                    None
                } else {
                    Some(response_text)
                },
            })
        }
        Some("tool_use") => extract_tool_proposed(content),
        _ => {
            let first_type = content
                .first()
                .and_then(|b| b.get("type"))
                .and_then(|t| t.as_str());
            match first_type {
                Some("thinking") => Some(AgentEvent::Thinking {
                    engine: Engine::ClaudeCode,
                }),
                Some("text") => {
                    let text = extract_text_content(content);
                    Some(AgentEvent::TextOutput {
                        engine: Engine::ClaudeCode,
                        text,
                    })
                }
                Some("tool_use") => extract_tool_proposed(content),
                _ => None,
            }
        }
    }
}

fn parse_system_event(obj: &serde_json::Value) -> Option<AgentEvent> {
    let session_id = obj.get("sessionId").and_then(|v| v.as_str())?;
    let model = obj
        .pointer("/message/model")
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(AgentEvent::SessionStarted {
        engine: Engine::ClaudeCode,
        session_id: session_id.to_string(),
        model,
    })
}

fn extract_tool_proposed(content: &[serde_json::Value]) -> Option<AgentEvent> {
    let tool_block = content
        .iter()
        .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))?;
    let tool_id = tool_block
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tool_name = tool_block
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tool_context = extract_tool_context(&tool_name, tool_block);
    Some(AgentEvent::ToolProposed {
        engine: Engine::ClaudeCode,
        tool_id,
        tool_name,
        tool_context,
    })
}

fn extract_text_content(content: &[serde_json::Value]) -> String {
    content
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn extract_tool_context(tool_name: &str, tool_block: &serde_json::Value) -> Option<String> {
    let input = tool_block.get("input")?;
    match tool_name {
        "AskUserQuestion" => {
            let questions = input.get("questions")?.as_array()?;
            let first_q = questions.first()?.get("question")?.as_str()?;
            Some(first_q.to_string())
        }
        "Bash" => {
            let cmd = input.get("command")?.as_str()?;
            if cmd.chars().count() > 100 {
                let truncated: String = cmd.chars().take(100).collect();
                Some(format!("{truncated}..."))
            } else {
                Some(cmd.to_string())
            }
        }
        "Read" | "Edit" | "Write" => {
            let path = input
                .get("file_path")
                .or(input.get("path"))
                .and_then(|v| v.as_str())?;
            Some(path.to_string())
        }
        _ => None,
    }
}

fn extract_tool_result_text(block: &serde_json::Value) -> String {
    if let Some(s) = block.get("content").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(arr) = block.get("content").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                return text.to_string();
            }
        }
    }
    "Unknown rejection".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_message() {
        let line = r#"{"type":"user","message":{"role":"user","content":"hello"}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(event, AgentEvent::UserMessage);
    }

    #[test]
    fn parse_thinking() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":null,"content":[{"type":"thinking","thinking":"let me think..."}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::Thinking {
                engine: Engine::ClaudeCode
            }
        );
    }

    #[test]
    fn parse_text_output() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":null,"content":[{"type":"text","text":"Here is my answer"}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TextOutput {
                engine: Engine::ClaudeCode,
                text: "Here is my answer".into()
            }
        );
    }

    #[test]
    fn parse_turn_complete() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"Done!"}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TurnComplete {
                engine: Engine::ClaudeCode,
                response_text: Some("Done!".into())
            }
        );
    }

    #[test]
    fn parse_tool_proposed() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_123","name":"Bash","input":{"command":"ls"}}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::ClaudeCode,
                tool_id: "toolu_123".to_string(),
                tool_name: "Bash".to_string(),
                tool_context: Some("ls".to_string()),
            }
        );
    }

    #[test]
    fn parse_tool_completed() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_123","is_error":false,"content":"output here"}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolCompleted {
                engine: Engine::ClaudeCode,
                tool_id: "toolu_123".to_string(),
                is_error: false,
            }
        );
    }

    #[test]
    fn parse_tool_rejected() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_456","is_error":true,"content":"User rejected tool use"}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolRejected {
                engine: Engine::ClaudeCode,
                tool_id: "toolu_456".to_string(),
                reason: "User rejected tool use".to_string(),
            }
        );
    }

    #[test]
    fn parse_progress_event() {
        let line = r#"{"type":"progress","data":{}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(event, AgentEvent::Progress);
    }

    #[test]
    fn parse_unknown_type_returns_none() {
        let line = r#"{"type":"file-history-snapshot","snapshot":{}}"#;
        assert!(parse_claude_event(line).is_none());
    }

    #[test]
    fn parse_malformed_json_returns_none() {
        assert!(parse_claude_event("not json at all").is_none());
    }

    #[test]
    fn parse_tool_proposed_without_stop_reason() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":null,"content":[{"type":"tool_use","id":"toolu_789","name":"Read","input":{}}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::ClaudeCode,
                tool_id: "toolu_789".to_string(),
                tool_name: "Read".to_string(),
                tool_context: None,
            }
        );
    }

    #[test]
    fn parse_ask_user_question_extracts_context() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_q1","name":"AskUserQuestion","input":{"questions":[{"question":"Which database should we use?","header":"DB","options":[{"label":"Postgres","description":"SQL"},{"label":"Mongo","description":"NoSQL"}],"multiSelect":false}]}}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::ClaudeCode,
                tool_id: "toolu_q1".to_string(),
                tool_name: "AskUserQuestion".to_string(),
                tool_context: Some("Which database should we use?".to_string()),
            }
        );
    }

    #[test]
    fn extract_context_bash_truncation() {
        let long_cmd = "a".repeat(150);
        let line = format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"{long_cmd}"}}}}]}}}}"#
        );
        let event = parse_claude_event(&line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                let ctx = tool_context.unwrap();
                assert!(ctx.ends_with("..."));
                assert_eq!(ctx.len(), 103); // 100 chars + "..."
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn extract_context_read_with_file_path() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/src/main.rs"}}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::ClaudeCode,
                tool_id: "t1".to_string(),
                tool_name: "Read".to_string(),
                tool_context: Some("/src/main.rs".to_string()),
            }
        );
    }

    #[test]
    fn extract_context_edit_with_file_path() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/src/lib.rs","old_string":"a","new_string":"b"}}]}}"#;
        let event = parse_claude_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("/src/lib.rs".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn extract_context_unknown_tool_returns_none() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"WebSearch","input":{"query":"rust async"}}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::ClaudeCode,
                tool_id: "t1".to_string(),
                tool_name: "WebSearch".to_string(),
                tool_context: None,
            }
        );
    }

    #[test]
    fn extract_context_bash_multibyte_utf8() {
        // 120 emoji chars — exceeds 100 char limit, must not panic on truncation
        let cmd = "🦀".repeat(120);
        let line = format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"{cmd}"}}}}]}}}}"#
        );
        let event = parse_claude_event(&line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                let ctx = tool_context.unwrap();
                assert!(ctx.ends_with("..."));
                assert_eq!(ctx.chars().count(), 103); // 100 + "..."
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_tool_rejected_with_array_content() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_abc","is_error":true,"content":[{"type":"text","text":"Error: This command requires approval"}]}]}}"#;
        let event = parse_claude_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolRejected {
                engine: Engine::ClaudeCode,
                tool_id: "toolu_abc".to_string(),
                reason: "Error: This command requires approval".to_string(),
            }
        );
    }
}
