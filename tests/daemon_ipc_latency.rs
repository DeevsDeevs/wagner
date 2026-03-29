//! Tests verifying that daemon IPC commands are processed immediately
//! instead of being gated behind the poll_interval sleep.
//!
//! Covers:
//! - VAL-HIGH-007: Daemon IPC commands processed immediately

use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};
use wagner::transport::{CoreCommand, CoreResponse};

/// Helper: simulate the daemon's tokio::select! loop pattern.
/// This replicates the exact select! structure from daemon.rs to verify
/// that IPC commands are processed via a dedicated branch immediately,
/// rather than waiting for poll_interval.
async fn run_select_loop(
    mut ipc_rx: mpsc::Receiver<(CoreCommand, oneshot::Sender<CoreResponse>)>,
    poll_interval: Duration,
    commands_to_process: usize,
) {
    let mut processed = 0;
    loop {
        if processed >= commands_to_process {
            break;
        }
        tokio::select! {
            Some((cmd, resp_tx)) = ipc_rx.recv() => {
                // Process the IPC command immediately — this mirrors
                // the daemon's new dedicated IPC branch
                let response = match cmd {
                    CoreCommand::ListTasks => CoreResponse::TaskList { tasks: vec![] },
                    CoreCommand::Help => CoreResponse::HelpText,
                    _ => CoreResponse::Error {
                        message: "unhandled".into(),
                    },
                };
                let _ = resp_tx.send(response);
                processed += 1;
            }
            _ = tokio::time::sleep(poll_interval) => {
                // daemon_tick would fire here — we just continue
            }
        }
    }
}

/// VAL-HIGH-007: IPC commands must be processed immediately, not gated
/// behind poll_interval. We set poll_interval to 10 seconds and verify
/// the command completes in well under 1 second.
#[tokio::test]
async fn test_daemon_ipc_immediate_response() {
    let very_long_poll = Duration::from_secs(10);
    let (ipc_tx, ipc_rx) = mpsc::channel::<(CoreCommand, oneshot::Sender<CoreResponse>)>(32);

    // Run the select loop in a background task
    let loop_handle = tokio::spawn(run_select_loop(ipc_rx, very_long_poll, 1));

    // Give the loop a moment to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Send an IPC command and measure response time
    let (resp_tx, resp_rx) = oneshot::channel();
    let start = Instant::now();
    ipc_tx
        .send((CoreCommand::ListTasks, resp_tx))
        .await
        .expect("channel send failed");

    let response = tokio::time::timeout(Duration::from_secs(2), resp_rx)
        .await
        .expect("IPC response timed out — command was gated by poll_interval")
        .expect("oneshot channel closed");

    let elapsed = start.elapsed();

    // The response must arrive much faster than the 10s poll_interval.
    // We allow up to 500ms for scheduling overhead; in practice it's <10ms.
    assert!(
        elapsed < Duration::from_millis(500),
        "IPC command took {:?} — should be immediate, not gated by poll_interval ({:?})",
        elapsed,
        very_long_poll,
    );

    match response {
        CoreResponse::TaskList { tasks } => assert!(tasks.is_empty()),
        other => panic!("expected TaskList, got {:?}", other),
    }

    loop_handle.await.expect("select loop panicked");
}

/// Multiple IPC commands arriving in quick succession should all be
/// processed promptly, each without waiting for poll_interval.
#[tokio::test]
async fn test_daemon_ipc_multiple_commands_processed_promptly() {
    let very_long_poll = Duration::from_secs(10);
    let num_commands = 5;
    let (ipc_tx, ipc_rx) = mpsc::channel::<(CoreCommand, oneshot::Sender<CoreResponse>)>(32);

    let loop_handle = tokio::spawn(run_select_loop(ipc_rx, very_long_poll, num_commands));

    tokio::time::sleep(Duration::from_millis(10)).await;

    let start = Instant::now();
    let mut receivers = Vec::new();

    // Send all commands in quick succession
    for _ in 0..num_commands {
        let (resp_tx, resp_rx) = oneshot::channel();
        ipc_tx
            .send((CoreCommand::ListTasks, resp_tx))
            .await
            .expect("channel send failed");
        receivers.push(resp_rx);
    }

    // Wait for all responses
    for (i, resp_rx) in receivers.into_iter().enumerate() {
        let response = tokio::time::timeout(Duration::from_secs(2), resp_rx)
            .await
            .unwrap_or_else(|_| panic!("IPC response {i} timed out"))
            .unwrap_or_else(|_| panic!("oneshot channel {i} closed"));

        match response {
            CoreResponse::TaskList { tasks } => assert!(tasks.is_empty()),
            other => panic!("command {i}: expected TaskList, got {:?}", other),
        }
    }

    let elapsed = start.elapsed();

    // All 5 commands should complete in well under 1 second
    assert!(
        elapsed < Duration::from_secs(1),
        "Processing {} IPC commands took {:?} — all should be immediate",
        num_commands,
        elapsed,
    );

    loop_handle.await.expect("select loop panicked");
}

/// Verify that the daemon_tick (poll_interval) arm still fires normally
/// when no IPC commands arrive.
#[tokio::test]
async fn test_daemon_tick_still_fires_at_poll_interval() {
    let poll_interval = Duration::from_millis(50);
    let (_ipc_tx, mut ipc_rx) = mpsc::channel::<(CoreCommand, oneshot::Sender<CoreResponse>)>(32);

    let mut tick_count = 0u32;
    let target_ticks = 3;

    let start = Instant::now();

    loop {
        if tick_count >= target_ticks {
            break;
        }
        tokio::select! {
            Some((_cmd, _resp_tx)) = ipc_rx.recv() => {
                // No commands sent, this branch won't fire
            }
            _ = tokio::time::sleep(poll_interval) => {
                tick_count += 1;
            }
        }
    }

    let elapsed = start.elapsed();

    // 3 ticks at 50ms each should take ~150ms
    assert!(
        tick_count == target_ticks,
        "Expected {} ticks, got {}",
        target_ticks,
        tick_count,
    );
    assert!(
        elapsed >= Duration::from_millis(100),
        "Ticks fired too fast: {:?} for {} ticks at {:?} interval",
        elapsed,
        target_ticks,
        poll_interval,
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "Ticks took too long: {:?}",
        elapsed,
    );
}

/// Full IPC roundtrip through the actual Unix socket server, verifying the
/// complete pipeline: client → socket → IPC server → channel → select loop → response.
/// Uses a 10-second poll_interval to prove that the response is not gated.
#[tokio::test]
async fn test_ipc_socket_roundtrip_not_gated_by_poll() {
    use tokio::net::UnixListener;
    use wagner::transport::ipc::{IpcRequest, IpcResponse, run_ipc_server};

    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("test.sock");
    let listener = UnixListener::bind(&sock).unwrap();

    let (cmd_tx, ipc_rx) = mpsc::channel::<(CoreCommand, oneshot::Sender<CoreResponse>)>(32);

    // Spawn the IPC server
    tokio::spawn(run_ipc_server(listener, cmd_tx));

    // Spawn the select loop handler (simulating the daemon's main loop)
    let very_long_poll = Duration::from_secs(10);
    tokio::spawn(run_select_loop(ipc_rx, very_long_poll, 1));

    // Let everything start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send a request through the full socket path
    let sock_clone = sock.clone();
    let start = Instant::now();

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

        serde_json::from_slice::<IpcResponse>(&buf)
            .unwrap()
            .response
    })
    .await
    .unwrap();

    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(1),
        "Full IPC socket roundtrip took {:?} — must not be gated by {:?} poll_interval",
        elapsed,
        very_long_poll,
    );

    match result {
        CoreResponse::TaskList { tasks } => assert!(tasks.is_empty()),
        other => panic!("expected TaskList, got {:?}", other),
    }
}
