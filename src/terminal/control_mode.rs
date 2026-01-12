use crate::error::{Result, WagnerError};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tracing::{debug, trace, warn};

const CONTROL_SESSION: &str = "wagner_control";

pub struct TmuxControlMode {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Arc<Mutex<Option<Sender<Result<String>>>>>,
    alive: Arc<AtomicBool>,
    timeout_ms: u64,
    _reader_handle: JoinHandle<()>,
}

impl TmuxControlMode {
    pub fn connect_with_timeout(timeout_ms: u64) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| WagnerError::Terminal(format!("Failed to open pty: {}", e)))?;

        let mut cmd = CommandBuilder::new("tmux");
        cmd.args(["-CC", "new-session", "-A", "-s", CONTROL_SESSION]);

        let _child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| WagnerError::Terminal(format!("Failed to spawn tmux -CC: {}", e)))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| WagnerError::Terminal(format!("Failed to get reader: {}", e)))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| WagnerError::Terminal(format!("Failed to get writer: {}", e)))?;

        let pending: Arc<Mutex<Option<Sender<Result<String>>>>> = Arc::new(Mutex::new(None));
        let alive = Arc::new(AtomicBool::new(true));

        let reader_pending = Arc::clone(&pending);
        let reader_alive = Arc::clone(&alive);

        let ready = Arc::new(AtomicBool::new(false));
        let reader_ready = Arc::clone(&ready);

        let reader_handle = thread::spawn(move || {
            Self::reader_loop(reader, reader_pending, reader_alive, reader_ready);
        });

        let start = std::time::Instant::now();
        while !ready.load(Ordering::SeqCst) {
            if start.elapsed().as_millis() > 2000 {
                warn!("control_mode startup timeout");
                return Err(WagnerError::Terminal("Control mode startup timeout".into()));
            }
            thread::sleep(Duration::from_millis(10));
        }

        Ok(Self {
            writer: Mutex::new(writer),
            pending,
            alive,
            timeout_ms,
            _reader_handle: reader_handle,
        })
    }

    fn reader_loop(
        reader: Box<dyn std::io::Read + Send>,
        pending: Arc<Mutex<Option<Sender<Result<String>>>>>,
        alive: Arc<AtomicBool>,
        ready: Arc<AtomicBool>,
    ) {
        let reader = BufReader::new(reader);
        let mut in_response = false;
        let mut current_output = String::new();
        let mut line_count = 0;
        let mut initialized = false;

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    warn!(error = %e, lines_read = line_count, "reader_loop read error");
                    alive.store(false, Ordering::SeqCst);
                    break;
                }
            };
            line_count += 1;
            trace!(line_num = line_count, line = %line, "reader got line");

            if line.starts_with("%begin ") {
                debug!("begin block");
                in_response = true;
                current_output.clear();
            } else if line.starts_with("%end ") {
                debug!(output_len = current_output.len(), initialized, "end block");
                if !initialized {
                    // First %end is from tmux session startup, signal ready
                    initialized = true;
                    ready.store(true, Ordering::SeqCst);
                    in_response = false;
                    current_output.clear();
                } else if in_response {
                    let output = current_output.trim().to_string();
                    if let Some(sender) = pending.lock().unwrap().take() {
                        let _ = sender.send(Ok(output));
                    }
                    in_response = false;
                    current_output.clear();
                }
            } else if line.starts_with("%error ") {
                debug!(output_len = current_output.len(), initialized, "error block");
                if !initialized {
                    // First response was an error during startup
                    initialized = true;
                    ready.store(true, Ordering::SeqCst);
                    in_response = false;
                    current_output.clear();
                } else if in_response {
                    let error_msg = current_output.trim().to_string();
                    let error_msg = if error_msg.is_empty() {
                        "tmux command failed".to_string()
                    } else {
                        error_msg
                    };
                    warn!(error = %error_msg, "command error");
                    if let Some(sender) = pending.lock().unwrap().take() {
                        let _ = sender.send(Err(WagnerError::Terminal(error_msg)));
                    }
                    in_response = false;
                    current_output.clear();
                }
            } else if line.starts_with("%") && line.chars().nth(1).is_some_and(|c| c.is_ascii_lowercase()) {
                // Ignore tmux notifications (%output, %session-changed, etc.)
                // But NOT pane IDs like %80 which start with %<digit>
                if line.starts_with("%exit") {
                    warn!(line = %line, "tmux sent %exit notification - server may be shutting down");
                }
            } else if in_response {
                if !current_output.is_empty() {
                    current_output.push('\n');
                }
                current_output.push_str(&line);
            }
        }

        warn!(lines_read = line_count, "reader_loop exited");
        alive.store(false, Ordering::SeqCst);
        if let Some(sender) = pending.lock().unwrap().take() {
            let _ = sender.send(Err(WagnerError::Terminal(
                "Control mode connection lost".into(),
            )));
        }
    }

    pub fn execute(&self, command: &str) -> Result<String> {
        if !self.is_alive() {
            return Err(WagnerError::Terminal("Control mode not connected".into()));
        }

        let (tx, rx) = mpsc::channel();
        *self.pending.lock().unwrap() = Some(tx);
        debug!(command, "sending command to control mode");

        {
            let mut writer = self.writer.lock().unwrap();
            if writeln!(writer, "{}", command).is_err() {
                *self.pending.lock().unwrap() = None;
                self.alive.store(false, Ordering::SeqCst);
                return Err(WagnerError::Terminal("Failed to write to tmux".into()));
            }
            if writer.flush().is_err() {
                *self.pending.lock().unwrap() = None;
                self.alive.store(false, Ordering::SeqCst);
                return Err(WagnerError::Terminal("Failed to flush tmux".into()));
            }
        }

        match rx.recv_timeout(Duration::from_millis(self.timeout_ms)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                *self.pending.lock().unwrap() = None;
                Err(WagnerError::Terminal("Command timed out".into()))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.alive.store(false, Ordering::SeqCst);
                Err(WagnerError::Terminal("Control mode disconnected".into()))
            }
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
}

impl Drop for TmuxControlMode {
    fn drop(&mut self) {
        // Gracefully detach before PTY cleanup to avoid SIGHUP cascading
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writeln!(writer, "detach-client");
            let _ = writer.flush();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
