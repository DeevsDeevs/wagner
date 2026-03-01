use crate::transport::RemoteCommand;

pub fn parse_command(text: &str) -> Option<RemoteCommand> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    // Split into command and args
    let mut parts = text.splitn(2, |c: char| c.is_whitespace());
    let cmd = parts.next()?;
    let rest = parts.next().unwrap_or("").trim();

    match cmd {
        "/status" | "/s" => {
            if rest.is_empty() {
                Some(RemoteCommand::FullStatus)
            } else {
                Some(RemoteCommand::TaskStatus {
                    task_name: rest.split_whitespace().next().unwrap().to_string(),
                })
            }
        }

        "/tasks" | "/list" => Some(RemoteCommand::ListTasks),

        "/approve" | "/y" => {
            if rest.is_empty() {
                return Some(RemoteCommand::Approve {
                    task_name: String::new(),
                    pane_id: None,
                });
            }
            let (task_name, pane_id) = split_task_pane(rest);
            Some(RemoteCommand::Approve { task_name, pane_id })
        }

        "/reject" | "/n" => {
            if rest.is_empty() {
                return Some(RemoteCommand::Unknown {
                    text: text.to_string(),
                });
            }
            let (task_name, pane_id) = split_task_pane(rest);
            Some(RemoteCommand::Reject { task_name, pane_id })
        }

        "/send" => {
            if rest.is_empty() {
                return Some(RemoteCommand::Unknown {
                    text: text.to_string(),
                });
            }
            let mut parts = rest.splitn(2, |c: char| c.is_whitespace());
            let task_name = parts.next()?.to_string();
            let message = parts.next().unwrap_or("").trim().to_string();
            if message.is_empty() {
                return Some(RemoteCommand::Unknown {
                    text: text.to_string(),
                });
            }
            Some(RemoteCommand::SendMessage {
                task_name,
                pane_id: None,
                message,
            })
        }

        "/output" | "/o" => {
            if rest.is_empty() {
                return Some(RemoteCommand::Unknown {
                    text: text.to_string(),
                });
            }
            let mut parts = rest.split_whitespace();
            let task_name = parts.next()?.to_string();
            let lines = parts.next().and_then(|s| s.parse().ok());
            Some(RemoteCommand::CaptureOutput {
                task_name,
                pane_id: None,
                lines,
            })
        }

        "/resume" => {
            if rest.is_empty() {
                return Some(RemoteCommand::Unknown {
                    text: text.to_string(),
                });
            }
            let (task_name, pane_id) = split_task_pane(rest);
            Some(RemoteCommand::Resume { task_name, pane_id })
        }

        "/focus" => {
            if rest.is_empty() {
                return Some(RemoteCommand::Unknown {
                    text: text.to_string(),
                });
            }
            let sticky = rest.contains("--sticky");
            let clean = rest.replace("--sticky", "");
            let (task_name, pane_id) = split_task_pane(clean.trim());
            Some(RemoteCommand::Focus {
                task_name,
                pane_id,
                sticky,
            })
        }

        "/unfocus" => Some(RemoteCommand::Unfocus),

        "/help" | "/start" => Some(RemoteCommand::Help),

        other if other.starts_with('/') => Some(RemoteCommand::Unknown {
            text: text.to_string(),
        }),

        _ => None,
    }
}

fn split_task_pane(s: &str) -> (String, Option<String>) {
    let mut parts = s.split_whitespace();
    let task_name = parts.next().unwrap_or("").to_string();
    let pane_id = parts.next().map(String::from);
    (task_name, pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status() {
        assert!(matches!(parse_command("/status"), Some(RemoteCommand::FullStatus)));
        assert!(matches!(parse_command("/s"), Some(RemoteCommand::FullStatus)));
    }

    #[test]
    fn parse_tasks() {
        assert!(matches!(parse_command("/tasks"), Some(RemoteCommand::ListTasks)));
        assert!(matches!(parse_command("/list"), Some(RemoteCommand::ListTasks)));
    }

    #[test]
    fn parse_approve() {
        match parse_command("/approve my-task") {
            Some(RemoteCommand::Approve { task_name, pane_id }) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_id, None);
            }
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/y my-task %5") {
            Some(RemoteCommand::Approve { task_name, pane_id }) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_id, Some("%5".into()));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_reject() {
        match parse_command("/reject my-task") {
            Some(RemoteCommand::Reject { task_name, pane_id }) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_id, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_send() {
        match parse_command("/send my-task hello world") {
            Some(RemoteCommand::SendMessage { task_name, message, .. }) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(message, "hello world");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_output() {
        match parse_command("/output my-task") {
            Some(RemoteCommand::CaptureOutput { task_name, lines, .. }) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(lines, None);
            }
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/o my-task 50") {
            Some(RemoteCommand::CaptureOutput { task_name, lines, .. }) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(lines, Some(50));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_help() {
        assert!(matches!(parse_command("/help"), Some(RemoteCommand::Help)));
        assert!(matches!(parse_command("/start"), Some(RemoteCommand::Help)));
    }

    #[test]
    fn parse_empty() {
        assert!(parse_command("").is_none());
        assert!(parse_command("   ").is_none());
    }

    #[test]
    fn parse_unknown() {
        match parse_command("/unknown") {
            Some(RemoteCommand::Unknown { text }) => assert_eq!(text, "/unknown"),
            other => panic!("unexpected: {other:?}"),
        }
        assert!(parse_command("hello").is_none());
    }

    #[test]
    fn parse_approve_no_task() {
        match parse_command("/approve") {
            Some(RemoteCommand::Approve { task_name, .. }) => assert!(task_name.is_empty()),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_status_with_task() {
        match parse_command("/status my-task") {
            Some(RemoteCommand::TaskStatus { task_name }) => {
                assert_eq!(task_name, "my-task");
            }
            other => panic!("unexpected: {other:?}"),
        }
        match parse_command("/s agents") {
            Some(RemoteCommand::TaskStatus { task_name }) => {
                assert_eq!(task_name, "agents");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_send_no_message() {
        match parse_command("/send my-task") {
            Some(RemoteCommand::Unknown { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_output_no_task() {
        match parse_command("/output") {
            Some(RemoteCommand::Unknown { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_resume() {
        match parse_command("/resume my-task") {
            Some(RemoteCommand::Resume { task_name, pane_id }) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_id, None);
            }
            other => panic!("unexpected: {other:?}"),
        }

        match parse_command("/resume my-task %5") {
            Some(RemoteCommand::Resume { task_name, pane_id }) => {
                assert_eq!(task_name, "my-task");
                assert_eq!(pane_id, Some("%5".into()));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_resume_no_task() {
        match parse_command("/resume") {
            Some(RemoteCommand::Unknown { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_reject_no_task() {
        match parse_command("/reject") {
            Some(RemoteCommand::Unknown { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
