use crate::config::Config;
use crate::model::{Engine, Task};
use crate::monitor::status::PaneStatus;
use crate::monitor::strip_ansi;
use crate::plugins::PluginProvider;
use crate::store::Store;
use crate::terminal::{PaneHandle, SessionHandle, Terminal, session_name_for_task};
use crate::transport::{CoreCommand, CoreResponse, TaskSummary};

use super::status_engine::StatusEngine;

pub fn execute(
    terminal: &dyn Terminal,
    store: &Store,
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
                            pane_count: live_pane_count(terminal, &t.name),
                        },
                        status,
                    )
                })
                .collect();
            CoreResponse::TaskList { tasks: list }
        }

        CoreCommand::TaskStatus { task_name } => {
            let task = tasks.iter().find(|t| t.name == *task_name);
            let session_name = session_name_for_task(task_name);
            let session_status = engine.get_session_status(&session_name);
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
                    let name = task
                        .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == p.0))
                        .map(|tp| tp.name.clone())
                        .unwrap_or_else(|| p.1.clone());
                    (name, status)
                })
                .collect();

            let summary = TaskSummary {
                name: task_name.clone(),
                repo_count: task.map(|t| t.repos.len()).unwrap_or(0),
                pane_count: session_panes.len(),
            };

            CoreResponse::Status {
                task_name: task_name.clone(),
                summary,
                status: session_status,
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
                            let name = t
                                .panes
                                .iter()
                                .find(|tp| tp.pane_id == p.0)
                                .map(|tp| tp.name.clone())
                                .unwrap_or_else(|| p.1.clone());
                            (name, s)
                        })
                        .collect();

                    (
                        TaskSummary {
                            name: t.name.clone(),
                            repo_count: t.repos.len(),
                            pane_count: pane_statuses.len(),
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
            pane_name,
            message,
        } => {
            let session_name = session_name_for_task(task_name);
            match resolve_pane(
                terminal,
                engine,
                &session_name,
                tasks,
                task_name,
                pane_name.as_deref(),
                None,
            ) {
                Some(pane) => {
                    let delay = resolve_pane_engine(tasks, task_name, &pane.0)
                        .map(|e| e.enter_delay_ms())
                        .unwrap_or(100);
                    if let Err(e) = terminal.send_text_enter(&pane, message, delay) {
                        return CoreResponse::Error {
                            message: format!("Failed to send: {e}"),
                        };
                    }
                    let display_pane = resolve_pane_display_name(tasks, task_name, &pane.0);
                    CoreResponse::Confirmation {
                        message: format!("Sent to {task_name} | {display_pane}"),
                    }
                }
                None => CoreResponse::Error {
                    message: format!("No pane found for task '{task_name}'"),
                },
            }
        }

        CoreCommand::Approve {
            task_name,
            pane_name,
        } => {
            if task_name.is_empty() {
                return smart_approve(terminal, engine, tasks);
            }

            let session_name = session_name_for_task(task_name);
            match resolve_pane(
                terminal,
                engine,
                &session_name,
                tasks,
                task_name,
                pane_name.as_deref(),
                Some(true),
            ) {
                Some(pane) => {
                    if let Err(e) = send_approve_to_pane(terminal, &pane) {
                        return CoreResponse::Error { message: e };
                    }
                    let display_pane = resolve_pane_display_name(tasks, task_name, &pane.0);
                    CoreResponse::Confirmation {
                        message: format!("Approved {task_name} | {display_pane}"),
                    }
                }
                None => CoreResponse::Error {
                    message: format!("No waiting pane found for task '{task_name}'"),
                },
            }
        }

        CoreCommand::Reject {
            task_name,
            pane_name,
        } => {
            let session_name = session_name_for_task(task_name);
            match resolve_pane(
                terminal,
                engine,
                &session_name,
                tasks,
                task_name,
                pane_name.as_deref(),
                Some(true),
            ) {
                Some(pane) => {
                    if let Err(e) = send_reject_to_pane(terminal, &pane) {
                        return CoreResponse::Error { message: e };
                    }
                    let display_pane = resolve_pane_display_name(tasks, task_name, &pane.0);
                    CoreResponse::Confirmation {
                        message: format!("Rejected {task_name} | {display_pane}"),
                    }
                }
                None => CoreResponse::Error {
                    message: format!("No waiting pane found for task '{task_name}'"),
                },
            }
        }

        CoreCommand::Resume {
            task_name,
            pane_name,
        } => {
            let task = match tasks.iter().find(|t| t.name == *task_name) {
                Some(t) => t,
                None => {
                    return CoreResponse::Error {
                        message: format!("Task '{task_name}' not found"),
                    };
                }
            };

            let session_name = session_name_for_task(task_name);
            let target_pane = match resolve_pane(
                terminal,
                engine,
                &session_name,
                tasks,
                task_name,
                pane_name.as_deref(),
                None,
            ) {
                Some(p) => p,
                None => {
                    return CoreResponse::Error {
                        message: format!("No pane found for task '{task_name}'"),
                    };
                }
            };

            let tracked = match task.panes.iter().find(|tp| tp.pane_id == target_pane.0) {
                Some(tp) => tp,
                None => {
                    return CoreResponse::Error {
                        message: format!("No session data for pane in '{task_name}'"),
                    };
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

            // Clear any existing typed input before sending the resume command.
            let _ = terminal.send_key(&target_pane, "C-c");
            let _ = terminal.send_key(&target_pane, "C-u");

            let resume_cmd = tracked.engine.resume_command(&tracked.session_id);
            if let Err(e) =
                terminal.send_text_enter(&target_pane, &resume_cmd, tracked.engine.enter_delay_ms())
            {
                return CoreResponse::Error {
                    message: format!("Failed to send resume command: {e}"),
                };
            }
            let display_pane = resolve_pane_display_name(tasks, task_name, &target_pane.0);
            CoreResponse::Confirmation {
                message: format!("Resuming {task_name} | {display_pane}"),
            }
        }

        CoreCommand::CaptureOutput {
            task_name,
            pane_name,
            lines,
        } => {
            let session_name = session_name_for_task(task_name);
            let capture_lines = lines.unwrap_or(config.daemon.default_output_lines);
            match resolve_pane(
                terminal,
                engine,
                &session_name,
                tasks,
                task_name,
                pane_name.as_deref(),
                None,
            ) {
                Some(pane) => {
                    let content = capture_tail(terminal, &pane, capture_lines);
                    let resolved_name = pane_name.clone().unwrap_or_else(|| {
                        tasks
                            .iter()
                            .find(|t| t.name == *task_name)
                            .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane.0))
                            .map(|tp| tp.name.clone())
                            .unwrap_or_else(|| pane.1.clone())
                    });
                    CoreResponse::Output {
                        task_name: task_name.clone(),
                        pane_name: resolved_name,
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

        CoreCommand::PluginGet { plugin_id, item_id } => {
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

        CoreCommand::AddPane {
            task_name,
            pane_name,
            agent,
            repo_name,
        } => {
            let engine_type = match agent.as_deref() {
                Some("codex") => Engine::Codex,
                Some("terminal") => Engine::Terminal,
                Some("droid") => Engine::Droid,
                Some("claude") | None => Engine::ClaudeCode,
                Some(other) => {
                    return CoreResponse::Error {
                        message: format!(
                            "Unknown agent type '{other}'. Use claude, codex, droid, or terminal."
                        ),
                    };
                }
            };

            let mut task = match tasks.iter().find(|t| t.name == *task_name) {
                Some(t) => t.clone(),
                None => {
                    return CoreResponse::Error {
                        message: format!("Task '{task_name}' not found"),
                    };
                }
            };

            let repo = match crate::wagner::resolve_repo(&task, repo_name.as_deref()) {
                Ok(r) => r,
                Err(_) => {
                    let name = repo_name.as_deref().unwrap_or("(default)");
                    return CoreResponse::Error {
                        message: format!(
                            "Repo '{}' not found in task '{}'",
                            name, task_name
                        ),
                    };
                }
            };

            match crate::wagner::add_pane_shared(
                terminal,
                store,
                &mut task,
                &repo,
                engine_type,
                pane_name.as_deref(),
            ) {
                Ok(tracked) => {
                    let label = match engine_type {
                        Engine::ClaudeCode => "Claude",
                        Engine::Codex => "Codex",
                        Engine::Droid => "Droid",
                        Engine::Terminal => "terminal",
                    };
                    CoreResponse::Confirmation {
                        message: format!(
                            "Added {label} pane '{}' to {task_name}",
                            tracked.name
                        ),
                    }
                }
                Err(e) => CoreResponse::Error {
                    message: format!("Failed to add pane: {e}"),
                },
            }
        }

        CoreCommand::RenamePane {
            task_name,
            pane_name,
            new_name,
        } => {
            let mut task = match tasks.iter().find(|t| t.name == *task_name) {
                Some(t) => t.clone(),
                None => {
                    return CoreResponse::Error {
                        message: format!("Task '{task_name}' not found"),
                    };
                }
            };

            if !task.rename_pane(pane_name, new_name) {
                return CoreResponse::Error {
                    message: format!(
                        "Cannot rename '{pane_name}' to '{new_name}' — source not found or target exists"
                    ),
                };
            }

            if let Err(e) = store.save_task(&task) {
                return CoreResponse::Error {
                    message: format!("Renamed but failed to save: {e}"),
                };
            }

            CoreResponse::Confirmation {
                message: format!("Renamed '{pane_name}' to '{new_name}' in {task_name}"),
            }
        }

        CoreCommand::KillPane {
            task_name,
            pane_name,
        } => {
            let mut task = match tasks.iter().find(|t| t.name == *task_name) {
                Some(t) => t.clone(),
                None => {
                    return CoreResponse::Error {
                        message: format!("Task '{task_name}' not found"),
                    };
                }
            };

            let pane_id = match task.find_pane_by_name(pane_name) {
                Some(tp) => tp.pane_id.clone(),
                None => {
                    return CoreResponse::Error {
                        message: format!("Pane '{pane_name}' not found in task '{task_name}'"),
                    };
                }
            };

            let handle = PaneHandle(pane_id.clone(), String::new());
            if let Err(e) = terminal.kill_pane(&handle) {
                return CoreResponse::Error {
                    message: format!("Failed to kill pane: {e}"),
                };
            }

            task.panes.retain(|p| p.pane_id != pane_id);

            if let Err(e) = store.save_task(&task) {
                return CoreResponse::Error {
                    message: format!("Pane killed but failed to save: {e}"),
                };
            }

            CoreResponse::Confirmation {
                message: format!("Killed pane '{pane_name}' in {task_name}"),
            }
        }

        CoreCommand::SetPaneMode {
            task_name,
            pane_name,
            mode,
        } => {
            if !tasks.iter().any(|t| t.name == *task_name) {
                return CoreResponse::Error {
                    message: format!("Task '{}' not found", task_name),
                };
            }
            let resolved_name = pane_name.clone().unwrap_or_else(|| "all".to_string());
            CoreResponse::ModeChanged {
                task_name: task_name.clone(),
                pane_name: resolved_name,
                mode: *mode,
            }
        }

        CoreCommand::Help => CoreResponse::HelpText,
    }
}

fn send_approve_to_pane(terminal: &dyn Terminal, pane: &PaneHandle) -> Result<(), String> {
    terminal
        .send_approve(pane)
        .map_err(|e| format!("Failed to approve: {e}"))
}

fn send_reject_to_pane(terminal: &dyn Terminal, pane: &PaneHandle) -> Result<(), String> {
    terminal
        .send_reject(pane)
        .map_err(|e| format!("Failed to reject: {e}"))
}

fn smart_approve(terminal: &dyn Terminal, engine: &StatusEngine, tasks: &[Task]) -> CoreResponse {
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

    if waiting_panes.is_empty() {
        return CoreResponse::Error {
            message: "No panes are waiting for approval.".into(),
        };
    }

    let mut approved: Vec<String> = vec![];
    let mut errors: Vec<String> = vec![];

    for (task_name, pane_id, _) in &waiting_panes {
        let handle = PaneHandle(pane_id.clone(), String::new());
        match send_approve_to_pane(terminal, &handle) {
            Ok(()) => {
                let display_pane = resolve_pane_display_name(tasks, task_name, pane_id);
                approved.push(format!("{task_name} | {display_pane}"));
            }
            Err(e) => {
                errors.push(e);
            }
        }
    }

    if approved.is_empty() {
        return CoreResponse::Error {
            message: format!("Failed to approve any panes: {}", errors.join("; ")),
        };
    }

    CoreResponse::Confirmation {
        message: format!("Approved {} pane(s): {}", approved.len(), approved.join(", ")),
    }
}

fn capture_tail(terminal: &dyn Terminal, pane: &PaneHandle, lines: usize) -> String {
    terminal
        .capture(pane, lines)
        .map(|s| strip_ansi(&s))
        .unwrap_or_default()
}

fn live_pane_count(terminal: &dyn Terminal, task_name: &str) -> usize {
    let session_name = session_name_for_task(task_name);
    terminal
        .list_panes(&SessionHandle(session_name))
        .map(|p| p.len())
        .unwrap_or(0)
}

fn resolve_pane_engine(tasks: &[Task], task_name: &str, pane_id: &str) -> Option<Engine> {
    tasks
        .iter()
        .find(|t| t.name == task_name)
        .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
        .map(|tp| tp.engine)
}

fn resolve_pane_display_name(tasks: &[Task], task_name: &str, pane_id: &str) -> String {
    tasks
        .iter()
        .find(|t| t.name == task_name)
        .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
        .map(|tp| tp.name.clone())
        .unwrap_or_else(|| pane_id.to_string())
}

fn resolve_pane(
    terminal: &dyn Terminal,
    engine: &StatusEngine,
    session_name: &str,
    tasks: &[Task],
    task_name: &str,
    pane_name: Option<&str>,
    want_waiting: Option<bool>,
) -> Option<PaneHandle> {
    let task = tasks.iter().find(|t| t.name == task_name);

    if let Some(name) = pane_name {
        if let Some(task) = task
            && let Some(tracked) = task.find_pane_by_name(name)
        {
            let panes = terminal
                .list_panes(&SessionHandle(session_name.to_string()))
                .unwrap_or_default();
            return panes.into_iter().find(|p| p.0 == tracked.pane_id);
        }
        return None;
    }

    let panes = terminal
        .list_panes(&SessionHandle(session_name.to_string()))
        .unwrap_or_default();

    if want_waiting == Some(true) {
        for pane in &panes {
            if let Some(status) = engine.get_pane_status(session_name, &pane.0)
                && status.is_waiting()
            {
                return Some(pane.clone());
            }
        }
        return None;
    }

    panes.into_iter().next()
}
