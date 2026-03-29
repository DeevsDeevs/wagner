use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

use wagner::config::Config;
use wagner::core::WagnerCore;
use wagner::model::Engine;
use wagner::store::Store;
use wagner::terminal::session_name_for_task;
use wagner::{
    MockTerminal, RepoSource, RepoSpec, SessionHandle, Terminal, TestAgent, Wagner,
};
use wagner::transport::{CoreCommand, CoreResponse};

struct TestContext {
    _temp_dir: TempDir,
    tasks_root: PathBuf,
    repo_path: PathBuf,
}

impl TestContext {
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

    fn create_task_with_panes(
        &self,
        terminal: &MockTerminal,
        task_name: &str,
    ) -> wagner::Task {
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

// VAL-PANE-006: command_executor AddPane delegates to shared logic
// Verifies that AddPane through command_executor produces the same tracked pane
// structure as the direct wagner.rs path — proving delegation to shared code.
#[test]
fn test_command_executor_add_pane_delegates_to_shared_fn() {
    let ctx = TestContext::new();
    let terminal = MockTerminal::new();
    ctx.create_task_with_panes(&terminal, "dedup-delegate");

    // Add a pane via command_executor
    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "dedup-delegate".into(),
            pane_name: Some("test-pane".into()),
            agent: Some("claude".into()),
            repo_name: None,
        },
    );

    // Should succeed
    match &resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("test-pane"),
                "Expected pane name in confirmation: {message}"
            );
            assert!(
                message.contains("Claude"),
                "Expected 'Claude' in confirmation: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    // Verify the task was saved with the tracked pane
    let store = Store::new(ctx.config());
    let task = store.load_task("dedup-delegate").unwrap();

    // Find the pane we just added
    let added_pane = task
        .panes
        .iter()
        .find(|p| p.name == "test-pane")
        .expect("Should find pane 'test-pane' in saved task");

    // Verify core properties match what shared code would produce
    assert_eq!(added_pane.engine, Engine::ClaudeCode);
    assert_eq!(added_pane.repo_name, "main");
    assert!(!added_pane.session_id.is_empty());
    assert!(!added_pane.pane_id.is_empty());

    // Verify launch command was sent
    let keys = terminal.get_sent_keys();
    let has_claude_launch = keys.iter().any(|(_, k)| k.contains("claude") && k.contains("--session-id"));
    assert!(
        has_claude_launch,
        "Should have sent claude launch command via shared code: {keys:?}"
    );
}

// VAL-PANE-007: command_executor AddPane session recreation uses repo worktree
#[test]
fn test_command_executor_add_pane_recreates_session_in_worktree() {
    let ctx = TestContext::new();
    let terminal = MockTerminal::new();
    let _task = ctx.create_task_with_panes(&terminal, "dedup-recreate");

    // Kill the session to simulate dead session
    let session_name = session_name_for_task("dedup-recreate");
    terminal
        .kill_session(&SessionHandle(session_name))
        .unwrap();

    // Add pane via command_executor — should recreate session
    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "dedup-recreate".into(),
            pane_name: None,
            agent: None,
            repo_name: None,
        },
    );

    match &resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("Claude"),
                "Expected 'Claude' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    // Verify session was recreated with repo.worktree, NOT task.path
    let created_sessions = terminal.get_created_sessions();
    assert!(
        created_sessions.len() >= 2,
        "Should have at least 2 session creations (initial + recreation): got {}",
        created_sessions.len()
    );

    let store = Store::new(ctx.config());
    let updated_task = store.load_task("dedup-recreate").unwrap();
    let repo_worktree = &updated_task.repos[0].worktree;

    let last_session = &created_sessions[created_sessions.len() - 1];
    assert_eq!(
        &last_session.1, repo_worktree,
        "Session recreation via command_executor should use repo.worktree. Got {:?}, expected {:?}",
        last_session.1, repo_worktree
    );
    assert_ne!(
        last_session.1, updated_task.path,
        "Session recreation must NOT use task.path"
    );
}

// Verify that AddPane with no inline session creation, pane naming, or agent launch 
// in command_executor — the behavior matches the direct wagner.rs path.
#[test]
fn test_command_executor_add_pane_codex_matches_wagner_path() {
    let ctx = TestContext::new();
    let terminal = MockTerminal::new();
    ctx.create_task_with_panes(&terminal, "codex-match");

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "codex-match".into(),
            pane_name: None,
            agent: Some("codex".into()),
            repo_name: None,
        },
    );

    match &resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("Codex"),
                "Expected 'Codex' in: {message}"
            );
            assert!(
                message.contains("codex-main"),
                "Expected auto-name 'codex-main' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    // Verify tracked pane
    let store = Store::new(ctx.config());
    let task = store.load_task("codex-match").unwrap();
    let codex_pane = task
        .panes
        .iter()
        .find(|p| p.engine == Engine::Codex)
        .expect("Should have a Codex pane");

    assert_eq!(codex_pane.name, "codex-main");
    assert_eq!(codex_pane.repo_name, "main");

    // Codex should have a launch command sent
    let keys = terminal.get_sent_keys();
    let has_codex = keys.iter().any(|(_, k)| k == "codex");
    assert!(has_codex, "Should have sent 'codex' launch command: {keys:?}");
}

// Verify terminal engine pane via command_executor does NOT launch an agent
#[test]
fn test_command_executor_add_pane_terminal_no_launch() {
    let ctx = TestContext::new();
    let terminal = MockTerminal::new();
    ctx.create_task_with_panes(&terminal, "term-nol");

    let keys_before = terminal.get_sent_keys().len();

    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "term-nol".into(),
            pane_name: None,
            agent: Some("terminal".into()),
            repo_name: None,
        },
    );

    match &resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.to_lowercase().contains("terminal"),
                "Expected 'terminal' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    // Terminal engine should NOT send agent launch commands
    let keys = terminal.get_sent_keys();
    let new_keys: Vec<_> = keys[keys_before..].to_vec();
    let has_agent_launch = new_keys
        .iter()
        .any(|(_, k)| k.contains("claude") || k == "codex");
    assert!(
        !has_agent_launch,
        "Terminal pane should not launch an agent: {new_keys:?}"
    );

    // Verify tracked pane has Terminal engine
    let store = Store::new(ctx.config());
    let task = store.load_task("term-nol").unwrap();
    let term_pane = task
        .panes
        .iter()
        .find(|p| p.engine == Engine::Terminal)
        .expect("Should have a Terminal pane");
    assert_eq!(term_pane.repo_name, "main");
}

// Verify multi-repo default repo selection via command_executor uses repos[0]
#[test]
fn test_command_executor_add_pane_multi_repo_default_uses_first_repo() {
    let ctx = TestContext::new();
    let terminal = MockTerminal::new();

    // Create task with 2 repos via direct Wagner
    let repo2_path = ctx._temp_dir.path().join("test-repo-2");
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
    std::fs::write(repo2_path.join("README.md"), "# Repo 2").unwrap();
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

    let wagner = Wagner::new(terminal.clone(), TestAgent::echo(), ctx.config());
    let specs = vec![
        RepoSpec {
            name: "frontend".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/multi-default".to_string(),
        },
        RepoSpec {
            name: "backend".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/multi-default".to_string(),
        },
    ];
    wagner
        .create_task("multi-default", &specs, None)
        .unwrap();

    // Add pane via command_executor with repo_name=None
    let resp = ctx.execute_cmd(
        &terminal,
        &CoreCommand::AddPane {
            task_name: "multi-default".into(),
            pane_name: None,
            agent: None,
            repo_name: None,
        },
    );

    match &resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("Claude"),
                "Expected 'Claude' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    // The newly added pane should use repos[0] (frontend), not core_repo
    let store = Store::new(ctx.config());
    let task = store.load_task("multi-default").unwrap();
    let last_pane = task.panes.last().unwrap();
    assert_eq!(
        last_pane.repo_name, "frontend",
        "Default repo for multi-repo task should be repos[0] ('frontend'), not task name"
    );
}
