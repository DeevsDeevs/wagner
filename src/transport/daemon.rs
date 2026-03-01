use std::collections::HashMap;
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

use crate::config::Config;
use crate::error::WagnerError;
use crate::monitor::strip_ansi;
use crate::monitor::status::{PaneStatus, SessionAggregateStatus};
use crate::monitor::watcher::SessionWatcher;
use crate::monitor::StatusMonitor;
use crate::store::Store;
use crate::terminal::{PaneHandle, Terminal, Tmux, session_name_for_task};

use super::{
    ActionButton, CommandResponse, MessageRef, RemoteCommand, TaskSummary, Transport,
    TransportEvent,
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
    // ID registries for callback data (64-byte limit)
    entity_registry: HashMap<u16, (String, String)>,
    entity_reverse: HashMap<(String, String), u16>,
    task_registry: HashMap<u16, String>,
    task_reverse: HashMap<String, u16>,
    next_entity_id: u16,
    next_task_id: u16,
    // Focus mode
    focus: Option<FocusTarget>,
    suppressed_count: u32,
}

struct FocusTarget {
    task_name: String,
    pane_id: Option<String>,
    sticky: bool,
}

impl DaemonState {
    fn register_entity(&mut self, task: &str, pane: &str) -> u16 {
        let key = (task.to_string(), pane.to_string());
        if let Some(&id) = self.entity_reverse.get(&key) {
            return id;
        }
        let id = self.next_entity_id;
        self.next_entity_id = self.next_entity_id.wrapping_add(1);
        self.entity_registry.insert(id, key.clone());
        self.entity_reverse.insert(key, id);
        id
    }

    fn register_task(&mut self, task: &str) -> u16 {
        if let Some(&id) = self.task_reverse.get(task) {
            return id;
        }
        let id = self.next_task_id;
        self.next_task_id = self.next_task_id.wrapping_add(1);
        self.task_registry.insert(id, task.to_string());
        self.task_reverse.insert(task.to_string(), id);
        id
    }

    fn resolve_entity(&self, id: u16) -> Option<(&str, &str)> {
        self.entity_registry
            .get(&id)
            .map(|(t, p)| (t.as_str(), p.as_str()))
    }

    fn resolve_task(&self, id: u16) -> Option<&str> {
        self.task_registry.get(&id).map(|s| s.as_str())
    }

    fn matches_focus(&self, task_name: &str, pane_id: &str) -> bool {
        match &self.focus {
            None => true,
            Some(f) => {
                if f.task_name != task_name {
                    return false;
                }
                match &f.pane_id {
                    Some(pid) => pid == pane_id,
                    None => true,
                }
            }
        }
    }
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

    let fallback = StatusMonitor::with_detectors(vec![]);
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
        entity_registry: HashMap::new(),
        entity_reverse: HashMap::new(),
        task_registry: HashMap::new(),
        task_reverse: HashMap::new(),
        next_entity_id: 1,
        next_task_id: 1,
        focus: None,
        suppressed_count: 0,
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
                let tid = state.register_task(&task.name);
                let actions = vec![vec![ActionButton {
                    label: "Details".into(),
                    callback_data: format!("td:{tid}"),
                }]];
                transport
                    .send_event(&TransportEvent::SessionStatusChanged {
                        task_name: task.name.clone(),
                        status: session_status,
                        actions,
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
                // Focus filtering: suppress if not matching focus target
                if !state.matches_focus(&task.name, pane_id) {
                    state.suppressed_count += 1;
                    continue;
                }

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

                let eid = state.register_entity(&task.name, pane_id);
                let actions = build_attention_actions(eid, &reason, state.suppressed_count, state.focus.is_some());

                let msg_ref = transport
                    .send_event(&TransportEvent::NeedsAttention {
                        task_name: task.name.clone(),
                        pane_id: pane_id.clone(),
                        pane_title: pane_title.clone(),
                        reason,
                        output_tail,
                        actions,
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
                // Focus filtering for idle notifications
                if !state.matches_focus(&task.name, pane_id) {
                    state.suppressed_count += 1;
                    continue;
                }

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

fn build_attention_actions(
    entity_id: u16,
    reason: &crate::monitor::status::WaitReason,
    suppressed_count: u32,
    focused: bool,
) -> Vec<Vec<ActionButton>> {
    use crate::monitor::status::WaitReason;

    let mut row1 = vec![];

    match reason {
        WaitReason::Approval | WaitReason::Permission => {
            row1.push(ActionButton {
                label: "Approve".into(),
                callback_data: format!("a:{entity_id}"),
            });
            row1.push(ActionButton {
                label: "Reject".into(),
                callback_data: format!("r:{entity_id}"),
            });
        }
        WaitReason::Question | WaitReason::Input => {}
    }

    row1.push(ActionButton {
        label: "Output".into(),
        callback_data: format!("o:{entity_id}"),
    });

    let mut row2 = vec![];
    if focused {
        let label = if suppressed_count > 0 {
            format!("Unfocus ({suppressed_count} suppressed)")
        } else {
            "Unfocus".into()
        };
        row2.push(ActionButton {
            label,
            callback_data: "uf".into(),
        });
    } else {
        row2.push(ActionButton {
            label: "Focus".into(),
            callback_data: format!("fp:{entity_id}"),
        });
    }

    vec![row1, row2]
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

            // Build per-pane action buttons
            let mut actions = vec![];
            for p in &session_panes {
                let eid = state.register_entity(task_name, &p.0);
                let status = state
                    .watcher
                    .get_pane_status(&session_name, &p.0)
                    .cloned()
                    .unwrap_or(PaneStatus::Unknown);
                let mut row = vec![];
                if status.is_waiting() {
                    row.push(ActionButton {
                        label: format!("Approve {}", p.1),
                        callback_data: format!("a:{eid}"),
                    });
                }
                row.push(ActionButton {
                    label: format!("Output {}", p.1),
                    callback_data: format!("o:{eid}"),
                });
                if !row.is_empty() {
                    actions.push(row);
                }
            }

            let tid = state.register_task(task_name);
            // Approve All button if any waiting
            if panes.iter().any(|(_, s)| s.is_waiting()) {
                actions.push(vec![ActionButton {
                    label: "Approve All".into(),
                    callback_data: format!("aa:{tid}"),
                }]);
            }
            actions.push(vec![ActionButton {
                label: "Back".into(),
                callback_data: "bk".into(),
            }]);

            CommandResponse::Status {
                task_name: task_name.clone(),
                panes,
                actions,
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

            // Build per-task [Details] buttons + [Refresh]
            let mut actions: Vec<Vec<ActionButton>> = vec![];
            let mut detail_row = vec![];
            for t in tasks {
                let tid = state.register_task(&t.name);
                detail_row.push(ActionButton {
                    label: format!("{} Details", t.name),
                    callback_data: format!("td:{tid}"),
                });
                // Keep rows at max 2 buttons for mobile
                if detail_row.len() >= 2 {
                    actions.push(std::mem::take(&mut detail_row));
                }
            }
            if !detail_row.is_empty() {
                actions.push(detail_row);
            }
            actions.push(vec![ActionButton {
                label: "Refresh".into(),
                callback_data: "sr".into(),
            }]);

            CommandResponse::FullStatus {
                tasks: list,
                actions,
            }
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
                        actions: vec![],
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
            // Smart argless approve: if task_name is empty, find the single waiting pane
            if task_name.is_empty() {
                return smart_approve(state, tasks);
            }

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
                        actions: vec![],
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
                        actions: vec![],
                    }
                }
                None => CommandResponse::Error {
                    message: format!("No waiting pane found for task '{task_name}'"),
                },
            }
        }

        RemoteCommand::Resume {
            task_name,
            pane_id,
        } => {
            let task = tasks.iter().find(|t| t.name == *task_name);
            let task = match task {
                Some(t) => t,
                None => {
                    return CommandResponse::Error {
                        message: format!("Task '{task_name}' not found"),
                    }
                }
            };

            let session_name = session_name_for_task(task_name);
            let target_pane = match resolve_pane(state, &session_name, pane_id.as_deref(), None) {
                Some(p) => p,
                None => {
                    return CommandResponse::Error {
                        message: format!("No pane found for task '{task_name}'"),
                    }
                }
            };

            // Find the TrackedPane to get engine + session_id
            let tracked = task
                .panes
                .iter()
                .find(|tp| tp.pane_id == target_pane.0);
            let tracked = match tracked {
                Some(tp) => tp,
                None => {
                    return CommandResponse::Error {
                        message: format!("No session data for pane in '{task_name}'"),
                    }
                }
            };

            // Check if agent is already running
            let pane_cmd = state
                .terminal
                .get_pane_command(&target_pane)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if pane_cmd.contains(tracked.engine.process_name()) {
                return CommandResponse::Error {
                    message: format!("Agent already running in {task_name}"),
                };
            }

            let resume_cmd = tracked.engine.resume_command(&tracked.session_id);
            if let Err(e) = state.terminal.send_literal(&target_pane, &resume_cmd) {
                return CommandResponse::Error {
                    message: format!("Failed to send resume command: {e}"),
                };
            }
            if let Err(e) = state.terminal.send_key(&target_pane, "Enter") {
                return CommandResponse::Error {
                    message: format!("Failed to execute resume: {e}"),
                };
            }
            CommandResponse::Confirmation {
                message: format!("Resuming {task_name}"),
                actions: vec![],
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
                    actions: vec![],
                }
            }
            None => CommandResponse::Error {
                message: "Cannot route reply — message not found. Use /send <task> <message> instead.".into(),
            },
        },

        RemoteCommand::Callback {
            data,
            source_message_id: _,
        } => handle_callback(state, data, tasks),

        RemoteCommand::Focus {
            task_name,
            pane_id,
            sticky,
        } => {
            state.focus = Some(FocusTarget {
                task_name: task_name.clone(),
                pane_id: pane_id.clone(),
                sticky: *sticky,
            });
            state.suppressed_count = 0;
            let target = match pane_id {
                Some(p) => format!("{task_name}/{p}"),
                None => task_name.clone(),
            };
            let sticky_note = if *sticky { " (sticky)" } else { "" };
            CommandResponse::Confirmation {
                message: format!("Focused on {target}{sticky_note}"),
                actions: vec![vec![ActionButton {
                    label: "Unfocus".into(),
                    callback_data: "uf".into(),
                }]],
            }
        }

        RemoteCommand::Unfocus => {
            let count = state.suppressed_count;
            state.focus = None;
            state.suppressed_count = 0;
            CommandResponse::Confirmation {
                message: format!("Focus cleared. {count} notifications were suppressed."),
                actions: vec![vec![ActionButton {
                    label: "Status".into(),
                    callback_data: "sr".into(),
                }]],
            }
        }

        RemoteCommand::Help => CommandResponse::HelpText,

        RemoteCommand::Unknown { .. } => CommandResponse::Error {
            message: "Unknown command. /help for available commands.".into(),
        },
    }
}

fn handle_callback(
    state: &mut DaemonState,
    data: &str,
    tasks: &[crate::model::Task],
) -> CommandResponse {
    let parts: Vec<&str> = data.splitn(2, ':').collect();
    let action = parts[0];
    let id_str = parts.get(1).unwrap_or(&"");

    match action {
        "a" => {
            // Approve entity
            let id: u16 = match id_str.parse() {
                Ok(v) => v,
                Err(_) => return CommandResponse::Error { message: "Invalid callback data.".into() },
            };
            match state.resolve_entity(id) {
                Some((task, pane)) => {
                    let task = task.to_string();
                    let pane = pane.to_string();
                    let handle = PaneHandle(pane, String::new());
                    if let Err(e) = state.terminal.send_key(&handle, "y") {
                        return CommandResponse::Error { message: format!("Failed to approve: {e}") };
                    }
                    let _ = state.terminal.send_key(&handle, "Enter");
                    CommandResponse::Confirmation {
                        message: format!("Approved {task}"),
                        actions: vec![],
                    }
                }
                None => CommandResponse::Error { message: "Stale button — entity no longer tracked.".into() },
            }
        }

        "r" => {
            let id: u16 = match id_str.parse() {
                Ok(v) => v,
                Err(_) => return CommandResponse::Error { message: "Invalid callback data.".into() },
            };
            match state.resolve_entity(id) {
                Some((task, pane)) => {
                    let task = task.to_string();
                    let pane = pane.to_string();
                    let handle = PaneHandle(pane, String::new());
                    if let Err(e) = state.terminal.send_key(&handle, "n") {
                        return CommandResponse::Error { message: format!("Failed to reject: {e}") };
                    }
                    let _ = state.terminal.send_key(&handle, "Enter");
                    CommandResponse::Confirmation {
                        message: format!("Rejected {task}"),
                        actions: vec![],
                    }
                }
                None => CommandResponse::Error { message: "Stale button — entity no longer tracked.".into() },
            }
        }

        "o" => {
            let id: u16 = match id_str.parse() {
                Ok(v) => v,
                Err(_) => return CommandResponse::Error { message: "Invalid callback data.".into() },
            };
            match state.resolve_entity(id) {
                Some((task, pane)) => {
                    let task = task.to_string();
                    let pane = pane.to_string();
                    let handle = PaneHandle(pane.clone(), String::new());
                    let lines = state.config.daemon.telegram.as_ref()
                        .map(|t| t.default_output_lines)
                        .unwrap_or(30);
                    let content = capture_tail(&state.terminal, &handle, lines);
                    CommandResponse::Output {
                        task_name: task,
                        pane_id: pane,
                        content,
                    }
                }
                None => CommandResponse::Error { message: "Stale button — entity no longer tracked.".into() },
            }
        }

        "fp" => {
            let id: u16 = match id_str.parse() {
                Ok(v) => v,
                Err(_) => return CommandResponse::Error { message: "Invalid callback data.".into() },
            };
            match state.resolve_entity(id) {
                Some((task, pane)) => {
                    let task = task.to_string();
                    let pane = pane.to_string();
                    state.focus = Some(FocusTarget {
                        task_name: task.clone(),
                        pane_id: Some(pane.clone()),
                        sticky: false,
                    });
                    state.suppressed_count = 0;
                    CommandResponse::Confirmation {
                        message: format!("Focused on {task}/{pane}"),
                        actions: vec![vec![ActionButton {
                            label: "Unfocus".into(),
                            callback_data: "uf".into(),
                        }]],
                    }
                }
                None => CommandResponse::Error { message: "Stale button — entity no longer tracked.".into() },
            }
        }

        "ft" => {
            let id: u16 = match id_str.parse() {
                Ok(v) => v,
                Err(_) => return CommandResponse::Error { message: "Invalid callback data.".into() },
            };
            match state.resolve_task(id) {
                Some(task) => {
                    let task = task.to_string();
                    state.focus = Some(FocusTarget {
                        task_name: task.clone(),
                        pane_id: None,
                        sticky: false,
                    });
                    state.suppressed_count = 0;
                    CommandResponse::Confirmation {
                        message: format!("Focused on {task}"),
                        actions: vec![vec![ActionButton {
                            label: "Unfocus".into(),
                            callback_data: "uf".into(),
                        }]],
                    }
                }
                None => CommandResponse::Error { message: "Stale button — task no longer tracked.".into() },
            }
        }

        "td" => {
            // Task drill-down: same as TaskStatus
            let id: u16 = match id_str.parse() {
                Ok(v) => v,
                Err(_) => return CommandResponse::Error { message: "Invalid callback data.".into() },
            };
            match state.resolve_task(id) {
                Some(task_name) => {
                    let task_name = task_name.to_string();
                    handle_command(
                        state,
                        &RemoteCommand::TaskStatus { task_name },
                        tasks,
                    )
                }
                None => CommandResponse::Error { message: "Stale button — task no longer tracked.".into() },
            }
        }

        "aa" => {
            // Approve all waiting panes in task
            let id: u16 = match id_str.parse() {
                Ok(v) => v,
                Err(_) => return CommandResponse::Error { message: "Invalid callback data.".into() },
            };
            match state.resolve_task(id) {
                Some(task_name) => {
                    let task_name = task_name.to_string();
                    let session_name = session_name_for_task(&task_name);
                    let panes = state.terminal
                        .list_panes(&crate::terminal::SessionHandle(session_name.clone()))
                        .unwrap_or_default();

                    let mut approved = 0;
                    for pane in &panes {
                        let status = state.watcher
                            .get_pane_status(&session_name, &pane.0)
                            .cloned()
                            .unwrap_or(PaneStatus::Unknown);
                        if status.is_waiting() {
                            let _ = state.terminal.send_key(pane, "y");
                            let _ = state.terminal.send_key(pane, "Enter");
                            approved += 1;
                        }
                    }
                    CommandResponse::Confirmation {
                        message: format!("Approved {approved} panes in {task_name}"),
                        actions: vec![],
                    }
                }
                None => CommandResponse::Error { message: "Stale button — task no longer tracked.".into() },
            }
        }

        "sr" => {
            // Refresh: return full status
            handle_command(state, &RemoteCommand::FullStatus, tasks)
        }

        "bk" => {
            // Back: return full status
            handle_command(state, &RemoteCommand::FullStatus, tasks)
        }

        "uf" => {
            // Unfocus
            handle_command(state, &RemoteCommand::Unfocus, tasks)
        }

        _ => {
            warn!(%data, "unknown callback action");
            CommandResponse::Error {
                message: "Unknown action.".into(),
            }
        }
    }
}

fn smart_approve(state: &mut DaemonState, tasks: &[crate::model::Task]) -> CommandResponse {
    let mut waiting_panes: Vec<(String, String, String)> = vec![]; // (task, pane_id, pane_title)

    for task in tasks {
        let session_name = session_name_for_task(&task.name);
        let panes = state.terminal
            .list_panes(&crate::terminal::SessionHandle(session_name.clone()))
            .unwrap_or_default();
        for pane in &panes {
            let status = state.watcher
                .get_pane_status(&session_name, &pane.0)
                .cloned()
                .unwrap_or(PaneStatus::Unknown);
            if status.is_waiting() {
                waiting_panes.push((task.name.clone(), pane.0.clone(), pane.1.clone()));
            }
        }
    }

    match waiting_panes.len() {
        0 => CommandResponse::Error {
            message: "No panes are waiting for approval.".into(),
        },
        1 => {
            let (task_name, pane_id, _) = &waiting_panes[0];
            let handle = PaneHandle(pane_id.clone(), String::new());
            if let Err(e) = state.terminal.send_key(&handle, "y") {
                return CommandResponse::Error {
                    message: format!("Failed to approve: {e}"),
                };
            }
            let _ = state.terminal.send_key(&handle, "Enter");
            CommandResponse::Confirmation {
                message: format!("Approved {task_name}"),
                actions: vec![],
            }
        }
        _ => {
            // Multiple waiting: return picker buttons
            let actions: Vec<Vec<ActionButton>> = waiting_panes
                .iter()
                .map(|(task, pane_id, pane_title)| {
                    let eid = state.register_entity(task, pane_id);
                    vec![ActionButton {
                        label: format!("Approve {task}/{pane_title}"),
                        callback_data: format!("a:{eid}"),
                    }]
                })
                .collect();
            CommandResponse::Confirmation {
                message: format!("{} panes waiting. Choose one:", waiting_panes.len()),
                actions,
            }
        }
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
