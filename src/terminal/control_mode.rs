use crate::error::{Result, WagnerError};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const CONTROL_SESSION: &str = "wagner_control";

pub struct TmuxControlMode {
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<u64, Sender<Result<String>>>>>,
    next_cmd_num: AtomicU64,
    alive: Arc<AtomicBool>,
    timeout_ms: u64,
    _reader_handle: JoinHandle<()>,
    _child: Mutex<Child>,
}

impl TmuxControlMode {
    pub fn connect_with_timeout(timeout_ms: u64) -> Result<Self> {
        let mut child = Command::new("tmux")
            .args(["-CC", "new-session", "-A", "-s", CONTROL_SESSION])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| WagnerError::Terminal(format!("Failed to spawn tmux -CC: {}", e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| WagnerError::Terminal("Failed to get stdin".into()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| WagnerError::Terminal("Failed to get stdout".into()))?;

        let pending: Arc<Mutex<HashMap<u64, Sender<Result<String>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));

        let reader_pending = Arc::clone(&pending);
        let reader_alive = Arc::clone(&alive);

        let reader_handle = thread::spawn(move || {
            Self::reader_loop(stdout, reader_pending, reader_alive);
        });

        Ok(Self {
            stdin: Mutex::new(stdin),
            pending,
            next_cmd_num: AtomicU64::new(1),
            alive,
            timeout_ms,
            _reader_handle: reader_handle,
            _child: Mutex::new(child),
        })
    }

    fn reader_loop(
        stdout: ChildStdout,
        pending: Arc<Mutex<HashMap<u64, Sender<Result<String>>>>>,
        alive: Arc<AtomicBool>,
    ) {
        let reader = BufReader::new(stdout);
        let mut current_cmd: Option<u64> = None;
        let mut current_output = String::new();

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => {
                    alive.store(false, Ordering::SeqCst);
                    break;
                }
            };

            if line.starts_with("%begin ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    if let Ok(cmd_num) = parts[2].parse::<u64>() {
                        current_cmd = Some(cmd_num);
                        current_output.clear();
                    }
                }
            } else if line.starts_with("%end ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    if let Ok(cmd_num) = parts[2].parse::<u64>() {
                        if current_cmd == Some(cmd_num) {
                            let output = current_output.trim().to_string();
                            if let Some(sender) = pending.lock().unwrap().remove(&cmd_num) {
                                let _ = sender.send(Ok(output));
                            }
                            current_cmd = None;
                            current_output.clear();
                        }
                    }
                }
            } else if line.starts_with("%error ") {
                if let Some(cmd_num) = current_cmd {
                    let error_msg = line.strip_prefix("%error ").unwrap_or(&line).to_string();
                    if let Some(sender) = pending.lock().unwrap().remove(&cmd_num) {
                        let _ = sender.send(Err(WagnerError::Terminal(error_msg)));
                    }
                    current_cmd = None;
                    current_output.clear();
                }
            } else if line.starts_with("%output ") || line.starts_with("%") {
                // Ignore notifications for now (%output, %session-changed, etc.)
            } else if current_cmd.is_some() {
                if !current_output.is_empty() {
                    current_output.push('\n');
                }
                current_output.push_str(&line);
            }
        }

        alive.store(false, Ordering::SeqCst);
        let mut pending = pending.lock().unwrap();
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err(WagnerError::Terminal("Control mode connection lost".into())));
        }
    }

    pub fn execute(&self, command: &str) -> Result<String> {
        if !self.is_alive() {
            return Err(WagnerError::Terminal("Control mode not connected".into()));
        }

        let cmd_num = self.next_cmd_num.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();

        self.pending.lock().unwrap().insert(cmd_num, tx);

        {
            let mut stdin = self.stdin.lock().unwrap();
            if writeln!(stdin, "{}", command).is_err() {
                self.pending.lock().unwrap().remove(&cmd_num);
                self.alive.store(false, Ordering::SeqCst);
                return Err(WagnerError::Terminal("Failed to write to tmux".into()));
            }
            if stdin.flush().is_err() {
                self.pending.lock().unwrap().remove(&cmd_num);
                self.alive.store(false, Ordering::SeqCst);
                return Err(WagnerError::Terminal("Failed to flush tmux stdin".into()));
            }
        }

        match rx.recv_timeout(Duration::from_millis(self.timeout_ms)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.pending.lock().unwrap().remove(&cmd_num);
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
        if let Ok(mut stdin) = self.stdin.lock() {
            let _ = writeln!(stdin);
            let _ = stdin.flush();
        }
        if let Ok(mut child) = self._child.lock() {
            let _ = child.kill();
        }
    }
}
