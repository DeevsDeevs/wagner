use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{BufRead, BufReader, Read, Write};
use std::thread;
use std::time::Duration;

const CONTROL_SESSION: &str = "wagner_ctrl_test";

fn main() {
    println!("=== tmux Control Mode Test ===\n");

    // Kill any existing test session
    println!("1. Cleaning up existing session...");
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", CONTROL_SESSION])
        .output();
    println!("   Done.\n");

    // Open PTY
    println!("2. Opening PTY...");
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("Failed to open pty");
    println!("   Done.\n");

    // Spawn tmux -CC
    println!("3. Spawning tmux -CC...");
    let mut cmd = CommandBuilder::new("tmux");
    cmd.args(["-CC", "new-session", "-s", CONTROL_SESSION]);

    let _child = pair
        .slave
        .spawn_command(cmd)
        .expect("Failed to spawn tmux -CC");
    println!("   Done.\n");

    // Get reader/writer
    let reader = pair
        .master
        .try_clone_reader()
        .expect("Failed to get reader");
    let mut writer = pair.master.take_writer().expect("Failed to get writer");

    // Spawn reader thread that prints raw output
    println!("4. Starting reader thread...\n");
    let reader_handle = thread::spawn(move || {
        raw_reader_loop(reader);
    });

    // Wait for startup
    println!("5. Waiting for startup (2s)...\n");
    thread::sleep(Duration::from_secs(2));

    // Send test commands
    println!("6. Sending test commands...\n");

    // Test 1: list-sessions (simple, no -t)
    println!("--- Command: list-sessions ---");
    writeln!(writer, "list-sessions").expect("write failed");
    writer.flush().expect("flush failed");
    thread::sleep(Duration::from_secs(1));

    // Test 2: list-panes with -t (may have different behavior)
    println!("\n--- Command: list-panes -s -t wagner_wagner ---");
    writeln!(writer, "list-panes -s -t wagner_wagner -F \"#{{pane_id}}\\t#{{pane_current_path}}\"")
        .expect("write failed");
    writer.flush().expect("flush failed");
    thread::sleep(Duration::from_secs(1));

    // Test 3: Simple display-message
    println!("\n--- Command: display-message -p 'hello world' ---");
    writeln!(writer, "display-message -p \"hello world\"").expect("write failed");
    writer.flush().expect("flush failed");
    thread::sleep(Duration::from_secs(1));

    println!("\n7. Done. Killing session...");
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", CONTROL_SESSION])
        .output();

    drop(writer);
    let _ = reader_handle.join();

    println!("\n=== Test Complete ===");
}

fn raw_reader_loop(mut reader: Box<dyn Read + Send>) {
    println!("   [Reader] Started\n");

    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                println!("   [Reader] EOF");
                break;
            }
            Ok(n) => {
                let data = &buf[..n];
                // Print as string, escaping non-printable
                let s = String::from_utf8_lossy(data);
                for line in s.lines() {
                    if line.starts_with("%begin") {
                        println!("   [Reader] >>> BEGIN: {}", line);
                    } else if line.starts_with("%end") {
                        println!("   [Reader] <<< END: {}", line);
                    } else if line.starts_with("%error") {
                        println!("   [Reader] !!! ERROR: {}", line);
                    } else if line.starts_with("%") {
                        println!("   [Reader] %%% NOTIFICATION: {}", line);
                    } else if !line.is_empty() {
                        println!("   [Reader] OUTPUT: {}", line);
                    }
                }
            }
            Err(e) => {
                println!("   [Reader] Error: {}", e);
                break;
            }
        }
    }
}
