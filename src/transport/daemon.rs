use std::path::PathBuf;
use std::time::Duration;

use tokio::signal::unix::{SignalKind, signal};
use tracing::{error, info};

use crate::config::Config;
use crate::core::WagnerCore;
use crate::store::Store;
use crate::terminal::{Tmux, session_name_for_task};

use super::adapter::{Adapter, DaemonAdapter, LogAdapter};
use super::{CoreEvent, TaskSummary};

struct DaemonState {
    core: WagnerCore,
    terminal: Tmux,
    store: Store,
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
    let events = state.core.tick(&state.terminal, &tasks);

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
