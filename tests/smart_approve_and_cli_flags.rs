use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

use wagner::config::Config;
use wagner::core::WagnerCore;
use wagner::model::Engine;
use wagner::monitor::status::{AgentStatus, AgentType, PaneStatus, WaitReason};
use wagner::store::Store;
use wagner::terminal::MockTerminal;
use wagner::transport::{CoreCommand, CoreResponse};
use wagner::{RepoSource, RepoSpec, TestAgent, Wagner};

struct TestCtx {
    _temp_dir: TempDir,
    tasks_root: PathBuf,
    repo_path: PathBuf,
}

impl TestCtx {
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

    fn create_task(&self, terminal: &MockTerminal, task_name: &str) -> wagner::Task {
        let wagner = Wagner::new(terminal.clone(), TestAgent::echo(), self.config());
        let spec = RepoSpec {
            name: "main".to_string(),
            source: RepoSource::Local(self.repo_path.clone()),
            branch: format!("feature/{task_name}"),
        };
        wagner.create_task(task_name, &[spec], None).unwrap();
        wagner.get_task(task_name).unwrap()
    }
}

fn waiting_status() -> PaneStatus {
    PaneStatus::Agent {
        agent_type: AgentType::ClaudeCode,
        status: AgentStatus::Waiting(WaitReason::Approval),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Fix 1: smart_approve with multiple waiting panes
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_smart_approve_single_waiting_pane() {
    let ctx = TestCtx::new();
    let terminal = ctx.terminal();
    let task = ctx.create_task(&terminal, "approve-single");

    let config = ctx.config();
    let store = Store::new(config.clone());
    let mut core = WagnerCore::new(config);

    let pane_id = &task.panes[0].pane_id;

    // Inject waiting status for the pane
    core.status_engine
        .inject_pane_status(pane_id, waiting_status());

    let tasks = store.list_tasks().unwrap_or_default();
    let resp = core.execute(
        &terminal,
        &store,
        &CoreCommand::Approve {
            task_name: String::new(),
            pane_name: None,
        },
        &tasks,
    );

    match resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("Approved"),
                "Expected 'Approved' in: {message}"
            );
            assert!(
                message.contains("1 pane(s)"),
                "Expected '1 pane(s)' in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    // Verify that send_approve was actually called (Enter key)
    let keys = terminal.get_sent_keys();
    let sent_approve = keys.iter().any(|(_, k)| k == "Enter");
    assert!(
        sent_approve,
        "Should have sent Enter (approve) to the pane: {keys:?}"
    );
}

#[test]
fn test_smart_approve_multiple_waiting_panes() {
    let ctx = TestCtx::new();
    let terminal = ctx.terminal();
    let _task = ctx.create_task(&terminal, "approve-multi");

    // Add a second pane to the same task
    let config = ctx.config();
    let store = Store::new(config.clone());
    let wagner_inst = Wagner::new(terminal.clone(), TestAgent::echo(), ctx.config());
    let mut task_clone = wagner_inst.get_task("approve-multi").unwrap();
    let repo = task_clone.repos[0].clone();
    wagner::wagner::add_pane_shared(
        &terminal,
        &store,
        &mut task_clone,
        &repo,
        Engine::ClaudeCode,
        Some("second-pane"),
    )
    .unwrap();

    // Reload task to get updated panes
    let task = store.load_task("approve-multi").unwrap();
    assert!(
        task.panes.len() >= 2,
        "Should have at least 2 panes, got: {}",
        task.panes.len()
    );

    let mut core = WagnerCore::new(config.clone());

    // Inject waiting status for both panes
    for tp in &task.panes {
        core.status_engine
            .inject_pane_status(&tp.pane_id, waiting_status());
    }

    let tasks = store.list_tasks().unwrap_or_default();
    let keys_before = terminal.get_sent_keys().len();

    let resp = core.execute(
        &terminal,
        &store,
        &CoreCommand::Approve {
            task_name: String::new(),
            pane_name: None,
        },
        &tasks,
    );

    match resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("Approved"),
                "Expected 'Approved' in: {message}"
            );
            assert!(
                message.contains("2 pane(s)"),
                "Expected '2 pane(s)' in: {message}"
            );
            // Should list each approved pane
            assert!(
                message.contains("approve-multi"),
                "Expected task name in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    // Verify that send_approve was called for BOTH panes (two Enter keys)
    let keys = terminal.get_sent_keys();
    let new_keys: Vec<_> = keys[keys_before..].to_vec();
    let enter_count = new_keys.iter().filter(|(_, k)| k == "Enter").count();
    assert_eq!(
        enter_count, 2,
        "Should have sent Enter twice (once per pane), got: {enter_count}"
    );
}

#[test]
fn test_smart_approve_no_waiting_panes() {
    let ctx = TestCtx::new();
    let terminal = ctx.terminal();
    let _task = ctx.create_task(&terminal, "approve-none");

    let config = ctx.config();
    let store = Store::new(config.clone());
    let core = WagnerCore::new(config);

    // Don't inject any waiting status — all panes are Unknown
    let tasks = store.list_tasks().unwrap_or_default();
    let resp = core.execute(
        &terminal,
        &store,
        &CoreCommand::Approve {
            task_name: String::new(),
            pane_name: None,
        },
        &tasks,
    );

    match resp {
        CoreResponse::Error { message } => {
            assert!(
                message.contains("No panes are waiting"),
                "Expected 'No panes are waiting' in: {message}"
            );
        }
        other => panic!("Expected Error, got: {other:?}"),
    }
}

#[test]
fn test_smart_approve_across_multiple_tasks() {
    let ctx = TestCtx::new();
    let terminal = ctx.terminal();
    let task1 = ctx.create_task(&terminal, "multi-task-1");
    let task2 = ctx.create_task(&terminal, "multi-task-2");

    let config = ctx.config();
    let store = Store::new(config.clone());
    let mut core = WagnerCore::new(config);

    // Inject waiting status for panes in both tasks
    core.status_engine
        .inject_pane_status(&task1.panes[0].pane_id, waiting_status());
    core.status_engine
        .inject_pane_status(&task2.panes[0].pane_id, waiting_status());

    let tasks = store.list_tasks().unwrap_or_default();
    let keys_before = terminal.get_sent_keys().len();

    let resp = core.execute(
        &terminal,
        &store,
        &CoreCommand::Approve {
            task_name: String::new(),
            pane_name: None,
        },
        &tasks,
    );

    match resp {
        CoreResponse::Confirmation { message } => {
            assert!(
                message.contains("2 pane(s)"),
                "Expected '2 pane(s)' in: {message}"
            );
            assert!(
                message.contains("multi-task-1"),
                "Expected task-1 name in: {message}"
            );
            assert!(
                message.contains("multi-task-2"),
                "Expected task-2 name in: {message}"
            );
        }
        other => panic!("Expected Confirmation, got: {other:?}"),
    }

    let keys = terminal.get_sent_keys();
    let new_keys: Vec<_> = keys[keys_before..].to_vec();
    let enter_count = new_keys.iter().filter(|(_, k)| k == "Enter").count();
    assert_eq!(
        enter_count, 2,
        "Should have sent Enter twice (once per task pane)"
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Fix 2: Repair --dry-run flag removed (tested via binary CLI)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Test that `wagner repair --help` does NOT mention --dry-run and DOES mention --execute.
#[test]
fn test_repair_help_shows_execute_not_dry_run() {
    let binary = env!("CARGO_BIN_EXE_wagner");
    let output = Command::new(binary)
        .args(["repair", "--help"])
        .output()
        .expect("Failed to run wagner repair --help");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--execute"),
        "Repair help should mention --execute, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("--dry-run"),
        "Repair help should NOT mention --dry-run, got:\n{stdout}"
    );
}

/// Test that `wagner repair --dry-run` is rejected by the CLI.
#[test]
fn test_repair_rejects_dry_run_flag() {
    let binary = env!("CARGO_BIN_EXE_wagner");
    let output = Command::new(binary)
        .args(["repair", "--dry-run"])
        .output()
        .expect("Failed to run wagner repair --dry-run");

    assert!(
        !output.status.success(),
        "wagner repair --dry-run should fail (flag removed)"
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Fix 3: delete_task separate flags for confirmation and branch deletion
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Test that `wagner delete --help` shows separate --yes and --delete-branches flags.
#[test]
fn test_delete_help_shows_separate_flags() {
    let binary = env!("CARGO_BIN_EXE_wagner");
    let output = Command::new(binary)
        .args(["delete", "--help"])
        .output()
        .expect("Failed to run wagner delete --help");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--yes"),
        "Delete help should mention --yes flag, got:\n{stdout}"
    );
    assert!(
        stdout.contains("--delete-branches"),
        "Delete help should mention --delete-branches flag, got:\n{stdout}"
    );
    assert!(
        stdout.contains("-y"),
        "Delete help should show -y shorthand, got:\n{stdout}"
    );
}

/// Test that delete_task(name, false) does not delete branches.
#[test]
fn test_delete_without_branches_doesnt_delete_branches() {
    let ctx = TestCtx::new();
    let terminal = ctx.terminal();

    let wagner_inst = Wagner::new(terminal.clone(), TestAgent::echo(), ctx.config());
    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/no-branch-delete".to_string(),
    };
    wagner_inst
        .create_task("no-branch-del", &[spec], None)
        .unwrap();

    // Verify branch was created
    let branch_exists = Command::new("git")
        .args(["branch", "--list", "feature/no-branch-delete"])
        .current_dir(&ctx.repo_path)
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    assert!(branch_exists, "Branch should exist before delete");

    // Delete with delete_branches=false
    wagner_inst.delete_task("no-branch-del", false).unwrap();

    // Branch should still exist
    let branch_exists_after = Command::new("git")
        .args(["branch", "--list", "feature/no-branch-delete"])
        .current_dir(&ctx.repo_path)
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    assert!(
        branch_exists_after,
        "Branch should still exist when delete_branches=false"
    );
}

/// Test that delete_task(name, true) does delete branches.
#[test]
fn test_delete_with_branches_deletes_branches() {
    let ctx = TestCtx::new();
    let terminal = ctx.terminal();

    let wagner_inst = Wagner::new(terminal.clone(), TestAgent::echo(), ctx.config());
    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/branch-delete".to_string(),
    };
    wagner_inst
        .create_task("branch-del", &[spec], None)
        .unwrap();

    // Delete with delete_branches=true
    wagner_inst.delete_task("branch-del", true).unwrap();

    // Branch should be gone
    let branch_exists_after = Command::new("git")
        .args(["branch", "--list", "feature/branch-delete"])
        .current_dir(&ctx.repo_path)
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    assert!(
        !branch_exists_after,
        "Branch should be deleted when delete_branches=true"
    );
}
