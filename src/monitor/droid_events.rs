use super::events::{AgentEvent, QuestionData, QuestionOption};
use crate::model::Engine;

pub fn parse_droid_event(line: &str) -> Option<AgentEvent> {
    let obj: serde_json::Value = serde_json::from_str(line).ok()?;
    let event_type = obj.get("type")?.as_str()?;

    match event_type {
        "session_start" => parse_session_start(&obj),
        "message" => parse_message(&obj),
        "todo_state" => Some(AgentEvent::Progress),
        "session_end" => Some(AgentEvent::TurnComplete {
            engine: Engine::Droid,
            response_text: None,
        }),
        _ => None,
    }
}

fn parse_session_start(obj: &serde_json::Value) -> Option<AgentEvent> {
    let session_id = obj.get("id").and_then(|v| v.as_str())?;
    if session_id.is_empty() {
        return None;
    }
    let model = obj.get("model").and_then(|v| v.as_str()).map(String::from);

    Some(AgentEvent::SessionStarted {
        engine: Engine::Droid,
        session_id: session_id.to_string(),
        model,
    })
}

fn parse_message(obj: &serde_json::Value) -> Option<AgentEvent> {
    let role = obj.pointer("/message/role")?.as_str()?;

    match role {
        "user" => parse_user_message(obj),
        "assistant" => parse_assistant_message(obj),
        _ => None,
    }
}

fn parse_user_message(obj: &serde_json::Value) -> Option<AgentEvent> {
    let content = obj.pointer("/message/content")?;

    if let Some(arr) = content.as_array() {
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
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
                        engine: Engine::Droid,
                        tool_id,
                        reason,
                    });
                }

                return Some(AgentEvent::ToolCompleted {
                    engine: Engine::Droid,
                    tool_id,
                    is_error: false,
                });
            }
        }
    }

    Some(AgentEvent::UserMessage)
}

fn parse_assistant_message(obj: &serde_json::Value) -> Option<AgentEvent> {
    let stop_reason = obj.pointer("/message/stop_reason").and_then(|v| v.as_str());
    let content = obj.pointer("/message/content")?.as_array()?;

    match stop_reason {
        Some("end_turn") => {
            let response_text = extract_text_content(content);
            Some(AgentEvent::TurnComplete {
                engine: Engine::Droid,
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
                    engine: Engine::Droid,
                }),
                Some("text") => {
                    let text = extract_text_content(content);
                    Some(AgentEvent::TextOutput {
                        engine: Engine::Droid,
                        text,
                    })
                }
                Some("tool_use") => extract_tool_proposed(content),
                _ => None,
            }
        }
    }
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
    let question_data = extract_question_data(&tool_name, tool_block);
    Some(AgentEvent::ToolProposed {
        engine: Engine::Droid,
        tool_id,
        tool_name,
        tool_context,
        question_data,
    })
}

fn extract_question_data(
    tool_name: &str,
    tool_block: &serde_json::Value,
) -> Option<Vec<QuestionData>> {
    if tool_name != "AskUserQuestion" {
        return None;
    }
    let input = tool_block.get("input")?;
    let questions = input.get("questions")?.as_array()?;
    let parsed: Vec<QuestionData> = questions
        .iter()
        .filter_map(|q| {
            let question = q.get("question")?.as_str()?.to_string();
            let multi_select = q
                .get("multiSelect")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let options = q
                .get("options")?
                .as_array()?
                .iter()
                .filter_map(|o| {
                    Some(QuestionOption {
                        label: o.get("label")?.as_str()?.to_string(),
                        description: o
                            .get("description")
                            .and_then(|d| d.as_str())
                            .map(String::from),
                    })
                })
                .collect();
            Some(QuestionData {
                question,
                options,
                multi_select,
            })
        })
        .collect();
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
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
        "AskUserQuestion" | "AskUser" => {
            if let Some(questions) = input.get("questions").and_then(|v| v.as_array()) {
                let first_q = questions.first()?.get("question")?.as_str()?;
                return Some(first_q.to_string());
            }
            if let Some(q) = input.get("questionnaire").and_then(|v| v.as_str()) {
                let truncated: String = q.chars().take(100).collect();
                return Some(if q.chars().count() > 100 {
                    format!("{truncated}...")
                } else {
                    truncated
                });
            }
            None
        }
        "Bash" | "Execute" => {
            let cmd = input.get("command")?.as_str()?;
            if cmd.chars().count() > 100 {
                let truncated: String = cmd.chars().take(100).collect();
                Some(format!("{truncated}..."))
            } else {
                Some(cmd.to_string())
            }
        }
        "Read" | "Edit" | "MultiEdit" | "Write" | "Create" => {
            let path = input
                .get("file_path")
                .or(input.get("path"))
                .and_then(|v| v.as_str())?;
            Some(path.to_string())
        }
        "Grep" | "Glob" => {
            let val = input.get("pattern").or(input.get("patterns"))?;
            if let Some(s) = val.as_str() {
                Some(s.to_string())
            } else if let Some(arr) = val.as_array() {
                let joined: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                if joined.is_empty() {
                    None
                } else {
                    Some(joined.join(", "))
                }
            } else {
                None
            }
        }
        "WebSearch" => {
            let query = input.get("query")?.as_str()?;
            Some(query.to_string())
        }
        "FetchUrl" => {
            let url = input.get("url")?.as_str()?;
            Some(url.to_string())
        }
        "Task" => {
            let desc = input.get("description")?.as_str()?;
            Some(desc.to_string())
        }
        "Skill" => {
            let skill = input.get("skill")?.as_str()?;
            Some(skill.to_string())
        }
        "TodoWrite" => Some("updating todos".to_string()),
        "LS" => {
            let dir = input.get("directory_path")?.as_str()?;
            Some(dir.to_string())
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

    // --- session_start ---

    #[test]
    fn parse_session_start() {
        let line =
            r#"{"type":"session_start","id":"sess-abc-123","model":"claude-opus-4-20250514"}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStarted {
                engine: Engine::Droid,
                session_id: "sess-abc-123".to_string(),
                model: Some("claude-opus-4-20250514".to_string()),
            }
        );
    }

    #[test]
    fn parse_session_start_without_model() {
        let line = r#"{"type":"session_start","id":"sess-xyz"}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStarted {
                engine: Engine::Droid,
                session_id: "sess-xyz".to_string(),
                model: None,
            }
        );
    }

    #[test]
    fn parse_session_start_without_id() {
        let line = r#"{"type":"session_start","model":"opus"}"#;
        assert!(
            parse_droid_event(line).is_none(),
            "session_start with missing id should return None"
        );
    }

    #[test]
    fn parse_session_start_with_empty_id() {
        let line = r#"{"type":"session_start","id":"","model":"opus"}"#;
        assert!(
            parse_droid_event(line).is_none(),
            "session_start with empty id should return None"
        );
    }

    #[test]
    fn parse_session_start_with_null_id() {
        let line = r#"{"type":"session_start","id":null,"model":"opus"}"#;
        assert!(
            parse_droid_event(line).is_none(),
            "session_start with null id should return None"
        );
    }

    // --- message: user ---

    #[test]
    fn parse_user_message() {
        let line = r#"{"type":"message","message":{"role":"user","content":"hello"}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(event, AgentEvent::UserMessage);
    }

    #[test]
    fn parse_user_message_with_array_content() {
        let line = r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"hello world"}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(event, AgentEvent::UserMessage);
    }

    #[test]
    fn parse_tool_completed() {
        let line = r#"{"type":"message","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_123","is_error":false,"content":"output here"}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolCompleted {
                engine: Engine::Droid,
                tool_id: "toolu_123".to_string(),
                is_error: false,
            }
        );
    }

    #[test]
    fn parse_tool_rejected() {
        let line = r#"{"type":"message","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_456","is_error":true,"content":"User rejected tool use"}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolRejected {
                engine: Engine::Droid,
                tool_id: "toolu_456".to_string(),
                reason: "User rejected tool use".to_string(),
            }
        );
    }

    #[test]
    fn parse_tool_rejected_with_array_content() {
        let line = r#"{"type":"message","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_abc","is_error":true,"content":[{"type":"text","text":"Error: Permission denied"}]}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolRejected {
                engine: Engine::Droid,
                tool_id: "toolu_abc".to_string(),
                reason: "Error: Permission denied".to_string(),
            }
        );
    }

    // --- message: assistant ---

    #[test]
    fn parse_thinking() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":null,"content":[{"type":"thinking","thinking":"let me think..."}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::Thinking {
                engine: Engine::Droid
            }
        );
    }

    #[test]
    fn parse_text_output() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":null,"content":[{"type":"text","text":"Here is my answer"}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TextOutput {
                engine: Engine::Droid,
                text: "Here is my answer".into()
            }
        );
    }

    #[test]
    fn parse_assistant_turn_complete() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"Done!"}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TurnComplete {
                engine: Engine::Droid,
                response_text: Some("Done!".into())
            }
        );
    }

    #[test]
    fn parse_assistant_turn_complete_empty_text() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"end_turn","content":[]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TurnComplete {
                engine: Engine::Droid,
                response_text: None
            }
        );
    }

    #[test]
    fn parse_tool_proposed() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_123","name":"Bash","input":{"command":"ls"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Droid,
                tool_id: "toolu_123".to_string(),
                tool_name: "Bash".to_string(),
                tool_context: Some("ls".to_string()),
                question_data: None,
            }
        );
    }

    #[test]
    fn parse_tool_proposed_without_stop_reason() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":null,"content":[{"type":"tool_use","id":"toolu_789","name":"Read","input":{}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Droid,
                tool_id: "toolu_789".to_string(),
                tool_name: "Read".to_string(),
                tool_context: None,
                question_data: None,
            }
        );
    }

    #[test]
    fn parse_tool_proposed_with_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/src/main.rs"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Droid,
                tool_id: "t1".to_string(),
                tool_name: "Read".to_string(),
                tool_context: Some("/src/main.rs".to_string()),
                question_data: None,
            }
        );
    }

    #[test]
    fn parse_tool_proposed_edit_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/src/lib.rs","old_string":"a","new_string":"b"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("/src/lib.rs".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_tool_proposed_write_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/tmp/out.txt","content":"hello"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("/tmp/out.txt".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_tool_proposed_unknown_tool_no_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"SomeCustomTool","input":{"data":"stuff"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Droid,
                tool_id: "t1".to_string(),
                tool_name: "SomeCustomTool".to_string(),
                tool_context: None,
                question_data: None,
            }
        );
    }

    #[test]
    fn parse_tool_proposed_bash_truncation() {
        let long_cmd = "a".repeat(150);
        let line = format!(
            r#"{{"type":"message","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"{long_cmd}"}}}}]}}}}"#
        );
        let event = parse_droid_event(&line).unwrap();
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
    fn parse_tool_proposed_bash_multibyte_utf8() {
        let cmd = "🦀".repeat(120);
        let line = format!(
            r#"{{"type":"message","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"{cmd}"}}}}]}}}}"#
        );
        let event = parse_droid_event(&line).unwrap();
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
    fn parse_ask_user_question_extracts_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_q1","name":"AskUserQuestion","input":{"questions":[{"question":"Which database should we use?","header":"DB","options":[{"label":"Postgres","description":"SQL"},{"label":"Mongo","description":"NoSQL"}],"multiSelect":false}]}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Droid,
                tool_id: "toolu_q1".to_string(),
                tool_name: "AskUserQuestion".to_string(),
                tool_context: Some("Which database should we use?".to_string()),
                question_data: Some(vec![QuestionData {
                    question: "Which database should we use?".to_string(),
                    options: vec![
                        QuestionOption {
                            label: "Postgres".to_string(),
                            description: Some("SQL".to_string()),
                        },
                        QuestionOption {
                            label: "Mongo".to_string(),
                            description: Some("NoSQL".to_string()),
                        },
                    ],
                    multi_select: false,
                }]),
            }
        );
    }

    #[test]
    fn parse_ask_user_question_multiselect() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_q2","name":"AskUserQuestion","input":{"questions":[{"question":"Which features?","header":"Feat","options":[{"label":"Auth"},{"label":"DB","description":"Database"}],"multiSelect":true}]}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed {
                question_data: Some(qds),
                ..
            } => {
                assert_eq!(qds.len(), 1);
                let qd = &qds[0];
                assert_eq!(qd.question, "Which features?");
                assert!(qd.multi_select);
                assert_eq!(qd.options.len(), 2);
                assert_eq!(qd.options[0].label, "Auth");
                assert_eq!(qd.options[0].description, None);
                assert_eq!(qd.options[1].label, "DB");
                assert_eq!(qd.options[1].description, Some("Database".into()));
            }
            other => panic!("expected ToolProposed with question_data, got {other:?}"),
        }
    }

    #[test]
    fn non_question_tool_has_no_question_data() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { question_data, .. } => {
                assert!(question_data.is_none());
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    // --- todo_state ---

    #[test]
    fn parse_todo_state() {
        let line =
            r#"{"type":"todo_state","todos":[{"id":"1","text":"Fix bug","status":"completed"}]}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(event, AgentEvent::Progress);
    }

    #[test]
    fn parse_todo_state_empty() {
        let line = r#"{"type":"todo_state","todos":[]}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(event, AgentEvent::Progress);
    }

    // --- session_end ---

    #[test]
    fn parse_session_end() {
        let line = r#"{"type":"session_end","reason":"completed"}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TurnComplete {
                engine: Engine::Droid,
                response_text: None,
            }
        );
    }

    // --- unknown / malformed ---

    #[test]
    fn parse_unknown_type_returns_none() {
        let line = r#"{"type":"file-history-snapshot","snapshot":{}}"#;
        assert!(parse_droid_event(line).is_none());
    }

    #[test]
    fn parse_malformed_returns_none() {
        assert!(parse_droid_event("not json at all").is_none());
    }

    #[test]
    fn parse_empty_string_returns_none() {
        assert!(parse_droid_event("").is_none());
    }

    #[test]
    fn parse_missing_type_returns_none() {
        let line = r#"{"id":"sess-123","model":"opus"}"#;
        assert!(parse_droid_event(line).is_none());
    }

    #[test]
    fn parse_message_missing_role_returns_none() {
        let line = r#"{"type":"message","message":{"content":"hello"}}"#;
        assert!(parse_droid_event(line).is_none());
    }

    #[test]
    fn parse_message_unknown_role_returns_none() {
        let line = r#"{"type":"message","message":{"role":"system","content":"hello"}}"#;
        assert!(parse_droid_event(line).is_none());
    }

    #[test]
    fn parse_message_missing_message_field_returns_none() {
        let line = r#"{"type":"message","data":{"role":"user"}}"#;
        assert!(parse_droid_event(line).is_none());
    }

    // --- assistant content block combinations ---

    #[test]
    fn parse_assistant_thinking_and_tool_use() {
        // When thinking and tool_use are combined, tool_use takes priority via stop_reason
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"thinking","thinking":"analyzing..."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Droid,
                tool_id: "t1".to_string(),
                tool_name: "Bash".to_string(),
                tool_context: Some("cargo test".to_string()),
                question_data: None,
            }
        );
    }

    #[test]
    fn parse_assistant_multiple_text_blocks() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"Part 1"},{"type":"text","text":"Part 2"}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::TurnComplete {
                engine: Engine::Droid,
                response_text: Some("Part 1\n\nPart 2".into())
            }
        );
    }

    #[test]
    fn parse_assistant_empty_content() {
        let line =
            r#"{"type":"message","message":{"role":"assistant","stop_reason":null,"content":[]}}"#;
        assert!(parse_droid_event(line).is_none());
    }

    // --- read with path key (alternate form) ---

    #[test]
    fn parse_tool_proposed_read_with_path_key() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"path":"/src/main.rs"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Droid,
                tool_id: "t1".to_string(),
                tool_name: "Read".to_string(),
                tool_context: Some("/src/main.rs".to_string()),
                question_data: None,
            }
        );
    }

    // --- Droid-specific tool context extraction ---

    #[test]
    fn parse_execute_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Execute","input":{"command":"cargo build"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        assert_eq!(
            event,
            AgentEvent::ToolProposed {
                engine: Engine::Droid,
                tool_id: "t1".to_string(),
                tool_name: "Execute".to_string(),
                tool_context: Some("cargo build".to_string()),
                question_data: None,
            }
        );
    }

    #[test]
    fn parse_create_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Create","input":{"file_path":"/tmp/new.rs","content":"fn main() {}"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("/tmp/new.rs".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_multi_edit_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"MultiEdit","input":{"file_path":"/src/lib.rs","edits":[]}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("/src/lib.rs".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_grep_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Grep","input":{"pattern":"fn main"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("fn main".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_glob_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Glob","input":{"patterns":"**/*.rs"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("**/*.rs".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_web_search_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"WebSearch","input":{"query":"rust async patterns"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("rust async patterns".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_fetch_url_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"FetchUrl","input":{"url":"https://docs.rs"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("https://docs.rs".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_task_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Task","input":{"subagent_type":"worker","description":"research code","prompt":"explore the repo"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("research code".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_skill_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Skill","input":{"skill":"browser-navigation"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("browser-navigation".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_todo_write_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"TodoWrite","input":{"todos":"1. [completed] Done\n2. [in_progress] Working"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("updating todos".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_ls_tool_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"LS","input":{"directory_path":"/src"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed { tool_context, .. } => {
                assert_eq!(tool_context, Some("/src".to_string()));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }

    #[test]
    fn parse_ask_user_questionnaire_context() {
        let line = r#"{"type":"message","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"AskUser","input":{"questionnaire":"1. [question] Which approach?\n[option] A\n[option] B"}}]}}"#;
        let event = parse_droid_event(line).unwrap();
        match event {
            AgentEvent::ToolProposed {
                tool_name,
                tool_context,
                ..
            } => {
                assert_eq!(tool_name, "AskUser");
                assert!(tool_context.is_some());
                assert!(tool_context.unwrap().starts_with("1. [question]"));
            }
            other => panic!("expected ToolProposed, got {other:?}"),
        }
    }
}
