use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

use wagner::config::Config;
use wagner::core::WagnerCore;
use wagner::model::Engine;
use wagner::store::Store;
use wagner::terminal::MockTerminal;
use wagner::transport::{CoreCommand, CoreResponse};
use wagner::{PaneHandle, RepoSource, RepoSpec, Terminal, TestAgent, Wagner};

struct E2eContext {
    _temp_dir: TempDir,
    tasks_root: PathBuf,
    repo_path: PathBuf,
}

impl E2eContext {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let tasks_root = temp_dir.path().join("tasks");
        let repo_path = temp_dir.path().join("test-repo");

        std::fs::create_dir_all(&tasks_root).unwrap();
        std::fs::create_dir_all(&repo_path).unwrap();

        Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .expect("Failed to init git repo");

        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        std::fs::write(repo_path.join("README.md"), "# Test Repo").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        Self {
            _temp_dir: temp_dir,
            tasks_root,
            repo_path,
        }
    }

    fn config(&self) -> Config {
        Config {
            tasks_root: self.tasks_root.clone(),
            ..Config::default()
        }
    }

    fn terminal(&self) -> MockTerminal {
        MockTerminal::new()
    }

    fn add_second_repo(&self) -> PathBuf {
        let repo2_path = self._temp_dir.path().join("test-repo-2");
        std::fs::create_dir_all(&repo2_path).unwrap();

        Command::new("git")
            .args(["init"])
            .current_dir(&repo2_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo2_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&repo2_path)
            .output()
            .unwrap();

        std::fs::write(repo2_path.join("README.md"), "# Test Repo 2").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(&repo2_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&repo2_path)
            .output()
            .unwrap();

        repo2_path
    }

    fn create_task_with_panes(&self, terminal: &MockTerminal, task_name: &str) -> wagner::Task {
        let wagner = Wagner::new(terminal.clone(), TestAgent::echo(), self.config());
        let spec = RepoSpec {
            name: "main".to_string(),
            source: RepoSource::Local(self.repo_path.clone()),
            branch: format!("feature/{task_name}"),
        };
        wagner.create_task(task_name, &[spec], None).unwrap();
        wagner.get_task(task_name).unwrap()
    }

    fn execute_cmd(&self, terminal: &MockTerminal, cmd: &CoreCommand) -> CoreResponse {
        let config = self.config();
        let store = Store::new(config.clone());
        let core = WagnerCore::new(config);
        let tasks = store.list_tasks().unwrap_or_default();
        core.execute(terminal, &store, cmd, &tasks)
    }
}

// ─── 1. Add Pane Contract ────────────────────────────────────────────────────

#[test]
fn add_pane_default_agent() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "add-default");

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "add-default".into(),
            pane_name: None,
            agent: None,
            repo_name: None,
        },
    );

    match resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("Claude"),
                "Expected 'Claude' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    let keys = terminal.get_sent_keys();
    let launch = keys.iter().find(|(_, k)| k.contains("claude"));
    assert!(launch.is_some(), "Should have sent claude launch command");
    let (_, cmd) = launch.unwrap();
    assert!(
        cmd.contains("--session-id"),
        "Launch should contain --session-id: {cmd}"
    );
}

#[test]
fn add_pane_codex() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "add-codex");

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "add-codex".into(),
            pane_name: None,
            agent: Some("codex".into()),
            repo_name: None,
        },
    );

    match resp {
        CoreResponse::Confirmation { message } => {
            assert!(message.contains("Codex"), "Expected 'Codex' in: {message}");
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    let keys = terminal.get_sent_keys();
    let launch = keys.iter().find(|(_, k)| k == "codex");
    assert!(launch.is_some(), "Should have sent 'codex' launch command");
}

#[test]
fn add_pane_terminal() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "add-term");

    let keys_before = terminal.get_sent_keys().len();

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "add-term".into(),
            pane_name: None,
            agent: Some("terminal".into()),
            repo_name: None,
        },
    );

    match resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.to_lowercase().contains("terminal"),
                "Expected 'terminal' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    // Terminal engine should NOT send any launch command (send_text_enter)
    // The keys sent after AddPane should only be session/pane management, not agent launch.
    // Specifically, no send_literal (text) + "Enter" pair for launch.
    let keys_after = terminal.get_sent_keys();
    let new_keys: Vec<_> = keys_after[keys_before..].to_vec();
    let has_launch = new_keys
        .iter()
        .any(|(_, k)| k == "codex" || k.contains("claude"));
    assert!(
        !has_launch,
        "Terminal pane should not launch an agent: {new_keys:?}"
    );
}

#[test]
fn add_pane_invalid_agent() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "add-invalid");

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "add-invalid".into(),
            pane_name: None,
            agent: Some("gpt4".into()),
            repo_name: None,
        },
    );

    match resp {
        CoreResponse::Error { message } => {
            assert!(
                message.contains("Unknown agent type"),
                "Expected 'Unknown agent type' in: {message}"
            );
        }
        other => panic!("Expected Error, got: {other:?}"),
    }
}

#[test]
fn add_pane_with_repo_name() {
    let ctx = E2eContext::new();
    let repo2_path = ctx.add_second_repo();
    let terminal = ctx.terminal();

    let wagner = Wagner::new(terminal.clone(), TestAgent::echo(), ctx.config());
    let specs = vec![
        RepoSpec {
            name: "frontend".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/multi-repo".to_string(),
        },
        RepoSpec {
            name: "backend".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/multi-repo".to_string(),
        },
    ];
    wagner.create_task("multi-repo", &specs, None).unwrap();

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "multi-repo".into(),
            pane_name: None,
            agent: None,
            repo_name: Some("backend".into()),
        },
    );

    match resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("Claude"),
                "Expected 'Claude' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }
}

#[test]
fn add_pane_invalid_repo() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "invalid-repo");

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "invalid-repo".into(),
            pane_name: None,
            agent: None,
            repo_name: Some("nonexistent".into()),
        },
    );

    match resp {
        CoreResponse::Error { message } => {
            assert!(
                message.contains("not found in task"),
                "Expected 'not found in task' in: {message}"
            );
        }
        other => panic!("Expected Error, got: {other:?}"),
    }
}

#[test]
fn add_pane_auto_name() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "auto-name");

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "auto-name".into(),
            pane_name: None,
            agent: None,
            repo_name: None,
        },
    );

    match resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("claude-main"),
                "Expected auto-generated name 'claude-main' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }
}

#[test]
fn add_pane_custom_name() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "custom-name");

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "custom-name".into(),
            pane_name: Some("my-pane".into()),
            agent: None,
            repo_name: None,
        },
    );

    match resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("my-pane"),
                "Expected 'my-pane' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }
}

#[test]
fn add_pane_name_dedup() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "dedup");

    // First add with custom name
    let resp1 = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "dedup".into(),
            pane_name: Some("api".into()),
            agent: None,
            repo_name: None,
        },
    );
    match &resp1 {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("api"),
                "First pane should be 'api': {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    // Second add with same name → should get "-2" suffix
    let resp2 = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "dedup".into(),
            pane_name: Some("api".into()),
            agent: None,
            repo_name: None,
        },
    );
    match resp2 {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("api-2"),
                "Second pane should be 'api-2': {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }
}

#[test]
fn add_pane_task_not_found() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "nonexistent".into(),
            pane_name: None,
            agent: None,
            repo_name: None,
        },
    );

    match resp {
        CoreResponse::Error { message } => {
            assert!(
                message.contains("not found"),
                "Expected 'not found' in: {message}"
            );
        }
        other => panic!("Expected Error, got: {other:?}"),
    }
}

// ─── 2. Keybinding Rename ───────────────────────────────────────────────────

#[test]
fn config_next_tab_default() {
    let config = Config::default();
    assert_eq!(config.keybindings.next_tab, "Tab");
}

#[test]
fn config_toggle_sidebar_alias() {
    let json = r#"{"toggle_sidebar": "F1"}"#;
    let kb: wagner::config::Keybindings = serde_json::from_str(json).unwrap();
    assert_eq!(kb.next_tab, "F1");
}

// ─── 3. Send Path ───────────────────────────────────────────────────────────

#[test]
fn send_message_key_sequence() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "send-msg");

    let keys_before = terminal.get_sent_keys().len();

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::SendMessage {
            task_name: "send-msg".into(),
            pane_name: None,
            message: "hello world".into(),
        },
    );

    match &resp {
        CoreResponse::Confirmation { .. } => {}
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    let keys = terminal.get_sent_keys();
    let new_keys: Vec<_> = keys[keys_before..].to_vec();
    // send_text_enter sends: send_literal(text) then send_key("Enter")
    assert!(
        new_keys.iter().any(|(_, k)| k == "hello world"),
        "Should send literal text: {new_keys:?}"
    );
    assert!(
        new_keys.iter().any(|(_, k)| k == "Enter"),
        "Should send Enter: {new_keys:?}"
    );
}

#[test]
fn approve_no_waiting_pane_errors() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "approve-nw");

    let keys_before = terminal.get_sent_keys().len();

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::Approve {
            task_name: "approve-nw".into(),
            pane_name: None,
        },
    );

    match &resp {
        CoreResponse::Error { message } => {
            assert!(
                message.contains("No waiting pane"),
                "Expected 'No waiting pane' in: {message}"
            );
        }
        other => panic!("Expected Error when no pane is waiting, got: {other:?}"),
    }

    // No keys should have been sent to the terminal
    let keys = terminal.get_sent_keys();
    let new_keys: Vec<_> = keys[keys_before..].to_vec();
    assert!(
        new_keys.is_empty(),
        "Should NOT send any keys to non-waiting pane: {new_keys:?}"
    );
}

#[test]
fn reject_no_waiting_pane_errors() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "reject-nw");

    let keys_before = terminal.get_sent_keys().len();

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::Reject {
            task_name: "reject-nw".into(),
            pane_name: None,
        },
    );

    match &resp {
        CoreResponse::Error { message } => {
            assert!(
                message.contains("No waiting pane"),
                "Expected 'No waiting pane' in: {message}"
            );
        }
        other => panic!("Expected Error when no pane is waiting, got: {other:?}"),
    }

    let keys = terminal.get_sent_keys();
    let new_keys: Vec<_> = keys[keys_before..].to_vec();
    assert!(
        new_keys.is_empty(),
        "Should NOT send any keys to non-waiting pane: {new_keys:?}"
    );
}

// ─── 4. Resume Command ─────────────────────────────────────────────────────

#[test]
fn resume_sends_command() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    let task = ctx.create_task_with_panes(&terminal, "resume");

    let pane = &task.panes[0];
    let session_id = &pane.session_id;

    let keys_before = terminal.get_sent_keys().len();

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::Resume {
            task_name: "resume".into(),
            pane_name: None,
        },
    );

    match &resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("Resuming"),
                "Expected 'Resuming' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    let keys = terminal.get_sent_keys();
    let new_keys: Vec<_> = keys[keys_before..].to_vec();

    // Should send C-c, C-u before the resume command
    assert!(
        new_keys.iter().any(|(_, k)| k == "C-c"),
        "Should send C-c: {new_keys:?}"
    );
    assert!(
        new_keys.iter().any(|(_, k)| k == "C-u"),
        "Should send C-u: {new_keys:?}"
    );

    let expected_resume = format!("claude --resume {session_id}");
    assert!(
        new_keys.iter().any(|(_, k)| k == &expected_resume),
        "Should send resume command '{expected_resume}': {new_keys:?}"
    );
    assert!(
        new_keys.iter().any(|(_, k)| k == "Enter"),
        "Should send Enter: {new_keys:?}"
    );
}

#[test]
fn resume_already_running_errors() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    let task = ctx.create_task_with_panes(&terminal, "resume-running");

    // Set mock to report "claude" as the running process
    let pane_id = &task.panes[0].pane_id;
    terminal.set_pane_command(pane_id, "claude");

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::Resume {
            task_name: "resume-running".into(),
            pane_name: None,
        },
    );

    match resp {
        CoreResponse::Error { message } => {
            assert!(
                message.to_lowercase().contains("already running"),
                "Expected 'already running' in: {message}"
            );
        }
        other => panic!("Expected Error, got: {other:?}"),
    }
}

// ─── 5. Engine Delay Values ─────────────────────────────────────────────────

#[test]
fn engine_enter_delays() {
    assert_eq!(Engine::ClaudeCode.enter_delay_ms(), 5);
    assert_eq!(Engine::Codex.enter_delay_ms(), 100);
    assert_eq!(Engine::Terminal.enter_delay_ms(), 10);
}

// ─── 6. Pane Count Reflects Live State ──────────────────────────────────────

#[test]
fn pane_count_matches_live_tmux() {
    let ctx = E2eContext::new();
    let terminal = ctx.terminal();
    ctx.create_task_with_panes(&terminal, "pane-count");

    // Kill the tmux pane so tracked count (1) diverges from live count (0)
    let task = {
        let store = Store::new(ctx.config());
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.name == "pane-count")
            .unwrap()
    };
    let pane_id = &task.panes[0].pane_id;
    terminal
        .kill_pane(&PaneHandle(pane_id.clone(), String::new()))
        .unwrap();

    // ListTasks should report 0 panes (live), not 1 (tracked)
    let resp = ctx.execute_cmd(&terminal, &CoreCommand::ListTasks);
    match resp {
        CoreResponse::TaskList { tasks } => {
            let summary = tasks.iter().find(|(_s, _)| _s.name == "pane-count");
            assert!(summary.is_some(), "Task should appear in list");
            let (s, _) = summary.unwrap();
            assert_eq!(
                s.pane_count, 0,
                "pane_count should reflect live tmux state, not stale tracking"
            );
        }
        other => panic!("Expected TaskList, got: {other:?}"),
    }
}
