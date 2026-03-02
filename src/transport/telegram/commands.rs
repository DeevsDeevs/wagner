use crate::transport::{CoreCommand, PaneOutputMode};

#[derive(Debug)]
pub enum ParsedCommand {
    Core(CoreCommand),
    Focus {
        task_name: String,
        pane_name: Option<String>,
        sticky: bool,
    },
    Unfocus,
    UsageError {
        usage: &'static str,
    },
    Unknown {
        text: String,
    },
}

pub fn parse_command(text: &str) -> Option<ParsedCommand> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let mut parts = text.splitn(2, |c: char| c.is_whitespace());
    let raw_cmd = parts.next()?;
    let rest = parts.next().unwrap_or("").trim();

    // Strip @BotName suffix from commands (e.g. "/tasks@MyBot" → "/tasks")
    let cmd = raw_cmd.split('@').next().unwrap_or(raw_cmd);

    match cmd {
        "/status" | "/s" => {
            if rest.is_empty() {
                Some(ParsedCommand::Core(CoreCommand::FullStatus))
            } else {
                Some(ParsedCommand::Core(CoreCommand::TaskStatus {
                    task_name: rest.split_whitespace().next().unwrap().to_string(),
                }))
            }
        }

        "/tasks" | "/list" => Some(ParsedCommand::Core(CoreCommand::ListTasks)),

        "/approve" | "/y" => {
            if rest.is_empty() {
                return Some(ParsedCommand::Core(CoreCommand::Approve {
                    task_name: String::new(),
                    pane_name: None,
                }));
            }
            let (task_name, pane_name) = split_task_pane(rest);
            Some(ParsedCommand::Core(CoreCommand::Approve {
                task_name,
                pane_name,
            }))
        }

        "/reject" | "/n" => {
            if rest.is_empty() {
                return Some(ParsedCommand::UsageError {
                    usage: "/reject <task> [pane]",
                });
            }
            let (task_name, pane_name) = split_task_pane(rest);
            Some(ParsedCommand::Core(CoreCommand::Reject {
                task_name,
                pane_name,
            }))
        }

        "/send" => {
            if rest.is_empty() {
                return Some(ParsedCommand::UsageError {
                    usage: "/send <task> <message>",
                });
            }
            let mut parts = rest.splitn(2, |c: char| c.is_whitespace());
            let task_name = parts.next()?.to_string();
            let message = parts.next().unwrap_or("").trim().to_string();
            if message.is_empty() {
                return Some(ParsedCommand::UsageError {
                    usage: "/send <task> <message>",
                });
            }
            Some(ParsedCommand::Core(CoreCommand::SendMessage {
                task_name,
                pane_name: None,
                message,
            }))
        }

        "/output" | "/o" => {
            if rest.is_empty() {
                // Empty args — may be enriched by reply context in the adapter
                return Some(ParsedCommand::Core(CoreCommand::CaptureOutput {
                    task_name: String::new(),
                    pane_name: None,
                    lines: None,
                }));
            }
            let mut parts = rest.split_whitespace();
            let first = parts.next()?.to_string();
            // If the first arg is a number, treat it as lines (task from reply context)
            if let Ok(n) = first.parse::<usize>() {
                return Some(ParsedCommand::Core(CoreCommand::CaptureOutput {
                    task_name: String::new(),
                    pane_name: None,
                    lines: Some(n),
                }));
            }
            let lines = parts.next().and_then(|s| s.parse().ok());
            Some(ParsedCommand::Core(CoreCommand::CaptureOutput {
                task_name: first,
                pane_name: None,
                lines,
            }))
        }

        "/resume" => {
            if rest.is_empty() {
                return Some(ParsedCommand::UsageError {
                    usage: "/resume <task> [pane]",
                });
            }
            let (task_name, pane_name) = split_task_pane(rest);
            Some(ParsedCommand::Core(CoreCommand::Resume {
                task_name,
                pane_name,
            }))
        }

        "/add" => {
            if rest.is_empty() {
                return Some(ParsedCommand::UsageError {
                    usage: "/add <task> [name]",
                });
            }
            let (task_name, pane_name) = split_task_pane(rest);
            Some(ParsedCommand::Core(CoreCommand::AddPane {
                task_name,
                pane_name,
                agent: None,
            }))
        }

        "/rename" => {
            if rest.is_empty() {
                return Some(ParsedCommand::UsageError {
                    usage: "/rename <task> <old> <new>",
                });
            }
            let mut parts = rest.split_whitespace();
            let task_name = parts.next().unwrap_or("").to_string();
            let old_name = parts.next().map(String::from);
            let new_name = parts.next().map(String::from);
            match (old_name, new_name) {
                (Some(old), Some(new)) => Some(ParsedCommand::Core(CoreCommand::RenamePane {
                    task_name,
                    pane_name: old,
                    new_name: new,
                })),
                _ => Some(ParsedCommand::UsageError {
                    usage: "/rename <task> <old> <new>",
                }),
            }
        }

        "/kill" => {
            if rest.is_empty() {
                return Some(ParsedCommand::UsageError {
                    usage: "/kill <task> <pane>",
                });
            }
            let (task_name, pane_name) = split_task_pane(rest);
            match pane_name {
                Some(pane) => Some(ParsedCommand::Core(CoreCommand::KillPane {
                    task_name,
                    pane_name: pane,
                })),
                None => Some(ParsedCommand::UsageError {
                    usage: "/kill <task> <pane>",
                }),
            }
        }

        "/focus" => {
            if rest.is_empty() {
                return Some(ParsedCommand::UsageError {
                    usage: "/focus <task> [pane] [--sticky]",
                });
            }
            let sticky = rest.contains("--sticky");
            let clean = rest.replace("--sticky", "");
            let (task_name, pane_name) = split_task_pane(clean.trim());
            Some(ParsedCommand::Focus {
                task_name,
                pane_name,
                sticky,
            })
        }

        "/mode" => {
            if rest.is_empty() {
                return Some(ParsedCommand::UsageError {
                    usage: "/mode [task] [pane] <alerts|stream>",
                });
            }
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let mode_str = parts.last().copied().unwrap_or("");
            let mode = match mode_str {
                "alerts" => PaneOutputMode::Alerts,
                "stream" => PaneOutputMode::Stream,
                _ => {
                    return Some(ParsedCommand::UsageError {
                        usage: "/mode [task] [pane] <alerts|stream>",
                    });
                }
            };
            let (task_name, pane_name) = if parts.len() >= 3 {
                (parts[0].to_string(), Some(parts[1].to_string()))
            } else if parts.len() == 2 {
                (parts[0].to_string(), None)
            } else {
                // Just mode name — task will be inferred by the adapter
                (String::new(), None)
            };
            Some(ParsedCommand::Core(CoreCommand::SetPaneMode {
                task_name,
                pane_name,
                mode,
            }))
        }

        "/unfocus" => Some(ParsedCommand::Unfocus),

        "/help" | "/start" => Some(ParsedCommand::Core(CoreCommand::Help)),

        other if other.starts_with('/') => Some(ParsedCommand::Unknown {
            text: text.to_string(),
        }),

        _ => None,
    }
}

fn split_task_pane(s: &str) -> (String, Option<String>) {
    let mut parts = s.split_whitespace();
    let task_name = parts.next().unwrap_or("").to_string();
    let pane_name = parts.next().map(String::from);
    (task_name, pane_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status() {
        assert!(matches!(
            parse_command("/status"),
            Some(ParsedCommand::Core(CoreCommand::FullStatus))
        ));
        assert!(matches!(
            parse_command("/s"),
            Some(ParsedCommand::Core(CoreCommand::FullStatus))
        ));
    }

    #[test]
    fn parse_tasks() {
        assert!(matches!(
            parse_command("/tasks"),
            Some(ParsedCommand::Core(CoreCommand::ListTasks))
        ));
        assert!(matches!(
            parse_command("/list"),
            Some(ParsedCommand::Core(CoreCommand::ListTasks))
        ));
    }

    #[test]
    fn parse_approve() {
        match parse_command("/approve my-task") {
            Some(ParsedCommand::Core(CoreCommand::Approve {
                task_name,
                pane_name,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, None);
            }
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/y my-task %5") {
            Some(ParsedCommand::Core(CoreCommand::Approve {
                task_name,
                pane_name,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, Some("%5".into()));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_reject() {
        match parse_command("/reject my-task") {
            Some(ParsedCommand::Core(CoreCommand::Reject {
                task_name,
                pane_name,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_send() {
        match parse_command("/send my-task hello world") {
            Some(ParsedCommand::Core(CoreCommand::SendMessage {
                task_name, message, ..
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(message, "hello world");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_output() {
        match parse_command("/output my-task") {
            Some(ParsedCommand::Core(CoreCommand::CaptureOutput {
                task_name, lines, ..
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(lines, None);
            }
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/o my-task 50") {
            Some(ParsedCommand::Core(CoreCommand::CaptureOutput {
                task_name, lines, ..
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(lines, Some(50));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_help() {
        assert!(matches!(
            parse_command("/help"),
            Some(ParsedCommand::Core(CoreCommand::Help))
        ));
        assert!(matches!(
            parse_command("/start"),
            Some(ParsedCommand::Core(CoreCommand::Help))
        ));
    }

    #[test]
    fn parse_empty() {
        assert!(parse_command("").is_none());
        assert!(parse_command("   ").is_none());
    }

    #[test]
    fn parse_unknown() {
        match parse_command("/unknown") {
            Some(ParsedCommand::Unknown { text }) => assert_eq!(text, "/unknown"),
            other => panic!("unexpected: {other:?}"),
        }
        assert!(parse_command("hello").is_none());
    }

    #[test]
    fn parse_approve_no_task() {
        match parse_command("/approve") {
            Some(ParsedCommand::Core(CoreCommand::Approve { task_name, .. })) => {
                assert!(task_name.is_empty())
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_status_with_task() {
        match parse_command("/status my-task") {
            Some(ParsedCommand::Core(CoreCommand::TaskStatus { task_name })) => {
                assert_eq!(task_name, "my-task");
            }
            other => panic!("unexpected: {other:?}"),
        }
        match parse_command("/s agents") {
            Some(ParsedCommand::Core(CoreCommand::TaskStatus { task_name })) => {
                assert_eq!(task_name, "agents");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_send_no_message() {
        match parse_command("/send my-task") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_output_no_task() {
        // Empty /output now returns CaptureOutput with empty task (enriched by reply context)
        match parse_command("/output") {
            Some(ParsedCommand::Core(CoreCommand::CaptureOutput {
                task_name, lines, ..
            })) => {
                assert!(task_name.is_empty());
                assert_eq!(lines, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_output_lines_only() {
        match parse_command("/output 50") {
            Some(ParsedCommand::Core(CoreCommand::CaptureOutput {
                task_name, lines, ..
            })) => {
                assert!(task_name.is_empty());
                assert_eq!(lines, Some(50));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_resume() {
        match parse_command("/resume my-task") {
            Some(ParsedCommand::Core(CoreCommand::Resume {
                task_name,
                pane_name,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, None);
            }
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/resume my-task %5") {
            Some(ParsedCommand::Core(CoreCommand::Resume {
                task_name,
                pane_name,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, Some("%5".into()));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_resume_no_task() {
        match parse_command("/resume") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_reject_no_task() {
        match parse_command("/reject") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_add() {
        match parse_command("/add my-task") {
            Some(ParsedCommand::Core(CoreCommand::AddPane {
                task_name,
                pane_name,
                agent,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, None);
                assert_eq!(agent, None);
            }
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/add my-task custom-name") {
            Some(ParsedCommand::Core(CoreCommand::AddPane {
                task_name,
                pane_name,
                agent,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, Some("custom-name".into()));
                assert_eq!(agent, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_add_no_task() {
        match parse_command("/add") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_rename() {
        match parse_command("/rename my-task api backend") {
            Some(ParsedCommand::Core(CoreCommand::RenamePane {
                task_name,
                pane_name,
                new_name,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, "api");
                assert_eq!(new_name, "backend");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_rename_missing_args() {
        match parse_command("/rename my-task api") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/rename my-task") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/rename") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_kill() {
        match parse_command("/kill my-task api") {
            Some(ParsedCommand::Core(CoreCommand::KillPane {
                task_name,
                pane_name,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, "api");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_kill_missing_pane() {
        match parse_command("/kill my-task") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/kill") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_bot_username_suffix() {
        assert!(matches!(
            parse_command("/tasks@MyBot"),
            Some(ParsedCommand::Core(CoreCommand::ListTasks))
        ));
        assert!(matches!(
            parse_command("/status@MyBot"),
            Some(ParsedCommand::Core(CoreCommand::FullStatus))
        ));
        match parse_command("/status@MyBot my-task") {
            Some(ParsedCommand::Core(CoreCommand::TaskStatus { task_name })) => {
                assert_eq!(task_name, "my-task");
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(matches!(
            parse_command("/help@MyBot"),
            Some(ParsedCommand::Core(CoreCommand::Help))
        ));
    }

    #[test]
    fn parse_mode() {
        match parse_command("/mode my-task stream") {
            Some(ParsedCommand::Core(CoreCommand::SetPaneMode {
                task_name,
                pane_name,
                mode,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, None);
                assert_eq!(mode, PaneOutputMode::Stream);
            }
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/mode my-task api alerts") {
            Some(ParsedCommand::Core(CoreCommand::SetPaneMode {
                task_name,
                pane_name,
                mode,
            })) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_name, Some("api".into()));
                assert_eq!(mode, PaneOutputMode::Alerts);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_mode_infer_task() {
        match parse_command("/mode stream") {
            Some(ParsedCommand::Core(CoreCommand::SetPaneMode {
                task_name,
                pane_name,
                mode,
            })) => {
                assert!(task_name.is_empty());
                assert_eq!(pane_name, None);
                assert_eq!(mode, PaneOutputMode::Stream);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_mode_no_args() {
        match parse_command("/mode") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_mode_invalid_mode() {
        match parse_command("/mode my-task foo") {
            Some(ParsedCommand::UsageError { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
