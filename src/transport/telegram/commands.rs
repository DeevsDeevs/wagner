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
        "/status" | "/s" => Some(RemoteCommand::FullStatus),

        "/tasks" | "/list" => Some(RemoteCommand::ListTasks),

        "/approve" | "/y" => {
            if rest.is_empty() {
                return None;
            }
            let (task_name, pane_id) = split_task_pane(rest);
            Some(RemoteCommand::Approve { task_name, pane_id })
        }

        "/reject" | "/n" => {
            if rest.is_empty() {
                return None;
            }
            let (task_name, pane_id) = split_task_pane(rest);
            Some(RemoteCommand::Reject { task_name, pane_id })
        }

        "/send" => {
            if rest.is_empty() {
                return None;
            }
            let mut parts = rest.splitn(2, |c: char| c.is_whitespace());
            let task_name = parts.next()?.to_string();
            let message = parts.next().unwrap_or("").trim().to_string();
            if message.is_empty() {
                return None;
            }
            Some(RemoteCommand::SendMessage {
                task_name,
                pane_id: None,
                message,
            })
        }

        "/output" | "/o" => {
            if rest.is_empty() {
                return None;
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

        "/help" | "/start" => Some(RemoteCommand::Help),

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
    fn parse_send_no_message() {
        assert!(parse_command("/send my-task").is_none());
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
        assert!(parse_command("/unknown").is_none());
        assert!(parse_command("hello").is_none());
    }

    #[test]
    fn parse_approve_no_task() {
        assert!(parse_command("/approve").is_none());
        assert!(parse_command("/approve  ").is_none());
    }
}
