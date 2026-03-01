use crate::monitor::status::{PaneStatus, SessionAggregateStatus, WaitReason};
use crate::transport::{CommandResponse, TransportEvent};

pub fn render_event(event: &TransportEvent) -> String {
    match event {
        TransportEvent::NeedsAttention {
            task_name,
            pane_id: _,
            pane_title,
            reason,
            output_tail,
        } => {
            let task = escape(task_name);
            let title = escape(pane_title);
            let reason_label = escape(reason.label());
            let tail = if output_tail.is_empty() {
                String::new()
            } else {
                format!("\n\n```\n{}\n```", escape(output_tail))
            };
            let hint = match reason {
                WaitReason::Question | WaitReason::Input => {
                    String::from("_Reply to this message with your answer_")
                }
                WaitReason::Approval | WaitReason::Permission => {
                    format!("/approve {task}  /reject {task}")
                }
            };
            format!(
                "\u{1F534} *{task}* \\| {title} — Waiting: {reason_label}{tail}\n\n{hint}"
            )
        }

        TransportEvent::AgentIdle {
            task_name,
            pane_id: _,
            pane_title,
            output_tail,
        } => {
            let task = escape(task_name);
            let title = escape(pane_title);
            let tail = if output_tail.is_empty() {
                String::new()
            } else {
                format!("\n\n```\n{}\n```", escape(output_tail))
            };
            format!("\u{26AA} *{task}* \\| {title} — Idle{tail}")
        }

        TransportEvent::AgentWorking {
            task_name,
            pane_id: _,
            pane_title,
            activity,
        } => {
            let task = escape(task_name);
            let title = escape(pane_title);
            let act = escape(activity);
            format!("\u{1F7E2} *{task}* \\| {title} — {act}")
        }

        TransportEvent::SessionStatusChanged {
            task_name,
            status,
        } => {
            let icon = status_icon(status);
            let task = escape(task_name);
            let label = escape(status.label());
            format!("{icon} *{task}* — {label}")
        }

        TransportEvent::DaemonStarted { tasks } => {
            let mut lines = vec![String::from("*Wagner Daemon Started*\n")];
            if tasks.is_empty() {
                lines.push(String::from("No tasks found\\."));
            } else {
                for t in tasks {
                    let name = escape(&t.name);
                    lines.push(format!(
                        "  {} — {} repos, {} panes",
                        name, t.repo_count, t.pane_count
                    ));
                }
            }
            lines.join("\n")
        }

        TransportEvent::DaemonStopping => String::from("*Wagner Daemon Stopping*"),
    }
}

pub fn render_response(response: &CommandResponse) -> String {
    match response {
        CommandResponse::TaskList { tasks } => {
            if tasks.is_empty() {
                return String::from("No tasks found\\.");
            }
            let mut lines = vec![String::from("*Tasks*\n")];
            for (summary, status) in tasks {
                let icon = status_icon(status);
                let name = escape(&summary.name);
                let label = escape(status.label());
                lines.push(format!(
                    "{icon} {name} — {} repos, {label}",
                    summary.repo_count
                ));
            }
            lines.join("\n")
        }

        CommandResponse::Status { task_name, panes } => {
            let task = escape(task_name);
            let mut lines = vec![format!("*{task}*\n")];
            if panes.is_empty() {
                lines.push(String::from("  No panes\\."));
            } else {
                for (title, status) in panes {
                    let icon = pane_icon(status);
                    let title = escape(title);
                    let label = escape(&status.label());
                    lines.push(format!("  {icon} {title} — {label}"));
                }
            }
            lines.join("\n")
        }

        CommandResponse::FullStatus { tasks } => {
            if tasks.is_empty() {
                return String::from("No tasks found\\.");
            }
            let mut lines = vec![String::from("*Wagner Status*\n")];
            for (summary, status, panes) in tasks {
                let icon = status_icon(status);
                let name = escape(&summary.name);
                let label = escape(status.label());
                lines.push(format!(
                    "{icon} *{name}* — {} repos, {label}",
                    summary.repo_count
                ));
                for (title, pane_status) in panes {
                    let picon = pane_icon(pane_status);
                    let title = escape(title);
                    let plabel = escape(&pane_status.label());
                    lines.push(format!("  {picon} {title} — {plabel}"));
                }
            }
            lines.join("\n")
        }

        CommandResponse::Output {
            task_name,
            pane_id: _,
            content,
        } => {
            let task = escape(task_name);
            if content.is_empty() {
                format!("*{task}* — no output")
            } else {
                format!("*{task}*\n\n```\n{}\n```", escape(content))
            }
        }

        CommandResponse::Confirmation { message } => escape(message),

        CommandResponse::Error { message } => {
            format!("\u{274C} {}", escape(message))
        }

        CommandResponse::HelpText => String::from(
            "*Wagner Remote Commands*\n\n\
             /status, /s — Full status overview\n\
             /tasks — List all tasks\n\
             /approve <task>, /y <task> — Approve waiting pane\n\
             /reject <task>, /n <task> — Reject waiting pane\n\
             /send <task> <msg> — Send message to pane\n\
             /output <task> \\[N\\] — Capture pane output\n\
             /help — Show this message",
        ),
    }
}

fn status_icon(status: &SessionAggregateStatus) -> &'static str {
    match status {
        SessionAggregateStatus::NeedsAttention => "\u{1F534}",
        SessionAggregateStatus::Working => "\u{1F7E2}",
        SessionAggregateStatus::Idle => "\u{26AA}",
        SessionAggregateStatus::Empty => "\u{26AB}",
    }
}

fn pane_icon(status: &PaneStatus) -> char {
    status.icon()
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 4);
    for c in s.chars() {
        if ESCAPE_CHARS.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

const ESCAPE_CHARS: &[char] = &[
    '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TaskSummary;

    #[test]
    fn escape_special_chars() {
        assert_eq!(escape("hello_world"), "hello\\_world");
        assert_eq!(escape("test.rs"), "test\\.rs");
        assert_eq!(escape("a*b"), "a\\*b");
    }

    #[test]
    fn escape_plain_text() {
        assert_eq!(escape("hello"), "hello");
        assert_eq!(escape("abc123"), "abc123");
    }

    #[test]
    fn render_help() {
        let text = render_response(&CommandResponse::HelpText);
        assert!(text.contains("Wagner Remote Commands"));
        assert!(text.contains("/status"));
    }

    #[test]
    fn render_empty_tasks() {
        let text = render_response(&CommandResponse::TaskList { tasks: vec![] });
        assert!(text.contains("No tasks"));
    }

    #[test]
    fn render_confirmation() {
        let text = render_response(&CommandResponse::Confirmation {
            message: "Approved my-task".into(),
        });
        assert!(text.contains("Approved my\\-task"));
    }

    #[test]
    fn render_error() {
        let text = render_response(&CommandResponse::Error {
            message: "not found".into(),
        });
        assert!(text.contains("not found"));
    }

    #[test]
    fn render_daemon_started() {
        let text = render_event(&TransportEvent::DaemonStarted {
            tasks: vec![TaskSummary {
                name: "my-task".into(),
                repo_count: 2,
                pane_count: 3,
            }],
        });
        assert!(text.contains("Wagner Daemon Started"));
        assert!(text.contains("my\\-task"));
        assert!(text.contains("2 repos"));
    }

    #[test]
    fn render_needs_attention_approval() {
        let text = render_event(&TransportEvent::NeedsAttention {
            task_name: "my-task".into(),
            pane_id: "%5".into(),
            pane_title: "repo1".into(),
            reason: crate::monitor::status::WaitReason::Approval,
            output_tail: "last line".into(),
        });
        assert!(text.contains("my\\-task"));
        assert!(text.contains("Approval"));
        assert!(text.contains("/approve"));
        assert!(text.contains("/reject"));
        assert!(text.contains("last line"));
        assert!(!text.contains("Reply to this message"));
    }

    #[test]
    fn render_needs_attention_question() {
        let text = render_event(&TransportEvent::NeedsAttention {
            task_name: "my-task".into(),
            pane_id: "%5".into(),
            pane_title: "repo1".into(),
            reason: crate::monitor::status::WaitReason::Question,
            output_tail: "Which database?".into(),
        });
        assert!(text.contains("Question"));
        assert!(text.contains("Reply to this message"));
        assert!(!text.contains("/approve"));
    }

    #[test]
    fn render_needs_attention_input() {
        let text = render_event(&TransportEvent::NeedsAttention {
            task_name: "my-task".into(),
            pane_id: "%5".into(),
            pane_title: "repo1".into(),
            reason: crate::monitor::status::WaitReason::Input,
            output_tail: String::new(),
        });
        assert!(text.contains("Reply to this message"));
        assert!(!text.contains("/approve"));
    }

    #[test]
    fn render_needs_attention_permission() {
        let text = render_event(&TransportEvent::NeedsAttention {
            task_name: "my-task".into(),
            pane_id: "%5".into(),
            pane_title: "repo1".into(),
            reason: crate::monitor::status::WaitReason::Permission,
            output_tail: String::new(),
        });
        assert!(text.contains("/approve"));
        assert!(text.contains("/reject"));
        assert!(!text.contains("Reply to this message"));
    }
}
