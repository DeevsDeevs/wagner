use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::net::UnixListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

use crate::config::Config;
use crate::core::WagnerCore;
use crate::store::Store;
use crate::terminal::{Tmux, session_name_for_task};
use crate::transport::{CoreCommand, CoreResponse};

use super::adapter::{Adapter, DaemonAdapter, LogAdapter};
use super::ipc;
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

fn cleanup_stale_socket(sock_path: &Path) {
    if !sock_path.exists() {
        return;
    }
    let pid_file = pid_path();
    if pid_file.exists()
        && let Ok(pid_str) = std::fs::read_to_string(&pid_file)
    {
        let alive = std::process::Command::new("kill")
            .args(["-0", pid_str.trim()])
            .status()
            .is_ok_and(|s| s.success());
        if alive {
            return;
        }
    }
    let _ = std::fs::remove_file(sock_path);
    let _ = std::fs::remove_file(&pid_file);
}

pub async fn run_daemon(config: Config) -> crate::Result<()> {
    let sock_path = ipc::socket_path();
    cleanup_stale_socket(&sock_path);
    let listener = UnixListener::bind(&sock_path).map_err(|e| {
        crate::error::WagnerError::Transport(format!(
            "Cannot bind daemon socket (another daemon running?): {e}"
        ))
    })?;

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
                DaemonAdapter::Telegram(Box::new(super::telegram::TelegramAdapter::new(tg_config)?))
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

    let (ipc_tx, mut ipc_rx) = mpsc::channel::<(CoreCommand, oneshot::Sender<CoreResponse>)>(32);

    tokio::spawn(async move {
        ipc::run_ipc_server(listener, ipc_tx).await;
    });

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

                while let Ok((cmd, resp_tx)) = ipc_rx.try_recv() {
                    let tasks = state.store.list_tasks().unwrap_or_default();
                    let response = state.core.execute(
                        &state.terminal,
                        &state.store,
                        &cmd,
                        &tasks,
                    );
                    let _ = resp_tx.send(response);
                }
            }
        }
    }

    let stop_events = vec![CoreEvent::DaemonStopping];
    let tasks = state.store.list_tasks().unwrap_or_default();
    let _ = adapter
        .handle_events(
            &stop_events,
            &state.core,
            &state.terminal,
            &state.store,
            &tasks,
        )
        .await;

    remove_pid_file();
    let _ = std::fs::remove_file(&sock_path);
    info!("daemon stopped");
    Ok(())
}

async fn daemon_tick(state: &mut DaemonState, adapter: &mut DaemonAdapter) -> crate::Result<()> {
    let tasks = state.store.list_tasks()?;

    let events = state.core.tick(&state.terminal, &tasks);

    adapter
        .handle_events(&events, &state.core, &state.terminal, &state.store, &tasks)
        .await?;

    adapter
        .poll_and_handle(&state.core, &state.terminal, &state.store, &tasks)
        .await?;

    Ok(())
}
