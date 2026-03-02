use crate::monitor::status::{SessionAggregateStatus, WaitReason};
use crate::transport::{CoreEvent, CoreResponse, ProgressStep};

pub fn render_event(event: &CoreEvent) -> String {
    match event {
        CoreEvent::NeedsAttention {
            task_name,
            pane_name,
            pane_id: _,
            reason,
            output_tail,
        } => {
            let task = escape(task_name);
            let title = escape(pane_name);
            let reason_label = escape(reason.label());
            let tail = if output_tail.is_empty() {
                String::new()
            } else {
                format!("\n\n```\n{}\n```", escape_code(output_tail))
            };
            let hint = match reason {
                WaitReason::Question | WaitReason::Input => {
                    String::from("_Reply to this message with your answer_")
                }
                WaitReason::Approval | WaitReason::Permission => {
                    format!("/approve {task}  /reject {task}")
                }
            };
            format!("\u{1F534} *{task}* \\| {title} — Waiting: {reason_label}{tail}\n\n{hint}")
        }

        CoreEvent::AgentIdle {
            task_name,
            pane_name,
            pane_id: _,
            output_tail,
            response_text: _,
        } => {
            let task = escape(task_name);
            let title = escape(pane_name);
            let tail = if output_tail.is_empty() {
                String::new()
            } else {
                format!("\n\n```\n{}\n```", escape_code(output_tail))
            };
            format!("\u{26AA} *{task}* \\| {title} — Idle{tail}")
        }

        CoreEvent::AgentWorking {
            task_name,
            pane_name,
            pane_id: _,
            activity,
        } => {
            let task = escape(task_name);
            let title = escape(pane_name);
            let act = escape(activity);
            format!("\u{1F7E2} *{task}* \\| {title} — {act}")
        }

        CoreEvent::SessionStatusChanged { task_name, status } => {
            let icon = status_icon(status);
            let task = escape(task_name);
            let label = escape(status.label());
            format!("{icon} *{task}* — {label}")
        }

        CoreEvent::DaemonStarted { tasks } => {
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

        CoreEvent::AgentProgress { .. } => {
            // Rendered via render_progress() / render_progress_done() instead
            String::new()
        }

        CoreEvent::DaemonStopping => String::from("*Wagner Daemon Stopping*"),
    }
}

pub fn render_response(response: &CoreResponse) -> String {
    match response {
        CoreResponse::TaskList { tasks } => {
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

        CoreResponse::Status {
            task_name: _,
            summary,
            status,
            panes,
        } => {
            let icon = status_icon(status);
            let name = escape(&summary.name);
            let label = escape(status.label());
            let mut lines = vec![format!(
                "{icon} *{name}* — {} repos, {label}\n",
                summary.repo_count
            )];
            if panes.is_empty() {
                lines.push(String::from("  No panes\\."));
            } else {
                for (title, pane_status) in panes {
                    let picon = pane_status.icon();
                    let title = escape(title);
                    let plabel = escape(&pane_status.label());
                    lines.push(format!("  {picon} {title} — {plabel}"));
                }
            }
            lines.join("\n")
        }

        CoreResponse::FullStatus { tasks } => {
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
                    let picon = pane_status.icon();
                    let title = escape(title);
                    let plabel = escape(&pane_status.label());
                    lines.push(format!("  {picon} {title} — {plabel}"));
                }
            }
            lines.join("\n")
        }

        CoreResponse::Output {
            task_name,
            pane_name: _,
            content,
        } => {
            let task = escape(task_name);
            if content.is_empty() {
                format!("*{task}* — no output")
            } else {
                format!("*{task}*\n\n```\n{}\n```", escape_code(content))
            }
        }

        CoreResponse::Confirmation { message } => escape(message),

        CoreResponse::ModeChanged {
            task_name,
            pane_name,
            mode,
        } => {
            let task = escape(task_name);
            let pane = escape(pane_name);
            let mode_label = match mode {
                crate::transport::PaneOutputMode::Alerts => "Alerts",
                crate::transport::PaneOutputMode::Stream => "Stream",
            };
            format!("Mode set to *{mode_label}* for *{task}* \\| {pane}")
        }

        CoreResponse::Error { message } => {
            format!("\u{274C} {}", escape(message))
        }

        CoreResponse::PluginItems { plugin_id, items } => {
            if items.is_empty() {
                return format!("*{}* — no items", escape(plugin_id));
            }
            let mut lines = vec![format!("*{}*\n", escape(plugin_id))];
            for item in items {
                let name = escape(&item.name);
                let summary = if item.summary.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", escape(&item.summary))
                };
                lines.push(format!("  {name}{summary}"));
            }
            lines.join("\n")
        }

        CoreResponse::PluginDetail {
            plugin_id: _,
            detail,
        } => {
            let name = escape(&detail.item.name);
            if detail.content.is_empty() {
                format!("*{name}* — no content")
            } else {
                format!("*{name}*\n\n```\n{}\n```", escape_code(&detail.content))
            }
        }

        CoreResponse::HelpText => String::from(
            "*Wagner Remote Commands*\n\n\
             /status, /s — Full status overview\n\
             /status <task> — Task pane details\n\
             /tasks — List all tasks\n\
             /approve, /y — Approve waiting pane\n\
             /reject <task>, /n <task> — Reject waiting pane\n\
             /send <task> <msg> — Send message to pane\n\
             /output <task> \\[N\\] — Capture pane output\n\
             /resume <task> — Resume dead agent session\n\
             /add <task> \\[name\\] — Add pane to task\n\
             /rename <task> <old> <new> — Rename pane\n\
             /kill <task> <pane> — Kill pane\n\
             /mode <task> \\[pane\\] <alerts\\|stream> — Output mode\n\
             /focus <task> \\[pane\\] — Focus on task/pane\n\
             /unfocus — Exit focus mode\n\
             /help — Show this message\n\n\
             _Reply to a notification to send input directly\\._",
        ),
    }
}

pub fn render_progress(
    task_name: &str,
    pane_name: &str,
    steps: &[ProgressStep],
    pending: Option<&ProgressStep>,
    step_count: usize,
) -> String {
    let task = escape(task_name);
    let pane = escape(pane_name);
    let mut lines = vec![format!(
        "\u{1F7E2} *{task}* \\| {pane} — Working \u{00B7} step {step_count}\n"
    )];

    for step in steps {
        let icon = if step.ok { "\u{2713}" } else { "\u{2717}" };
        let ctx = step
            .context
            .as_deref()
            .map(|c| format!(" `{}`", escape_code(c)))
            .unwrap_or_default();
        lines.push(format!("{icon} {}{ctx}", escape(&step.tool_name)));
    }

    if let Some(p) = pending {
        let ctx = p
            .context
            .as_deref()
            .map(|c| format!(" `{}`", escape_code(c)))
            .unwrap_or_default();
        lines.push(format!("\u{25B8} {}{ctx}", escape(&p.tool_name)));
    }

    lines.join("\n")
}

pub fn render_progress_done(
    task_name: &str,
    pane_name: &str,
    steps: &[ProgressStep],
    step_count: usize,
) -> String {
    let task = escape(task_name);
    let pane = escape(pane_name);
    let mut lines = vec![format!(
        "\u{26AA} *{task}* \\| {pane} — Done \u{00B7} {step_count} steps\n"
    )];

    for step in steps {
        let icon = if step.ok { "\u{2713}" } else { "\u{2717}" };
        let ctx = step
            .context
            .as_deref()
            .map(|c| format!(" `{}`", escape_code(c)))
            .unwrap_or_default();
        lines.push(format!("{icon} {}{ctx}", escape(&step.tool_name)));
    }

    lines.join("\n")
}

pub fn render_agent_response(task_name: &str, pane_name: &str, markdown: &str) -> String {
    let header = format!(
        "<b>{} | {}</b>\n\n",
        escape_html(task_name),
        escape_html(pane_name)
    );
    // Truncate markdown BEFORE conversion to avoid breaking HTML tags
    let max_md_len = 3500;
    let md = if markdown.len() > max_md_len {
        let mut end = max_md_len;
        while end > 0 && !markdown.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…(truncated)", &markdown[..end])
    } else {
        markdown.to_string()
    };
    let body = markdown_to_telegram_html(&md);
    format!("{header}{body}")
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn markdown_to_telegram_html(md: &str) -> String {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(md, opts);

    let mut out = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { .. }) => out.push_str("<b>"),
            Event::End(TagEnd::Heading(_)) => out.push_str("</b>\n"),
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => out.push_str("\n\n"),
            Event::Start(Tag::Emphasis) => out.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => out.push_str("</i>"),
            Event::Start(Tag::Strong) => out.push_str("<b>"),
            Event::End(TagEnd::Strong) => out.push_str("</b>"),
            Event::Start(Tag::Strikethrough) => out.push_str("<s>"),
            Event::End(TagEnd::Strikethrough) => out.push_str("</s>"),
            Event::Start(Tag::Link { dest_url, .. }) => {
                out.push_str(&format!("<a href=\"{}\">", escape_html(&dest_url)));
            }
            Event::End(TagEnd::Link) => out.push_str("</a>"),
            Event::Start(Tag::CodeBlock(kind)) => match kind {
                CodeBlockKind::Fenced(lang) if !lang.is_empty() => {
                    out.push_str(&format!(
                        "<pre><code class=\"language-{}\">",
                        escape_html(&lang)
                    ));
                }
                _ => out.push_str("<pre><code>"),
            },
            Event::End(TagEnd::CodeBlock) => {
                out.push_str("</code></pre>\n");
            }
            Event::Start(Tag::List(None)) => {}
            Event::End(TagEnd::List(false)) => out.push('\n'),
            Event::Start(Tag::List(Some(_))) => {}
            Event::End(TagEnd::List(true)) => out.push('\n'),
            Event::Start(Tag::Item) => out.push_str("• "),
            Event::End(TagEnd::Item) => out.push('\n'),
            Event::Code(code) => {
                out.push_str("<code>");
                out.push_str(&escape_html(&code));
                out.push_str("</code>");
            }
            Event::Text(text) => {
                out.push_str(&escape_html(&text));
            }
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push('\n'),
            _ => {}
        }
    }

    while out.ends_with('\n') {
        out.pop();
    }
    out
}

fn status_icon(status: &SessionAggregateStatus) -> &'static str {
    match status {
        SessionAggregateStatus::NeedsAttention => "\u{1F534}",
        SessionAggregateStatus::Working => "\u{1F7E2}",
        SessionAggregateStatus::Idle => "\u{26AA}",
        SessionAggregateStatus::Empty => "\u{26AB}",
    }
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

fn escape_code(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for c in s.chars() {
        if c == '`' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

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
    fn escape_code_only_backtick_and_backslash() {
        assert_eq!(escape_code("hello_world"), "hello_world");
        assert_eq!(escape_code("test.rs"), "test.rs");
        assert_eq!(escape_code("a*b"), "a*b");
        assert_eq!(escape_code("error[E0308]"), "error[E0308]");
        assert_eq!(escape_code("path\\to\\file"), "path\\\\to\\\\file");
        assert_eq!(escape_code("use `foo`"), "use \\`foo\\`");
    }

    #[test]
    fn escape_plain_text() {
        assert_eq!(escape("hello"), "hello");
        assert_eq!(escape("abc123"), "abc123");
    }

    #[test]
    fn render_help() {
        let text = render_response(&CoreResponse::HelpText);
        assert!(text.contains("Wagner Remote Commands"));
        assert!(text.contains("/status"));
        assert!(text.contains("/add"));
        assert!(text.contains("/rename"));
        assert!(text.contains("/kill"));
    }

    #[test]
    fn render_empty_tasks() {
        let text = render_response(&CoreResponse::TaskList { tasks: vec![] });
        assert!(text.contains("No tasks"));
    }

    #[test]
    fn render_confirmation() {
        let text = render_response(&CoreResponse::Confirmation {
            message: "Approved my-task".into(),
        });
        assert!(text.contains("Approved my\\-task"));
    }

    #[test]
    fn render_error() {
        let text = render_response(&CoreResponse::Error {
            message: "not found".into(),
        });
        assert!(text.contains("not found"));
    }

    #[test]
    fn render_daemon_started() {
        let text = render_event(&CoreEvent::DaemonStarted {
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
        let text = render_event(&CoreEvent::NeedsAttention {
            task_name: "my-task".into(),
            pane_name: "repo1".into(),
            pane_id: "%5".into(),
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
        let text = render_event(&CoreEvent::NeedsAttention {
            task_name: "my-task".into(),
            pane_name: "repo1".into(),
            pane_id: "%5".into(),
            reason: crate::monitor::status::WaitReason::Question,
            output_tail: "Which database?".into(),
        });
        assert!(text.contains("Question"));
        assert!(text.contains("Reply to this message"));
        assert!(!text.contains("/approve"));
    }

    #[test]
    fn render_needs_attention_input() {
        let text = render_event(&CoreEvent::NeedsAttention {
            task_name: "my-task".into(),
            pane_name: "repo1".into(),
            pane_id: "%5".into(),
            reason: crate::monitor::status::WaitReason::Input,
            output_tail: String::new(),
        });
        assert!(text.contains("Reply to this message"));
        assert!(!text.contains("/approve"));
    }

    #[test]
    fn render_needs_attention_permission() {
        let text = render_event(&CoreEvent::NeedsAttention {
            task_name: "my-task".into(),
            pane_name: "repo1".into(),
            pane_id: "%5".into(),
            reason: crate::monitor::status::WaitReason::Permission,
            output_tail: String::new(),
        });
        assert!(text.contains("/approve"));
        assert!(text.contains("/reject"));
        assert!(!text.contains("Reply to this message"));
    }
}
