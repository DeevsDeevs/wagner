use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

use crate::config::Config;
use crate::error::WagnerError;
use crate::transport::{CoreCommand, CoreResponse};

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcRequest {
    pub command: CoreCommand,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub response: CoreResponse,
}

pub type IpcCommandTx = mpsc::Sender<(CoreCommand, oneshot::Sender<CoreResponse>)>;
pub type IpcCommandRx = mpsc::Receiver<(CoreCommand, oneshot::Sender<CoreResponse>)>;

const MAX_PAYLOAD: usize = 1024 * 1024;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

pub fn socket_path() -> PathBuf {
    Config::config_dir().join("daemon.sock")
}

pub async fn run_ipc_server(listener: UnixListener, cmd_tx: IpcCommandTx) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let tx = cmd_tx.clone();
                tokio::spawn(async move {
                    let result =
                        tokio::time::timeout(CONNECTION_TIMEOUT, handle_connection(stream, tx))
                            .await;
                    if result.is_err() {
                        tracing::warn!("IPC connection timed out");
                    }
                });
            }
            Err(e) => tracing::warn!(%e, "IPC accept error"),
        }
    }
}

async fn handle_connection(mut stream: UnixStream, cmd_tx: IpcCommandTx) {
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).await.is_err() {
        return;
    }
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > MAX_PAYLOAD {
        let resp = IpcResponse {
            response: CoreResponse::Error {
                message: "Payload too large".into(),
            },
        };
        let _ = write_response(&mut stream, &resp).await;
        return;
    }

    let mut buf = vec![0u8; len];
    if stream.read_exact(&mut buf).await.is_err() {
        return;
    }

    let request: IpcRequest = match serde_json::from_slice(&buf) {
        Ok(r) => r,
        Err(e) => {
            let resp = IpcResponse {
                response: CoreResponse::Error {
                    message: format!("Invalid request: {e}"),
                },
            };
            let _ = write_response(&mut stream, &resp).await;
            return;
        }
    };

    let (resp_tx, resp_rx) = oneshot::channel();
    if cmd_tx.send((request.command, resp_tx)).await.is_err() {
        return;
    }

    if let Ok(response) = resp_rx.await {
        let _ = write_response(&mut stream, &IpcResponse { response }).await;
    }
}

async fn write_response(stream: &mut UnixStream, resp: &IpcResponse) -> std::io::Result<()> {
    let json = serde_json::to_vec(resp).map_err(|e| std::io::Error::other(e.to_string()))?;
    let len = (json.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(&json).await?;
    stream.flush().await?;
    Ok(())
}

pub fn is_daemon_running() -> bool {
    let pid_path = super::daemon::pid_path();
    if !pid_path.exists() {
        return false;
    }
    let pid_str = match std::fs::read_to_string(&pid_path) {
        Ok(s) => s.trim().to_string(),
        Err(_) => return false,
    };

    let alive = std::process::Command::new("kill")
        .args(["-0", &pid_str])
        .status()
        .is_ok_and(|s| s.success());
    if !alive {
        let _ = std::fs::remove_file(&pid_path);
        let _ = std::fs::remove_file(socket_path());
        return false;
    }

    std::os::unix::net::UnixStream::connect(socket_path()).is_ok()
}

pub fn ensure_daemon_running() -> crate::Result<()> {
    if is_daemon_running() {
        return Ok(());
    }

    let log_path = Config::config_dir().join("daemon.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let err_file = log_file.try_clone()?;

    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["daemon", "start", "--foreground"])
        .stdout(log_file)
        .stderr(err_file)
        .stdin(std::process::Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    cmd.spawn()
        .map_err(|e| WagnerError::Transport(format!("Failed to start daemon: {e}")))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if std::os::unix::net::UnixStream::connect(socket_path()).is_ok() {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(WagnerError::Transport(
                "Daemon started but socket not ready after 3s".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn daemon_execute(cmd: CoreCommand) -> crate::Result<CoreResponse> {
    ensure_daemon_running()?;
    send_command(cmd)
}

pub fn send_command(cmd: CoreCommand) -> crate::Result<CoreResponse> {
    use std::io::{Read, Write};

    let sock = socket_path();
    let mut stream = std::os::unix::net::UnixStream::connect(&sock)
        .map_err(|e| WagnerError::Transport(format!("Cannot connect to daemon: {e}")))?;

    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let request = IpcRequest { command: cmd };
    let json = serde_json::to_vec(&request)?;
    let len = (json.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&json)?;
    stream.flush()?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let resp_len = u32::from_be_bytes(len_buf) as usize;

    if resp_len > MAX_PAYLOAD {
        return Err(WagnerError::Transport("Response too large".into()));
    }

    let mut buf = vec![0u8; resp_len];
    stream.read_exact(&mut buf)?;

    let resp: IpcResponse = serde_json::from_slice(&buf)?;
    Ok(resp.response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ipc_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");
        let listener = UnixListener::bind(&sock).unwrap();

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<(CoreCommand, oneshot::Sender<CoreResponse>)>(8);

        // Spawn server
        tokio::spawn(async move {
            run_ipc_server(listener, cmd_tx).await;
        });

        // Spawn handler that echoes back a TaskList
        tokio::spawn(async move {
            while let Some((cmd, resp_tx)) = cmd_rx.recv().await {
                let resp = match cmd {
                    CoreCommand::ListTasks => CoreResponse::TaskList { tasks: vec![] },
                    _ => CoreResponse::Error {
                        message: "unexpected".into(),
                    },
                };
                let _ = resp_tx.send(resp);
            }
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Sync client in blocking task
        let sock_clone = sock.clone();
        let result = tokio::task::spawn_blocking(move || {
            use std::io::{Read, Write};

            let mut stream = std::os::unix::net::UnixStream::connect(&sock_clone).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();

            let req = IpcRequest {
                command: CoreCommand::ListTasks,
            };
            let json = serde_json::to_vec(&req).unwrap();
            let len = (json.len() as u32).to_be_bytes();

            stream.write_all(&len).unwrap();
            stream.write_all(&json).unwrap();
            stream.flush().unwrap();

            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf).unwrap();
            let resp_len = u32::from_be_bytes(len_buf) as usize;

            let mut buf = vec![0u8; resp_len];
            stream.read_exact(&mut buf).unwrap();

            let resp: IpcResponse = serde_json::from_slice(&buf).unwrap();
            resp.response
        })
        .await
        .unwrap();

        match result {
            CoreResponse::TaskList { tasks } => assert!(tasks.is_empty()),
            other => panic!("expected TaskList, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn ipc_malformed_request() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");
        let listener = UnixListener::bind(&sock).unwrap();

        let (cmd_tx, _cmd_rx) = mpsc::channel::<(CoreCommand, oneshot::Sender<CoreResponse>)>(8);

        tokio::spawn(async move {
            run_ipc_server(listener, cmd_tx).await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let sock_clone = sock.clone();
        let result = tokio::task::spawn_blocking(move || {
            use std::io::{Read, Write};

            let mut stream = std::os::unix::net::UnixStream::connect(&sock_clone).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();

            let garbage = b"not valid json";
            let len = (garbage.len() as u32).to_be_bytes();
            stream.write_all(&len).unwrap();
            stream.write_all(garbage).unwrap();
            stream.flush().unwrap();

            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf).unwrap();
            let resp_len = u32::from_be_bytes(len_buf) as usize;

            let mut buf = vec![0u8; resp_len];
            stream.read_exact(&mut buf).unwrap();

            let resp: IpcResponse = serde_json::from_slice(&buf).unwrap();
            resp.response
        })
        .await
        .unwrap();

        match result {
            CoreResponse::Error { message } => {
                assert!(message.contains("Invalid request"), "got: {message}");
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn ipc_payload_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");
        let listener = UnixListener::bind(&sock).unwrap();

        let (cmd_tx, _cmd_rx) = mpsc::channel::<(CoreCommand, oneshot::Sender<CoreResponse>)>(8);

        tokio::spawn(async move {
            run_ipc_server(listener, cmd_tx).await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let sock_clone = sock.clone();
        let result = tokio::task::spawn_blocking(move || {
            use std::io::{Read, Write};

            let mut stream = std::os::unix::net::UnixStream::connect(&sock_clone).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();

            // Claim 2MB payload (over the 1MB limit)
            let fake_len = (2 * 1024 * 1024_u32).to_be_bytes();
            stream.write_all(&fake_len).unwrap();
            stream.flush().unwrap();

            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf).unwrap();
            let resp_len = u32::from_be_bytes(len_buf) as usize;

            let mut buf = vec![0u8; resp_len];
            stream.read_exact(&mut buf).unwrap();

            let resp: IpcResponse = serde_json::from_slice(&buf).unwrap();
            resp.response
        })
        .await
        .unwrap();

        match result {
            CoreResponse::Error { message } => {
                assert!(message.contains("too large"), "got: {message}");
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn ipc_concurrent_clients() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");
        let listener = UnixListener::bind(&sock).unwrap();

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<(CoreCommand, oneshot::Sender<CoreResponse>)>(8);

        tokio::spawn(async move {
            run_ipc_server(listener, cmd_tx).await;
        });

        // Handler that responds differently based on command
        tokio::spawn(async move {
            while let Some((cmd, resp_tx)) = cmd_rx.recv().await {
                let resp = match cmd {
                    CoreCommand::ListTasks => CoreResponse::TaskList { tasks: vec![] },
                    CoreCommand::Help => CoreResponse::HelpText,
                    _ => CoreResponse::Error {
                        message: "unexpected".into(),
                    },
                };
                let _ = resp_tx.send(resp);
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let sock1 = sock.clone();
        let sock2 = sock.clone();

        let (r1, r2) = tokio::join!(
            tokio::task::spawn_blocking(move || {
                use std::io::{Read, Write};

                let mut stream = std::os::unix::net::UnixStream::connect(&sock1).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();

                let req = IpcRequest {
                    command: CoreCommand::ListTasks,
                };
                let json = serde_json::to_vec(&req).unwrap();
                let len = (json.len() as u32).to_be_bytes();
                stream.write_all(&len).unwrap();
                stream.write_all(&json).unwrap();
                stream.flush().unwrap();

                let mut len_buf = [0u8; 4];
                stream.read_exact(&mut len_buf).unwrap();
                let resp_len = u32::from_be_bytes(len_buf) as usize;
                let mut buf = vec![0u8; resp_len];
                stream.read_exact(&mut buf).unwrap();
                serde_json::from_slice::<IpcResponse>(&buf)
                    .unwrap()
                    .response
            }),
            tokio::task::spawn_blocking(move || {
                use std::io::{Read, Write};

                let mut stream = std::os::unix::net::UnixStream::connect(&sock2).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();

                let req = IpcRequest {
                    command: CoreCommand::Help,
                };
                let json = serde_json::to_vec(&req).unwrap();
                let len = (json.len() as u32).to_be_bytes();
                stream.write_all(&len).unwrap();
                stream.write_all(&json).unwrap();
                stream.flush().unwrap();

                let mut len_buf = [0u8; 4];
                stream.read_exact(&mut len_buf).unwrap();
                let resp_len = u32::from_be_bytes(len_buf) as usize;
                let mut buf = vec![0u8; resp_len];
                stream.read_exact(&mut buf).unwrap();
                serde_json::from_slice::<IpcResponse>(&buf)
                    .unwrap()
                    .response
            }),
        );

        match r1.unwrap() {
            CoreResponse::TaskList { tasks } => assert!(tasks.is_empty()),
            other => panic!("client 1: expected TaskList, got {:?}", other),
        }
        match r2.unwrap() {
            CoreResponse::HelpText => {}
            other => panic!("client 2: expected HelpText, got {:?}", other),
        }
    }
}
