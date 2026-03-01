use crate::config::Config;
use crate::model::Task;
use crate::monitor::status::PaneStatus;
use crate::monitor::strip_ansi;
use crate::plugins::PluginProvider;
use crate::store::Store;
use crate::terminal::{PaneHandle, SessionHandle, Terminal, session_name_for_task};
use crate::transport::{CoreCommand, CoreResponse, TaskSummary};

use super::status_engine::StatusEngine;

pub fn execute(
    terminal: &dyn Terminal,
    _store: &Store,
    engine: &StatusEngine,
    config: &Config,
    plugins: &[Box<dyn PluginProvider>],
    cmd: &CoreCommand,
    tasks: &[Task],
) -> CoreResponse {
    match cmd {
        CoreCommand::ListTasks => {
            let list: Vec<_> = tasks
                .iter()
                .map(|t| {
                    let session_name = session_name_for_task(&t.name);
                    let status = engine.get_session_status(&session_name);
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
            CoreResponse::TaskList { tasks: list }
        }

        CoreCommand::TaskStatus { task_name } => {
            let session_name = session_name_for_task(task_name);
            let session_panes = terminal
                .list_panes(&SessionHandle(session_name.clone()))
                .unwrap_or_default();

            let panes: Vec<_> = session_panes
                .iter()
                .map(|p| {
                    let status = engine
                        .get_pane_status(&session_name, &p.0)
                        .cloned()
                        .unwrap_or(PaneStatus::Unknown);
                    (p.1.clone(), status)
                })
                .collect();

            CoreResponse::Status {
                task_name: task_name.clone(),
                panes,
            }
        }

        CoreCommand::FullStatus => {
            let list: Vec<_> = tasks
                .iter()
                .map(|t| {
                    let session_name = session_name_for_task(&t.name);
                    let status = engine.get_session_status(&session_name);
                    let session_panes = terminal
                        .list_panes(&SessionHandle(session_name.clone()))
                        .unwrap_or_default();

                    let pane_statuses: Vec<_> = session_panes
                        .iter()
                        .map(|p| {
                            let s = engine
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

            CoreResponse::FullStatus { tasks: list }
        }

        CoreCommand::SendMessage {
            task_name,
            pane_id,
            message,
        } => {
            let session_name = session_name_for_task(task_name);
            match resolve_pane(terminal, engine, &session_name, pane_id.as_deref(), None) {
                Some(pane) => {
                    if let Err(e) = terminal.send_literal(&pane, message) {
                        return CoreResponse::Error {
                            message: format!("Failed to send: {e}"),
                        };
                    }
                    if let Err(e) = terminal.send_key(&pane, "Enter") {
                        return CoreResponse::Error {
                            message: format!("Failed to send Enter: {e}"),
                        };
                    }
                    CoreResponse::Confirmation {
                        message: format!("Sent to {task_name}"),
                    }
                }
                None => CoreResponse::Error {
                    message: format!("No pane found for task '{task_name}'"),
                },
            }
        }

        CoreCommand::Approve {
            task_name,
            pane_id,
        } => {
            if task_name.is_empty() {
                return smart_approve(terminal, engine, tasks);
            }

            let session_name = session_name_for_task(task_name);
            match resolve_pane(terminal, engine, &session_name, pane_id.as_deref(), Some(true)) {
                Some(pane) => {
                    if let Err(e) = terminal.send_key(&pane, "y") {
                        return CoreResponse::Error {
                            message: format!("Failed to approve: {e}"),
                        };
                    }
                    if let Err(e) = terminal.send_key(&pane, "Enter") {
                        return CoreResponse::Error {
                            message: format!("Failed to send Enter: {e}"),
                        };
                    }
                    CoreResponse::Confirmation {
                        message: format!("Approved {task_name}"),
                    }
                }
                None => CoreResponse::Error {
                    message: format!("No waiting pane found for task '{task_name}'"),
                },
            }
        }

        CoreCommand::Reject {
            task_name,
            pane_id,
        } => {
            let session_name = session_name_for_task(task_name);
            match resolve_pane(terminal, engine, &session_name, pane_id.as_deref(), Some(true)) {
                Some(pane) => {
                    if let Err(e) = terminal.send_key(&pane, "n") {
                        return CoreResponse::Error {
                            message: format!("Failed to reject: {e}"),
                        };
                    }
                    if let Err(e) = terminal.send_key(&pane, "Enter") {
                        return CoreResponse::Error {
                            message: format!("Failed to send Enter: {e}"),
                        };
                    }
                    CoreResponse::Confirmation {
                        message: format!("Rejected {task_name}"),
                    }
                }
                None => CoreResponse::Error {
                    message: format!("No waiting pane found for task '{task_name}'"),
                },
            }
        }

        CoreCommand::Resume {
            task_name,
            pane_id,
        } => {
            let task = match tasks.iter().find(|t| t.name == *task_name) {
                Some(t) => t,
                None => {
                    return CoreResponse::Error {
                        message: format!("Task '{task_name}' not found"),
                    }
                }
            };

            let session_name = session_name_for_task(task_name);
            let target_pane = match resolve_pane(terminal, engine, &session_name, pane_id.as_deref(), None) {
                Some(p) => p,
                None => {
                    return CoreResponse::Error {
                        message: format!("No pane found for task '{task_name}'"),
                    }
                }
            };

            let tracked = match task.panes.iter().find(|tp| tp.pane_id == target_pane.0) {
                Some(tp) => tp,
                None => {
                    return CoreResponse::Error {
                        message: format!("No session data for pane in '{task_name}'"),
                    }
                }
            };

            let pane_cmd = terminal
                .get_pane_command(&target_pane)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if pane_cmd.contains(tracked.engine.process_name()) {
                return CoreResponse::Error {
                    message: format!("Agent already running in {task_name}"),
                };
            }

            let resume_cmd = tracked.engine.resume_command(&tracked.session_id);
            if let Err(e) = terminal.send_literal(&target_pane, &resume_cmd) {
                return CoreResponse::Error {
                    message: format!("Failed to send resume command: {e}"),
                };
            }
            if let Err(e) = terminal.send_key(&target_pane, "Enter") {
                return CoreResponse::Error {
                    message: format!("Failed to execute resume: {e}"),
                };
            }
            CoreResponse::Confirmation {
                message: format!("Resuming {task_name}"),
            }
        }

        CoreCommand::CaptureOutput {
            task_name,
            pane_id,
            lines,
        } => {
            let session_name = session_name_for_task(task_name);
            let capture_lines = lines.unwrap_or(config.daemon.default_output_lines);
            match resolve_pane(terminal, engine, &session_name, pane_id.as_deref(), None) {
                Some(pane) => {
                    let content = capture_tail(terminal, &pane, capture_lines);
                    CoreResponse::Output {
                        task_name: task_name.clone(),
                        pane_id: pane.0.clone(),
                        content,
                    }
                }
                None => CoreResponse::Error {
                    message: format!("No pane found for task '{task_name}'"),
                },
            }
        }

        CoreCommand::PluginList {
            plugin_id,
            task_name,
        } => {
            let provider = plugins.iter().find(|p| p.id() == plugin_id);
            match provider {
                Some(p) => match p.list_items(&config.tasks_root, task_name.as_deref()) {
                    Ok(items) => CoreResponse::PluginItems {
                        plugin_id: plugin_id.clone(),
                        items,
                    },
                    Err(e) => CoreResponse::Error {
                        message: format!("Plugin error: {e}"),
                    },
                },
                None => CoreResponse::Error {
                    message: format!("Plugin '{plugin_id}' not found"),
                },
            }
        }

        CoreCommand::PluginGet {
            plugin_id,
            item_id,
        } => {
            let provider = plugins.iter().find(|p| p.id() == plugin_id);
            match provider {
                Some(p) => match p.get_item(&config.tasks_root, None, item_id) {
                    Ok(Some(detail)) => CoreResponse::PluginDetail {
                        plugin_id: plugin_id.clone(),
                        detail,
                    },
                    Ok(None) => CoreResponse::Error {
                        message: format!("Item '{item_id}' not found in plugin '{plugin_id}'"),
                    },
                    Err(e) => CoreResponse::Error {
                        message: format!("Plugin error: {e}"),
                    },
                },
                None => CoreResponse::Error {
                    message: format!("Plugin '{plugin_id}' not found"),
                },
            }
        }

        CoreCommand::Help => CoreResponse::HelpText,
    }
}

fn smart_approve(
    terminal: &dyn Terminal,
    engine: &StatusEngine,
    tasks: &[Task],
) -> CoreResponse {
    let mut waiting_panes: Vec<(String, String, String)> = vec![];

    for task in tasks {
        let session_name = session_name_for_task(&task.name);
        let panes = terminal
            .list_panes(&SessionHandle(session_name.clone()))
            .unwrap_or_default();
        for pane in &panes {
            let status = engine
                .get_pane_status(&session_name, &pane.0)
                .cloned()
                .unwrap_or(PaneStatus::Unknown);
            if status.is_waiting() {
                waiting_panes.push((task.name.clone(), pane.0.clone(), pane.1.clone()));
            }
        }
    }

    match waiting_panes.len() {
        0 => CoreResponse::Error {
            message: "No panes are waiting for approval.".into(),
        },
        1 => {
            let (task_name, pane_id, _) = &waiting_panes[0];
            let handle = PaneHandle(pane_id.clone(), String::new());
            if let Err(e) = terminal.send_key(&handle, "y") {
                return CoreResponse::Error {
                    message: format!("Failed to approve: {e}"),
                };
            }
            let _ = terminal.send_key(&handle, "Enter");
            CoreResponse::Confirmation {
                message: format!("Approved {task_name}"),
            }
        }
        _ => CoreResponse::Confirmation {
            message: format!("{} panes waiting for approval.", waiting_panes.len()),
        },
    }
}

fn capture_tail(terminal: &dyn Terminal, pane: &PaneHandle, lines: usize) -> String {
    terminal
        .capture(pane, lines)
        .map(|s| strip_ansi(&s))
        .unwrap_or_default()
}

fn resolve_pane(
    terminal: &dyn Terminal,
    engine: &StatusEngine,
    session_name: &str,
    pane_id: Option<&str>,
    want_waiting: Option<bool>,
) -> Option<PaneHandle> {
    if let Some(id) = pane_id {
        let panes = terminal
            .list_panes(&SessionHandle(session_name.to_string()))
            .unwrap_or_default();
        return panes.into_iter().find(|p| p.0 == id);
    }

    let panes = terminal
        .list_panes(&SessionHandle(session_name.to_string()))
        .unwrap_or_default();

    if want_waiting == Some(true) {
        for pane in &panes {
            if let Some(status) = engine.get_pane_status(session_name, &pane.0) {
                if status.is_waiting() {
                    return Some(pane.clone());
                }
            }
        }
    }

    panes.into_iter().next()
}
