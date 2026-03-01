use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::signal::unix::{SignalKind, signal};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::core::WagnerCore;
use crate::store::Store;
use crate::terminal::{PaneHandle, Terminal, Tmux, session_name_for_task};

use super::adapter::{Adapter, DaemonAdapter, LogAdapter};
use super::{CoreEvent, TaskSummary};

struct DaemonState {
    core: WagnerCore,
    terminal: Tmux,
    store: Store,
    last_health_check: Instant,
}

pub fn pid_path() -> PathBuf {
    Config::config_dir().join("daemon.pid")
}

fn write_pid_file() -> crate::Result<()> {
    let path = pid_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, std::process::id().to_string())?;
    Ok(())
}

fn remove_pid_file() {
    let _ = std::fs::remove_file(pid_path());
}

pub async fn run_daemon(config: Config) -> crate::Result<()> {
    write_pid_file()?;

    let mut sigterm = signal(SignalKind::terminate())
        .map_err(|e| crate::error::WagnerError::Transport(format!("signal handler: {e}")))?;
    let mut sigint = signal(SignalKind::interrupt())
        .map_err(|e| crate::error::WagnerError::Transport(format!("signal handler: {e}")))?;

    let terminal = Tmux::with_config(config.terminal.clone());
    let store = Store::new(config.clone());
    let mut core = WagnerCore::new(config.clone());
    core.register_plugin(Box::new(crate::plugins::chains::ChainsProvider));

    let mut adapter: DaemonAdapter = {
        #[cfg(feature = "telegram")]
        {
            if let Some(tg_config) = config.daemon.telegram.as_ref() {
                DaemonAdapter::Telegram(super::telegram::TelegramAdapter::new(tg_config)?)
            } else {
                info!("No Telegram configured, running with log transport");
                DaemonAdapter::Log(LogAdapter)
            }
        }
        #[cfg(not(feature = "telegram"))]
        {
            DaemonAdapter::Log(LogAdapter)
        }
    };

    let mut state = DaemonState {
        core,
        terminal,
        store,
        last_health_check: Instant::now(),
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

    for task in &tasks {
        let session_name = session_name_for_task(&task.name);
        state.core.status_engine.track_task(task, &session_name);
    }

    let startup_events = vec![CoreEvent::DaemonStarted { tasks: summaries }];
    adapter
        .handle_events(
            &startup_events,
            &state.core,
            &state.terminal,
            &state.store,
            &tasks,
        )
        .await?;

    info!(task_count = tasks.len(), "daemon started");

    let poll_interval = Duration::from_millis(config.daemon.poll_interval_ms);

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("received SIGTERM, shutting down");
                break;
            }
            _ = sigint.recv() => {
                info!("received SIGINT, shutting down");
                break;
            }
            _ = tokio::time::sleep(poll_interval) => {
                if let Err(e) = daemon_tick(&mut state, &mut adapter).await {
                    error!(%e, "daemon tick error");
                }
            }
        }
    }

    let stop_events = vec![CoreEvent::DaemonStopping];
    let tasks = state.store.list_tasks().unwrap_or_default();
    let _ = adapter
        .handle_events(&stop_events, &state.core, &state.terminal, &state.store, &tasks)
        .await;

    remove_pid_file();
    info!("daemon stopped");
    Ok(())
}

async fn daemon_tick(
    state: &mut DaemonState,
    adapter: &mut DaemonAdapter,
) -> crate::Result<()> {
    let tasks = state.store.list_tasks()?;

    // Poll all sessions and get debounced transition events
    let mut events = state.core.tick(&state.terminal, &tasks);

    // Dead-agent health check (throttled)
    let health_interval =
        Duration::from_millis(state.core.config.daemon.health_check_interval_ms);
    if state.last_health_check.elapsed() >= health_interval {
        state.last_health_check = Instant::now();
        let resume_events = check_dead_agents(&state.terminal, &tasks);
        events.extend(resume_events);
    }

    // Send events to adapter
    adapter
        .handle_events(&events, &state.core, &state.terminal, &state.store, &tasks)
        .await?;

    // Poll and handle commands from adapter
    adapter
        .poll_and_handle(&state.core, &state.terminal, &state.store, &tasks)
        .await?;

    Ok(())
}

fn check_dead_agents(
    terminal: &Tmux,
    tasks: &[crate::model::Task],
) -> Vec<CoreEvent> {
    let mut events = Vec::new();

    for task in tasks {
        for tracked in &task.panes {
            let pane_cmd = terminal
                .get_pane_command(&PaneHandle(tracked.pane_id.clone(), String::new()))
                .unwrap_or_default()
                .to_ascii_lowercase();
            if pane_cmd.contains(tracked.engine.process_name()) {
                continue;
            }

            let resume_cmd = tracked.engine.resume_command(&tracked.session_id);
            let pane = PaneHandle(tracked.pane_id.clone(), String::new());
            if let Err(e) = terminal.send_literal(&pane, &resume_cmd) {
                warn!(%e, task = %task.name, pane = %tracked.pane_id, "failed to send resume command");
                continue;
            }
            if let Err(e) = terminal.send_key(&pane, "Enter") {
                warn!(%e, task = %task.name, pane = %tracked.pane_id, "failed to execute resume");
                continue;
            }
            info!(task = %task.name, pane = %tracked.pane_id, "auto-resumed dead agent");

            let session_name = session_name_for_task(&task.name);
            let pane_title = terminal
                .list_panes(&crate::terminal::SessionHandle(session_name))
                .unwrap_or_default()
                .iter()
                .find(|p| p.0 == tracked.pane_id)
                .map(|p| p.1.clone())
                .unwrap_or_default();

            events.push(CoreEvent::AgentResumed {
                task_name: task.name.clone(),
                pane_id: tracked.pane_id.clone(),
                pane_title,
            });
        }
    }

    events
}
