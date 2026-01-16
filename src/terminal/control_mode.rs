use crate::error::{Result, WagnerError};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tracing::{debug, trace, warn};

fn control_session_name() -> String {
    format!("wagner_ctl_{}", std::process::id())
}

#[derive(Debug, Clone, PartialEq)]
enum LineAction {
    BeginBlock(u64),
    EndBlock(u64),
    ErrorBlock(u64),
    Notification,
    Content(String),
}

fn parse_command_num(line: &str) -> Option<u64> {
    line.split_whitespace().nth(2)?.parse().ok()
}

fn parse_line(line: &str) -> LineAction {
    if line.starts_with("%begin ") {
        let num = parse_command_num(line).unwrap_or(0);
        LineAction::BeginBlock(num)
    } else if line.starts_with("%end ") {
        let num = parse_command_num(line).unwrap_or(0);
        LineAction::EndBlock(num)
    } else if line.starts_with("%error ") {
        let num = parse_command_num(line).unwrap_or(0);
        LineAction::ErrorBlock(num)
    } else if line.starts_with('%') && line.chars().nth(1).is_some_and(|c| c.is_ascii_lowercase()) {
        LineAction::Notification
    } else {
        LineAction::Content(line.to_string())
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ParseResult {
    Ready,
    Output(u64, String),
    Error(u64, String),
    Continue,
}

struct ProtocolParser {
    current_cmd: u64,
    in_response: bool,
    current_output: String,
    initialized: bool,
}

impl Default for ProtocolParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolParser {
    fn new() -> Self {
        Self {
            current_cmd: 0,
            in_response: false,
            current_output: String::new(),
            initialized: false,
        }
    }

    fn process_line(&mut self, line: &str) -> ParseResult {
        match parse_line(line) {
            LineAction::BeginBlock(num) => {
                self.current_cmd = num;
                self.in_response = true;
                self.current_output.clear();
                ParseResult::Continue
            }
            LineAction::EndBlock(num) => {
                if !self.initialized {
                    self.initialized = true;
                    self.in_response = false;
                    self.current_output.clear();
                    ParseResult::Ready
                } else if self.in_response {
                    let output = self.current_output.trim().to_string();
                    self.in_response = false;
                    self.current_output.clear();
                    ParseResult::Output(num, output)
                } else {
                    ParseResult::Continue
                }
            }
            LineAction::ErrorBlock(num) => {
                if !self.initialized {
                    self.initialized = true;
                    self.in_response = false;
                    self.current_output.clear();
                    ParseResult::Ready
                } else if self.in_response {
                    let error_msg = self.current_output.trim().to_string();
                    let error_msg = if error_msg.is_empty() {
                        "tmux command failed".to_string()
                    } else {
                        error_msg
                    };
                    self.in_response = false;
                    self.current_output.clear();
                    ParseResult::Error(num, error_msg)
                } else {
                    ParseResult::Continue
                }
            }
            LineAction::Notification => ParseResult::Continue,
            LineAction::Content(content) => {
                if self.in_response {
                    if !self.current_output.is_empty() {
                        self.current_output.push('\n');
                    }
                    self.current_output.push_str(&content);
                }
                ParseResult::Continue
            }
        }
    }
}

struct CommandChannel {
    writer: ChildStdin,
    response_rx: mpsc::Receiver<Result<String>>,
}

pub struct TmuxControlMode {
    channel: Mutex<CommandChannel>,
    _response_tx: Sender<Result<String>>,
    alive: Arc<AtomicBool>,
    timeout_ms: u64,
    session_name: String,
    _reader_handle: JoinHandle<()>,
    _child: Mutex<Child>,
}

impl TmuxControlMode {
    pub fn connect_with_timeout(timeout_ms: u64) -> Result<Self> {
        let session_name = control_session_name();
        let mut child = Command::new("tmux")
            .args(["-C", "new-session", "-A", "-s", &session_name])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| WagnerError::Terminal(format!("Failed to spawn tmux -C: {}", e)))?;

        let writer = child
            .stdin
            .take()
            .ok_or_else(|| WagnerError::Terminal("Failed to get stdin".into()))?;

        let reader = child
            .stdout
            .take()
            .ok_or_else(|| WagnerError::Terminal("Failed to get stdout".into()))?;

        let (response_tx, response_rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));

        let reader_tx = response_tx.clone();
        let reader_alive = Arc::clone(&alive);

        let ready = Arc::new(AtomicBool::new(false));
        let reader_ready = Arc::clone(&ready);

        let reader_handle = thread::spawn(move || {
            Self::reader_loop(reader, reader_tx, reader_alive, reader_ready);
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
            channel: Mutex::new(CommandChannel {
                writer,
                response_rx,
            }),
            _response_tx: response_tx,
            alive,
            timeout_ms,
            session_name,
            _reader_handle: reader_handle,
            _child: Mutex::new(child),
        })
    }

    fn reader_loop(
        reader: impl std::io::Read,
        response_tx: Sender<Result<String>>,
        alive: Arc<AtomicBool>,
        ready: Arc<AtomicBool>,
    ) {
        let reader = BufReader::new(reader);
        let mut parser = ProtocolParser::new();
        let mut line_count = 0;

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

            match parser.process_line(&line) {
                ParseResult::Ready => {
                    debug!("control mode ready");
                    ready.store(true, Ordering::SeqCst);
                }
                ParseResult::Output(cmd_num, output) => {
                    debug!(cmd = cmd_num, output_len = output.len(), "command output");
                    let _ = response_tx.send(Ok(output));
                }
                ParseResult::Error(cmd_num, error_msg) => {
                    warn!(cmd = cmd_num, error = %error_msg, "command error");
                    let _ = response_tx.send(Err(WagnerError::Terminal(error_msg)));
                }
                ParseResult::Continue => {
                    if line.starts_with("%exit") {
                        warn!(line = %line, "tmux sent %exit notification - server may be shutting down");
                    }
                }
            }
        }

        warn!(lines_read = line_count, "reader_loop exited");
        alive.store(false, Ordering::SeqCst);
    }

    pub fn execute(&self, command: &str) -> Result<String> {
        if !self.is_alive() {
            return Err(WagnerError::Terminal("Control mode not connected".into()));
        }

        debug!(command, "sending command to control mode");

        let mut channel = self.channel.lock().unwrap();

        if writeln!(channel.writer, "{}", command).is_err() {
            self.alive.store(false, Ordering::SeqCst);
            return Err(WagnerError::Terminal("Failed to write to tmux".into()));
        }
        if channel.writer.flush().is_err() {
            self.alive.store(false, Ordering::SeqCst);
            return Err(WagnerError::Terminal("Failed to flush tmux".into()));
        }

        match channel
            .response_rx
            .recv_timeout(Duration::from_millis(self.timeout_ms))
        {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
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
        if let Ok(mut channel) = self.channel.lock() {
            let _ = writeln!(channel.writer, "detach-client");
            let _ = channel.writer.flush();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &self.session_name])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_notification_line(line: &str) -> bool {
        line.starts_with('%') && line.chars().nth(1).is_some_and(|c| c.is_ascii_lowercase())
    }

    #[test]
    fn parse_line_begin_block() {
        assert_eq!(
            parse_line("%begin 1234567890 1 0"),
            LineAction::BeginBlock(1)
        );
    }

    #[test]
    fn parse_line_end_block() {
        assert_eq!(parse_line("%end 1234567890 1 0"), LineAction::EndBlock(1));
    }

    #[test]
    fn parse_line_error_block() {
        assert_eq!(
            parse_line("%error 1234567890 1 0"),
            LineAction::ErrorBlock(1)
        );
    }

    #[test]
    fn parse_line_output_notification() {
        assert_eq!(
            parse_line("%output %42 some output text"),
            LineAction::Notification
        );
    }

    #[test]
    fn parse_line_session_changed_notification() {
        assert_eq!(
            parse_line("%session-changed $1 mysession"),
            LineAction::Notification
        );
    }

    #[test]
    fn parse_line_exit_notification() {
        assert_eq!(parse_line("%exit"), LineAction::Notification);
    }

    #[test]
    fn parse_line_pane_id_not_filtered() {
        assert_eq!(parse_line("%80"), LineAction::Content("%80".to_string()));
    }

    #[test]
    fn parse_line_pane_id_with_colon_not_filtered() {
        assert_eq!(parse_line("%0:"), LineAction::Content("%0:".to_string()));
    }

    #[test]
    fn parse_line_percent_digit_sequence() {
        assert_eq!(parse_line("%123"), LineAction::Content("%123".to_string()));
    }

    #[test]
    fn parse_line_regular_content() {
        assert_eq!(
            parse_line("some regular output"),
            LineAction::Content("some regular output".to_string())
        );
    }

    #[test]
    fn parse_line_empty_content() {
        assert_eq!(parse_line(""), LineAction::Content("".to_string()));
    }

    #[test]
    fn parse_line_percent_uppercase_not_notification() {
        assert_eq!(
            parse_line("%SOMETHING"),
            LineAction::Content("%SOMETHING".to_string())
        );
    }

    #[test]
    fn is_notification_line_lowercase_after_percent() {
        assert!(is_notification_line("%output foo"));
        assert!(is_notification_line("%session-changed bar"));
        assert!(is_notification_line("%exit"));
        assert!(is_notification_line("%window-renamed"));
    }

    #[test]
    fn is_notification_line_digit_after_percent() {
        assert!(!is_notification_line("%0"));
        assert!(!is_notification_line("%80"));
        assert!(!is_notification_line("%123"));
    }

    #[test]
    fn is_notification_line_uppercase_after_percent() {
        assert!(!is_notification_line("%A"));
        assert!(!is_notification_line("%BEGIN"));
    }

    #[test]
    fn is_notification_line_no_percent() {
        assert!(!is_notification_line("output"));
        assert!(!is_notification_line(""));
    }

    #[test]
    fn parser_initialization_on_first_end() {
        let mut parser = ProtocolParser::new();

        assert!(!parser.initialized);

        assert_eq!(
            parser.process_line("%begin 1234567890 1 0"),
            ParseResult::Continue
        );
        assert_eq!(
            parser.process_line("%end 1234567890 1 0"),
            ParseResult::Ready
        );
        assert!(parser.initialized);
    }

    #[test]
    fn parser_initialization_on_first_error() {
        let mut parser = ProtocolParser::new();

        assert!(!parser.initialized);

        assert_eq!(
            parser.process_line("%begin 1234567890 1 0"),
            ParseResult::Continue
        );
        assert_eq!(
            parser.process_line("%error 1234567890 1 0"),
            ParseResult::Ready
        );
        assert!(parser.initialized);
    }

    #[test]
    fn parser_successful_command_response() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        assert_eq!(
            parser.process_line("%begin 1234567890 1 0"),
            ParseResult::Continue
        );
        assert_eq!(parser.process_line("%42"), ParseResult::Continue);
        assert_eq!(
            parser.process_line("%end 1234567890 1 0"),
            ParseResult::Output(1, "%42".to_string())
        );
    }

    #[test]
    fn parser_multiline_output() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        parser.process_line("%begin 1 1 0");
        parser.process_line("line1");
        parser.process_line("line2");
        parser.process_line("line3");

        assert_eq!(
            parser.process_line("%end 1 1 0"),
            ParseResult::Output(1, "line1\nline2\nline3".to_string())
        );
    }

    #[test]
    fn parser_empty_output() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        parser.process_line("%begin 1 1 0");
        assert_eq!(
            parser.process_line("%end 1 1 0"),
            ParseResult::Output(1, "".to_string())
        );
    }

    #[test]
    fn parser_error_response_with_message() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        parser.process_line("%begin 1 1 0");
        parser.process_line("session not found: nonexistent");

        assert_eq!(
            parser.process_line("%error 1 1 0"),
            ParseResult::Error(1, "session not found: nonexistent".to_string())
        );
    }

    #[test]
    fn parser_error_response_empty_uses_default() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        parser.process_line("%begin 1 1 0");
        assert_eq!(
            parser.process_line("%error 1 1 0"),
            ParseResult::Error(1, "tmux command failed".to_string())
        );
    }

    #[test]
    fn parser_notifications_ignored_during_response() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        parser.process_line("%begin 1 1 0");
        parser.process_line("output line");
        assert_eq!(
            parser.process_line("%output %42 some notification"),
            ParseResult::Continue
        );
        parser.process_line("more output");

        assert_eq!(
            parser.process_line("%end 1 1 0"),
            ParseResult::Output(1, "output line\nmore output".to_string())
        );
    }

    #[test]
    fn parser_pane_ids_included_in_output() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        parser.process_line("%begin 1 1 0");
        parser.process_line("%0\t/home/user");
        parser.process_line("%1\t/home/user/project");

        assert_eq!(
            parser.process_line("%end 1 1 0"),
            ParseResult::Output(1, "%0\t/home/user\n%1\t/home/user/project".to_string())
        );
    }

    #[test]
    fn parser_output_trimmed() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        parser.process_line("%begin 1 1 0");
        parser.process_line("  output with whitespace  ");

        assert_eq!(
            parser.process_line("%end 1 1 0"),
            ParseResult::Output(1, "output with whitespace".to_string())
        );
    }

    #[test]
    fn parser_multiple_commands_sequential() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        parser.process_line("%begin 1 1 0");
        parser.process_line("first output");
        assert_eq!(
            parser.process_line("%end 1 1 0"),
            ParseResult::Output(1, "first output".to_string())
        );

        parser.process_line("%begin 2 2 0");
        parser.process_line("second output");
        assert_eq!(
            parser.process_line("%end 2 2 0"),
            ParseResult::Output(2, "second output".to_string())
        );
    }

    #[test]
    fn parser_content_outside_block_ignored() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        assert_eq!(parser.process_line("stray content"), ParseResult::Continue);

        parser.process_line("%begin 1 1 0");
        parser.process_line("actual output");

        assert_eq!(
            parser.process_line("%end 1 1 0"),
            ParseResult::Output(1, "actual output".to_string())
        );
    }

    #[test]
    fn parser_end_without_begin_ignored() {
        let mut parser = ProtocolParser::new();

        parser.process_line("%begin 0 0 0");
        parser.process_line("%end 0 0 0");

        assert_eq!(parser.process_line("%end 999 999 0"), ParseResult::Continue);
    }

    #[test]
    fn parse_command_num_extracts_second_number() {
        assert_eq!(parse_command_num("%begin 1234567890 42 0"), Some(42));
        assert_eq!(parse_command_num("%end 1234567890 1 0"), Some(1));
        assert_eq!(parse_command_num("%error 0 99 0"), Some(99));
    }

    #[test]
    fn parse_command_num_handles_missing() {
        assert_eq!(parse_command_num("%begin"), None);
        assert_eq!(parse_command_num("%begin 123"), None);
    }
}
