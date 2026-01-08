use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;
use wagner::config::Workspace;
use wagner::{Config, MockTerminal, PaneHandle, RepoSource, RepoSpec, Terminal, TestAgent, Wagner};

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

    fn wagner(&self) -> Wagner<MockTerminal, TestAgent> {
        Wagner::new(MockTerminal::new(), TestAgent::echo(), self.config())
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
}

#[test]
fn test_create_single_repo_task() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/test".to_string(),
    };

    let task = wagner.create_task("test-task", &[spec], None).unwrap();

    assert_eq!(task.name, "test-task");
    assert_eq!(task.repos.len(), 1);
    assert_eq!(task.repos[0].name, "main");
    assert_eq!(task.repos[0].branch, "feature/test");

    let worktree_path = ctx.tasks_root.join("test-task").join("main");
    assert!(worktree_path.exists(), "Worktree should exist");

    let readme = worktree_path.join("README.md");
    assert!(readme.exists(), "README should exist in worktree");

    let terminal = &wagner.terminal;
    assert!(terminal.session_exists("test-task").unwrap());

    let sent_keys = terminal.get_sent_keys();
    assert!(
        !sent_keys.is_empty(),
        "Should have sent keys to launch agent"
    );
    assert!(
        sent_keys[0].1.contains("echo"),
        "Should have launched test agent"
    );
}

#[test]
fn test_create_multi_repo_task() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();
    let wagner = ctx.wagner();

    let specs = vec![
        RepoSpec {
            name: "repo1".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/multi".to_string(),
        },
        RepoSpec {
            name: "repo2".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/multi".to_string(),
        },
    ];

    let task = wagner
        .create_task("multi-task", &specs, Some("main"))
        .unwrap();

    assert_eq!(task.name, "multi-task");
    assert_eq!(task.repos.len(), 2);
    assert_eq!(task.diff_base, Some("main".to_string()));

    let worktree1 = ctx.tasks_root.join("multi-task").join("repo1");
    let worktree2 = ctx.tasks_root.join("multi-task").join("repo2");
    assert!(worktree1.exists(), "Worktree 1 should exist");
    assert!(worktree2.exists(), "Worktree 2 should exist");

    let terminal = &wagner.terminal;
    let sent_keys = terminal.get_sent_keys();
    assert!(
        sent_keys.len() >= 3,
        "Should have sent keys to central pane + 2 repo panes"
    );
}

#[test]
fn test_add_repo_to_task() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/test".to_string(),
    };

    wagner.create_task("add-repo-task", &[spec], None).unwrap();

    let new_spec = RepoSpec {
        name: "second".to_string(),
        source: RepoSource::Local(repo2_path),
        branch: "feature/test".to_string(),
    };

    wagner.add_repo_to_task("add-repo-task", &new_spec).unwrap();

    let task = wagner.get_task("add-repo-task").unwrap();
    assert_eq!(task.repos.len(), 2);
    assert!(task.repos.iter().any(|r| r.name == "second"));

    let worktree2 = ctx.tasks_root.join("add-repo-task").join("second");
    assert!(worktree2.exists(), "New worktree should exist");
}

#[test]
fn test_remove_repo_from_task() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();
    let wagner = ctx.wagner();

    let specs = vec![
        RepoSpec {
            name: "repo1".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/rm".to_string(),
        },
        RepoSpec {
            name: "repo2".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/rm".to_string(),
        },
    ];

    wagner.create_task("rm-repo-task", &specs, None).unwrap();

    let worktree2 = ctx.tasks_root.join("rm-repo-task").join("repo2");
    assert!(worktree2.exists(), "Worktree 2 should exist before removal");

    wagner
        .remove_repo_from_task("rm-repo-task", "repo2")
        .unwrap();

    let task = wagner.get_task("rm-repo-task").unwrap();
    assert_eq!(task.repos.len(), 1);
    assert!(!task.repos.iter().any(|r| r.name == "repo2"));

    assert!(!worktree2.exists(), "Worktree 2 should be removed");
}

#[test]
fn test_delete_task_cleans_up() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/delete".to_string(),
    };

    wagner.create_task("delete-task", &[spec], None).unwrap();

    let task_path = ctx.tasks_root.join("delete-task");
    let worktree_path = task_path.join("main");
    assert!(
        worktree_path.exists(),
        "Worktree should exist before delete"
    );

    wagner.delete_task("delete-task", false).unwrap();

    assert!(
        !worktree_path.exists(),
        "Worktree should be removed after delete"
    );

    let tasks = wagner.list_tasks().unwrap();
    assert!(
        !tasks.iter().any(|t| t.name == "delete-task"),
        "Task should not be in list"
    );

    assert!(
        !wagner.terminal.session_exists("delete-task").unwrap(),
        "Session should be killed"
    );
}

#[test]
fn test_list_tasks() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/list1".to_string(),
    };

    wagner.create_task("list-task-1", &[spec], None).unwrap();

    let spec2 = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/list2".to_string(),
    };

    wagner.create_task("list-task-2", &[spec2], None).unwrap();

    let tasks = wagner.list_tasks().unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().any(|t| t.name == "list-task-1"));
    assert!(tasks.iter().any(|t| t.name == "list-task-2"));
}

#[test]
fn test_task_already_exists_error() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/dup".to_string(),
    };

    wagner
        .create_task("dup-task", &[spec.clone()], None)
        .unwrap();

    let result = wagner.create_task("dup-task", &[spec], None);
    assert!(result.is_err(), "Should error on duplicate task name");
}

#[test]
fn test_add_pane_defaults() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "single".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/pane1".to_string(),
    };
    wagner
        .create_task("single-pane-task", &[spec], None)
        .unwrap();

    wagner.add_pane("single-pane-task", None).unwrap();

    let specs = vec![
        RepoSpec {
            name: "repo1".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/pane2".to_string(),
        },
        RepoSpec {
            name: "repo2".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/pane2".to_string(),
        },
    ];
    wagner.create_task("multi-pane-task", &specs, None).unwrap();

    wagner.add_pane("multi-pane-task", None).unwrap();
    wagner.add_pane("multi-pane-task", Some("repo1")).unwrap();
}

// =====================
// WORKSPACE CONFIG TESTS
// =====================

#[test]
fn test_create_task_from_workspace() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();

    let mut config = ctx.config();
    let mut ws = Workspace::default();
    ws.base_branch = "main".to_string();
    ws.repos.insert(
        "repo1".to_string(),
        ctx.repo_path.to_string_lossy().to_string(),
    );
    ws.repos.insert(
        "repo2".to_string(),
        repo2_path.to_string_lossy().to_string(),
    );
    config.workspaces.insert("test-ws".to_string(), ws);

    let wagner = Wagner::new(MockTerminal::new(), TestAgent::echo(), config);

    let ws_config = wagner.config.workspaces.get("test-ws").unwrap();
    let specs: Vec<RepoSpec> = ws_config
        .repos
        .iter()
        .map(|(name, path)| RepoSpec {
            name: name.clone(),
            source: RepoSource::Local(PathBuf::from(path)),
            branch: "feature/ws-test".to_string(),
        })
        .collect();

    let task = wagner
        .create_task("ws-task", &specs, Some(&ws_config.base_branch))
        .unwrap();

    assert_eq!(task.repos.len(), 2);
    assert_eq!(task.diff_base, Some("main".to_string()));
}

#[test]
fn test_workspace_default_values() {
    let ws = Workspace::default();
    assert_eq!(ws.base_branch, "main");
    assert!(ws.repos.is_empty());
}

#[test]
fn test_workspace_custom_base_branch() {
    let mut ws = Workspace::default();
    ws.base_branch = "develop".to_string();
    assert_eq!(ws.base_branch, "develop");
}

// =====================
// REPO SPEC PARSING TESTS
// =====================

#[test]
fn test_repo_spec_parse_full() {
    let spec = RepoSpec::parse("myrepo:/path/to/repo:feature/branch", None).unwrap();
    assert_eq!(spec.name, "myrepo");
    assert_eq!(spec.branch, "feature/branch");
    match spec.source {
        RepoSource::Local(path) => assert_eq!(path, PathBuf::from("/path/to/repo")),
        _ => panic!("Expected local source"),
    }
}

#[test]
fn test_repo_spec_parse_without_branch() {
    let spec = RepoSpec::parse("myrepo:/path/to/repo", Some("default-branch")).unwrap();
    assert_eq!(spec.name, "myrepo");
    assert_eq!(spec.branch, "default-branch");
}

#[test]
fn test_repo_spec_parse_without_branch_no_default() {
    let spec = RepoSpec::parse("myrepo:/path/to/repo", None).unwrap();
    assert_eq!(spec.name, "myrepo");
    assert_eq!(spec.branch, "main");
}

#[test]
fn test_repo_spec_parse_invalid() {
    let result = RepoSpec::parse("invalid", None);
    assert!(result.is_err());
}

// =====================
// DELETE FORCE TESTS
// =====================

#[test]
fn test_delete_task_force_deletes_branch() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/force-delete".to_string(),
    };

    wagner
        .create_task("force-delete-task", &[spec], None)
        .unwrap();

    let branch_exists = Command::new("git")
        .args(["branch", "--list", "feature/force-delete"])
        .current_dir(&ctx.repo_path)
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    assert!(branch_exists, "Branch should exist before force delete");

    wagner.delete_task("force-delete-task", true).unwrap();

    let branch_exists_after = Command::new("git")
        .args(["branch", "--list", "feature/force-delete"])
        .current_dir(&ctx.repo_path)
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    assert!(
        !branch_exists_after,
        "Branch should be deleted with --force"
    );
}

// =====================
// CONFIG TESTS
// =====================

#[test]
fn test_config_default_values() {
    let config = Config::default();
    assert_eq!(config.default_agent, "claude");
    assert_eq!(config.diff_base, "main");
    assert_eq!(config.refresh_interval_ms, 100);
    assert!(config.workspaces.is_empty());
}

#[test]
fn test_config_save_and_load() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.json");

    let mut config = Config::default();
    config.diff_base = "develop".to_string();
    config.tasks_root = temp_dir.path().join("tasks");

    let mut ws = Workspace::default();
    ws.repos
        .insert("test".to_string(), "/path/to/test".to_string());
    config.workspaces.insert("my-ws".to_string(), ws);

    let content = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&config_path, &content).unwrap();

    let loaded: Config =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(loaded.diff_base, "develop");
    assert!(loaded.workspaces.contains_key("my-ws"));
    assert!(loaded.workspaces["my-ws"].repos.contains_key("test"));
}

// =====================
// DEFAULT BRANCH TESTS
// =====================

#[test]
fn test_default_branch_for_task() {
    use wagner::default_branch_for_task;

    assert_eq!(default_branch_for_task("my-feature"), "feature/my-feature");
    assert_eq!(default_branch_for_task("fix-bug"), "feature/fix-bug");
}

// =====================
// ERROR HANDLING TESTS
// =====================

#[test]
fn test_repo_not_found_error() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "nonexistent".to_string(),
        source: RepoSource::Local(PathBuf::from("/nonexistent/path")),
        branch: "feature/test".to_string(),
    };

    let result = wagner.create_task("error-task", &[spec], None);
    assert!(result.is_err());
}

#[test]
fn test_add_duplicate_repo_error() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/dup-repo".to_string(),
    };

    wagner
        .create_task("dup-repo-task", &[spec.clone()], None)
        .unwrap();

    let result = wagner.add_repo_to_task("dup-repo-task", &spec);
    assert!(result.is_err(), "Adding duplicate repo should error");
}

#[test]
fn test_remove_nonexistent_repo_error() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/rm-err".to_string(),
    };

    wagner.create_task("rm-err-task", &[spec], None).unwrap();

    let result = wagner.remove_repo_from_task("rm-err-task", "nonexistent");
    assert!(result.is_err(), "Removing nonexistent repo should error");
}

// =====================
// MOCK TERMINAL TESTS
// =====================

#[test]
fn test_mock_terminal_send_key_tracking() {
    let terminal = MockTerminal::new();
    let pane = PaneHandle("%0".to_string(), "test".to_string());

    terminal.send_key(&pane, "Escape").unwrap();
    terminal.send_key(&pane, "Tab").unwrap();
    terminal.send_key(&pane, "C-c").unwrap();

    let sent = terminal.get_sent_keys();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0], ("%0".to_string(), "Escape".to_string()));
    assert_eq!(sent[1], ("%0".to_string(), "Tab".to_string()));
    assert_eq!(sent[2], ("%0".to_string(), "C-c".to_string()));
}

#[test]
fn test_mock_terminal_capture_output() {
    let terminal = MockTerminal::new();
    let pane = PaneHandle("%0".to_string(), "test".to_string());

    assert_eq!(terminal.capture(&pane, 100).unwrap(), "");

    terminal.set_capture_output("%0", "line1\nline2\nline3");
    assert_eq!(terminal.capture(&pane, 100).unwrap(), "line1\nline2\nline3");

    let pane2 = PaneHandle("%1".to_string(), "test2".to_string());
    terminal.set_capture_output("%1", "other content");
    assert_eq!(terminal.capture(&pane2, 100).unwrap(), "other content");
    assert_eq!(terminal.capture(&pane, 100).unwrap(), "line1\nline2\nline3");
}

#[test]
fn test_mock_terminal_send_literal() {
    let terminal = MockTerminal::new();
    let pane = PaneHandle("%0".to_string(), "test".to_string());

    terminal.send_literal(&pane, "hello world").unwrap();

    let sent = terminal.get_sent_keys();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0], ("%0".to_string(), "hello world".to_string()));
}
