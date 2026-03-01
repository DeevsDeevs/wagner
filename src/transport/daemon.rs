use std::collections::HashMap;
use std::time::{Duration, Instant};

use tracing::{error, info};

use crate::agent::{ClaudeCodeDetector, CodexDetector};
use crate::config::Config;
use crate::error::WagnerError;
use crate::monitor::strip_ansi;
use crate::monitor::status::{PaneStatus, SessionAggregateStatus};
use crate::monitor::watcher::SessionWatcher;
use crate::monitor::StatusMonitor;
use crate::store::Store;
use crate::terminal::{PaneHandle, Terminal, Tmux, session_name_for_task};

use super::{
    CommandResponse, MessageRef, RemoteCommand, TaskSummary, Transport, TransportEvent,
};

struct DaemonState {
    watcher: SessionWatcher,
    terminal: Tmux,
    store: Store,
    config: Config,
    last_statuses: HashMap<String, PaneStatus>,
    last_session_statuses: HashMap<String, SessionAggregateStatus>,
    live_messages: HashMap<String, MessageRef>,
    message_to_pane: HashMap<i32, (String, String)>,
    session_stable_since: HashMap<String, (SessionAggregateStatus, Instant)>,
    startup_time: Instant,
}

pub struct LogTransport;

impl Transport for LogTransport {
    fn name(&self) -> &str {
        "log"
    }

    async fn send_event(
        &self,
        event: &TransportEvent,
    ) -> crate::Result<Option<MessageRef>> {
        info!(?event, "transport event");
        Ok(None)
    }

    async fn edit_message(
        &self,
        _msg_ref: &MessageRef,
        event: &TransportEvent,
    ) -> crate::Result<Option<MessageRef>> {
        info!(?event, "transport edit");
        Ok(None)
    }

    async fn send_response(
        &self,
        response: &CommandResponse,
        _reply_to: Option<&MessageRef>,
    ) -> crate::Result<Option<MessageRef>> {
        info!(?response, "transport response");
        Ok(None)
    }

    async fn poll_commands(&self) -> crate::Result<Vec<(RemoteCommand, MessageRef)>> {
        Ok(vec![])
    }
}

pub async fn run_daemon(config: Config) -> crate::Result<()> {
    let _telegram_config = config
        .daemon
        .telegram
        .as_ref()
        .ok_or_else(|| WagnerError::Transport("Telegram not configured".into()))?;

    let terminal = Tmux::with_config(config.terminal.clone());
    let store = Store::new(config.clone());

    let fallback = StatusMonitor::with_detectors(vec![
        Box::new(ClaudeCodeDetector::default()),
        Box::new(CodexDetector::default()),
    ]);
    let watcher = SessionWatcher::new(fallback, &config.monitor);

    #[cfg(feature = "telegram")]
    let transport = super::telegram::TelegramTransport::new(_telegram_config)?;
    #[cfg(not(feature = "telegram"))]
    let transport = LogTransport;

    let mut state = DaemonState {
        watcher,
        terminal,
        store,
        config: config.clone(),
        last_statuses: HashMap::new(),
        last_session_statuses: HashMap::new(),
        live_messages: HashMap::new(),
        message_to_pane: HashMap::new(),
        session_stable_since: HashMap::new(),
        startup_time: Instant::now(),
    };

    let tasks = state.store.list_tasks()?;
    let summaries: Vec<TaskSummary> = tasks
        .iter()
        .map(|t| TaskSummary {
            name: t.name.clone(),
            repo_count: t.repos.len(),
            pane_count: t.panes.len(),
        })
        .collect();

    transport
        .send_event(&TransportEvent::DaemonStarted {
            tasks: summaries,
        })
        .await?;

    for task in &tasks {
        let session_name = session_name_for_task(&task.name);
        state.watcher.track_task(task, &session_name);
    }

    info!(task_count = tasks.len(), "daemon started");

    let poll_interval = Duration::from_millis(config.daemon.poll_interval_ms);

    loop {
        if let Err(e) = daemon_tick(&mut state, &transport).await {
            error!(%e, "daemon tick error");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

async fn daemon_tick(
    state: &mut DaemonState,
    transport: &impl Transport,
) -> crate::Result<()> {
    // Reload tasks to pick up new ones
    let tasks = state.store.list_tasks()?;
    for task in &tasks {
        let session_name = session_name_for_task(&task.name);
        state.watcher.track_task(task, &session_name);
    }

    // Poll all sessions
    let mut all_sessions: Vec<(String, Vec<PaneHandle>)> = Vec::new();
    for task in &tasks {
        let session_name = session_name_for_task(&task.name);
        // session_exists() already calls session_name_for_task internally, so pass raw name
        if state.terminal.session_exists(&task.name).unwrap_or(false) {
            if let Ok(panes) = state.terminal.list_panes(
                &crate::terminal::SessionHandle(session_name.clone()),
            ) {
                all_sessions.push((session_name, panes));
            }
        }
    }

    for (session_name, panes) in &all_sessions {
        state
            .watcher
            .poll_active(&state.terminal, session_name, panes);
    }

    // Detect transitions and emit events
    for task in &tasks {
        let session_name = session_name_for_task(&task.name);

        // Check session aggregate status with debounce
        let session_status = state.watcher.get_session_status(&session_name);
        let last_emitted = state.last_session_statuses.get(&task.name);

        if last_emitted == Some(&session_status) {
            state.session_stable_since.remove(&task.name);
        } else {
            let now = Instant::now();
            let should_emit = match state.session_stable_since.get(&task.name) {
                Some((pending, since)) if *pending == session_status => {
                    since.elapsed() >= Duration::from_secs(1)
                        && state.startup_time.elapsed() >= Duration::from_secs(3)
                }
                _ => {
                    state.session_stable_since.insert(
                        task.name.clone(),
                        (session_status, now),
                    );
                    false
                }
            };

            if should_emit {
                state
                    .last_session_statuses
                    .insert(task.name.clone(), session_status);
                state.session_stable_since.remove(&task.name);
                transport
                    .send_event(&TransportEvent::SessionStatusChanged {
                        task_name: task.name.clone(),
                        status: session_status,
                    })
                    .await?;
            }
        }

        // Check per-pane transitions
        let session_panes = all_sessions
            .iter()
            .find(|(n, _)| n == &session_name)
            .map(|(_, p)| p.as_slice())
            .unwrap_or(&[]);

        for pane in session_panes {
            let pane_id = &pane.0;
            let pane_title = &pane.1;

            let current = state
                .watcher
                .get_pane_status(&session_name, pane_id)
                .cloned()
                .unwrap_or(PaneStatus::Unknown);

            let last = state.last_statuses.get(pane_id);
            if last == Some(&current) {
                continue;
            }

            let was_waiting = last.is_some_and(|s| s.is_waiting());
            let was_active = last.is_some_and(|s| s.is_active());
            let is_waiting = current.is_waiting();
            let is_active = current.is_active();
            let is_idle = current.is_idle();

            state.last_statuses.insert(pane_id.clone(), current.clone());

            if is_waiting && !was_waiting {
                let output_tail = state
                    .watcher
                    .get_pane_context(pane_id)
                    .unwrap_or_else(|| capture_tail(&state.terminal, pane, 5));
                let reason = match &current {
                    PaneStatus::Agent {
                        status: crate::monitor::status::AgentStatus::Waiting(r),
                        ..
                    } => *r,
                    _ => crate::monitor::status::WaitReason::Approval,
                };

                let msg_ref = transport
                    .send_event(&TransportEvent::NeedsAttention {
                        task_name: task.name.clone(),
                        pane_id: pane_id.clone(),
                        pane_title: pane_title.clone(),
                        reason,
                        output_tail,
                    })
                    .await?;

                if let Some(r) = msg_ref {
                    state.message_to_pane.insert(
                        r.message_id,
                        (task.name.clone(), pane_id.clone()),
                    );
                    state.live_messages.insert(pane_id.clone(), r);
                }
            } else if is_idle && was_active {
                let notify_idle = state.config.daemon.telegram.as_ref()
                    .is_some_and(|t| t.notify_idle);
                if notify_idle {
                    let output_tail = capture_tail(
                        &state.terminal,
                        pane,
                        state.config.daemon.telegram.as_ref()
                            .map(|t| t.default_output_lines)
                            .unwrap_or(30),
                    );
                    transport
                        .send_event(&TransportEvent::AgentIdle {
                            task_name: task.name.clone(),
                            pane_id: pane_id.clone(),
                            pane_title: pane_title.clone(),
                            output_tail,
                        })
                        .await?;
                }
            } else if is_active && !was_active {
                let activity = current.label();
                if let Some(msg_ref) = state.live_messages.remove(pane_id) {
                    state.message_to_pane.remove(&msg_ref.message_id);
                    transport
                        .edit_message(
                            &msg_ref,
                            &TransportEvent::AgentWorking {
                                task_name: task.name.clone(),
                                pane_id: pane_id.clone(),
                                pane_title: pane_title.clone(),
                                activity: activity.clone(),
                            },
                        )
                        .await?;
                }
            }
        }
    }

    // Poll and handle incoming commands
    let commands = transport.poll_commands().await?;
    for (cmd, msg_ref) in commands {
        let response = handle_command(state, &cmd, &tasks);
        transport
            .send_response(&response, Some(&msg_ref))
            .await?;
    }

    Ok(())
}

fn capture_tail(terminal: &Tmux, pane: &PaneHandle, lines: usize) -> String {
    terminal
        .capture(pane, lines)
        .map(|s| strip_ansi(&s))
        .unwrap_or_default()
}

fn handle_command(
    state: &mut DaemonState,
    cmd: &RemoteCommand,
    tasks: &[crate::model::Task],
) -> CommandResponse {
    match cmd {
        RemoteCommand::ListTasks => {
            let list: Vec<_> = tasks
                .iter()
                .map(|t| {
                    let session_name = session_name_for_task(&t.name);
                    let status = state.watcher.get_session_status(&session_name);
                    (
                        TaskSummary {
                            name: t.name.clone(),
                            repo_count: t.repos.len(),
                            pane_count: t.panes.len(),
                        },
                        status,
                    )
                })
                .collect();
            CommandResponse::TaskList { tasks: list }
        }

        RemoteCommand::TaskStatus { task_name } => {
            let session_name = session_name_for_task(task_name);
            let session_panes = state
                .terminal
                .list_panes(&crate::terminal::SessionHandle(session_name.clone()))
                .unwrap_or_default();

            let panes: Vec<_> = session_panes
                .iter()
                .map(|p| {
                    let status = state
                        .watcher
                        .get_pane_status(&session_name, &p.0)
                        .cloned()
                        .unwrap_or(PaneStatus::Unknown);
                    (p.1.clone(), status)
                })
                .collect();

            CommandResponse::Status {
                task_name: task_name.clone(),
                panes,
            }
        }

        RemoteCommand::FullStatus => {
            let list: Vec<_> = tasks
                .iter()
                .map(|t| {
                    let session_name = session_name_for_task(&t.name);
                    let status = state.watcher.get_session_status(&session_name);
                    let session_panes = state
                        .terminal
                        .list_panes(&crate::terminal::SessionHandle(session_name.clone()))
                        .unwrap_or_default();

                    let pane_statuses: Vec<_> = session_panes
                        .iter()
                        .map(|p| {
                            let s = state
                                .watcher
                                .get_pane_status(&session_name, &p.0)
                                .cloned()
                                .unwrap_or(PaneStatus::Unknown);
                            (p.1.clone(), s)
                        })
                        .collect();

                    (
                        TaskSummary {
                            name: t.name.clone(),
                            repo_count: t.repos.len(),
                            pane_count: t.panes.len(),
                        },
                        status,
                        pane_statuses,
                    )
                })
                .collect();
            CommandResponse::FullStatus { tasks: list }
        }

        RemoteCommand::SendMessage {
            task_name,
            pane_id,
            message,
        } => {
            let session_name = session_name_for_task(task_name);
            match resolve_pane(state, &session_name, pane_id.as_deref(), None) {
                Some(pane) => {
                    if let Err(e) = state.terminal.send_literal(&pane, message) {
                        return CommandResponse::Error {
                            message: format!("Failed to send: {e}"),
                        };
                    }
                    if let Err(e) = state.terminal.send_key(&pane, "Enter") {
                        return CommandResponse::Error {
                            message: format!("Failed to send Enter: {e}"),
                        };
                    }
                    CommandResponse::Confirmation {
                        message: format!("Sent to {task_name}"),
                    }
                }
                None => CommandResponse::Error {
                    message: format!("No pane found for task '{task_name}'"),
                },
            }
        }

        RemoteCommand::Approve {
            task_name,
            pane_id,
        } => {
            let session_name = session_name_for_task(task_name);
            match resolve_pane(state, &session_name, pane_id.as_deref(), Some(true)) {
                Some(pane) => {
                    if let Err(e) = state.terminal.send_key(&pane, "y") {
                        return CommandResponse::Error {
                            message: format!("Failed to approve: {e}"),
                        };
                    }
                    if let Err(e) = state.terminal.send_key(&pane, "Enter") {
                        return CommandResponse::Error {
                            message: format!("Failed to send Enter: {e}"),
                        };
                    }
                    CommandResponse::Confirmation {
                        message: format!("Approved {task_name}"),
                    }
                }
                None => CommandResponse::Error {
                    message: format!("No waiting pane found for task '{task_name}'"),
                },
            }
        }

        RemoteCommand::Reject {
            task_name,
            pane_id,
        } => {
            let session_name = session_name_for_task(task_name);
            match resolve_pane(state, &session_name, pane_id.as_deref(), Some(true)) {
                Some(pane) => {
                    if let Err(e) = state.terminal.send_key(&pane, "n") {
                        return CommandResponse::Error {
                            message: format!("Failed to reject: {e}"),
                        };
                    }
                    if let Err(e) = state.terminal.send_key(&pane, "Enter") {
                        return CommandResponse::Error {
                            message: format!("Failed to send Enter: {e}"),
                        };
                    }
                    CommandResponse::Confirmation {
                        message: format!("Rejected {task_name}"),
                    }
                }
                None => CommandResponse::Error {
                    message: format!("No waiting pane found for task '{task_name}'"),
                },
            }
        }

        RemoteCommand::CaptureOutput {
            task_name,
            pane_id,
            lines,
        } => {
            let session_name = session_name_for_task(task_name);
            let capture_lines = lines.unwrap_or(
                state
                    .config
                    .daemon
                    .telegram
                    .as_ref()
                    .map(|t| t.default_output_lines)
                    .unwrap_or(30),
            );
            match resolve_pane(state, &session_name, pane_id.as_deref(), None) {
                Some(pane) => {
                    let content = capture_tail(&state.terminal, &pane, capture_lines);
                    CommandResponse::Output {
                        task_name: task_name.clone(),
                        pane_id: pane.0.clone(),
                        content,
                    }
                }
                None => CommandResponse::Error {
                    message: format!("No pane found for task '{task_name}'"),
                },
            }
        }

        RemoteCommand::ReplyInput {
            reply_to_message_id,
            text,
        } => match state.message_to_pane.get(reply_to_message_id) {
            Some((task_name, pane_id)) => {
                let pane = PaneHandle(pane_id.clone(), String::new());
                if let Err(e) = state.terminal.send_literal(&pane, text) {
                    return CommandResponse::Error {
                        message: format!("Failed to send: {e}"),
                    };
                }
                if let Err(e) = state.terminal.send_key(&pane, "Enter") {
                    return CommandResponse::Error {
                        message: format!("Failed to send Enter: {e}"),
                    };
                }
                CommandResponse::Confirmation {
                    message: format!("Sent to {task_name}"),
                }
            }
            None => CommandResponse::Error {
                message: "Cannot route reply — message not found. Use /send <task> <message> instead.".into(),
            },
        },

        RemoteCommand::Help => CommandResponse::HelpText,
    }
}

fn resolve_pane(
    state: &DaemonState,
    session_name: &str,
    pane_id: Option<&str>,
    want_waiting: Option<bool>,
) -> Option<PaneHandle> {
    if let Some(id) = pane_id {
        let panes = state
            .terminal
            .list_panes(&crate::terminal::SessionHandle(session_name.to_string()))
            .unwrap_or_default();
        return panes.into_iter().find(|p| p.0 == id);
    }

    let panes = state
        .terminal
        .list_panes(&crate::terminal::SessionHandle(session_name.to_string()))
        .unwrap_or_default();

    if want_waiting == Some(true) {
        // Find first waiting pane
        for pane in &panes {
            if let Some(status) = state.watcher.get_pane_status(session_name, &pane.0) {
                if status.is_waiting() {
                    return Some(pane.clone());
                }
            }
        }
    }

    panes.into_iter().next()
}
