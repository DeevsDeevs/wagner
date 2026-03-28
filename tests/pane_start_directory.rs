use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;
use wagner::terminal::session_name_for_task;
use wagner::{
    Config, Engine, MockTerminal, RepoSource, RepoSpec, SessionHandle, Terminal, TestAgent, Wagner,
};

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

// VAL-PANE-001: Session recreation uses repo worktree, not task.path
#[test]
fn test_add_pane_session_recreation_uses_repo_worktree() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/recreate".to_string(),
    };

    wagner
        .create_task("recreate-task", &[spec], None)
        .unwrap();

    // Kill the session to simulate a dead session
    let session_name = session_name_for_task("recreate-task");
    wagner
        .terminal
        .kill_session(&SessionHandle(session_name))
        .unwrap();

    // Now add_pane should recreate the session
    wagner.add_pane("recreate-task", None, None).unwrap();

    let created_sessions = wagner.terminal.get_created_sessions();
    // Second session creation (the recreation) should use repo.worktree
    assert!(
        created_sessions.len() >= 2,
        "Should have at least 2 session creations (initial + recreation)"
    );

    let task = wagner.get_task("recreate-task").unwrap();
    let repo_worktree = &task.repos[0].worktree;
    let last_session = &created_sessions[created_sessions.len() - 1];
    assert_eq!(
        &last_session.1, repo_worktree,
        "Session recreation should use repo.worktree, not task.path. Got {:?}, expected {:?}",
        last_session.1, repo_worktree
    );
    // Verify it's NOT task.path (which would be the task directory, not the worktree)
    assert_ne!(
        last_session.1, task.path,
        "Session recreation must NOT use task.path"
    );
}

// VAL-PANE-002: Multi-repo first pane starts in first repo's worktree
#[test]
fn test_create_session_multi_repo_first_pane_uses_repo_worktree() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();
    let wagner = ctx.wagner();

    let specs = vec![
        RepoSpec {
            name: "repo1".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/multi-first".to_string(),
        },
        RepoSpec {
            name: "repo2".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/multi-first".to_string(),
        },
    ];

    let task = wagner
        .create_task("multi-first-task", &specs, Some("main"))
        .unwrap();

    // The session should be created in repos[0].worktree
    let created_sessions = wagner.terminal.get_created_sessions();
    assert!(!created_sessions.is_empty(), "Should have created a session");

    let session_dir = &created_sessions[0].1;
    let first_repo_worktree = &task.repos[0].worktree;

    assert_eq!(
        session_dir, first_repo_worktree,
        "Multi-repo session should be created in repos[0].worktree, not task.path"
    );
    assert_ne!(
        *session_dir, task.path,
        "Multi-repo session must NOT use task.path (the synthetic task directory)"
    );

    // First pane should be for repo1, not a synthetic core_repo
    assert_eq!(
        task.panes[0].repo_name, "repo1",
        "First pane should be repo1, not a synthetic core repo"
    );
}

// VAL-PANE-003: core_repo() is not used for pane working directories
#[test]
fn test_no_pane_uses_core_repo_worktree() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();
    let wagner = ctx.wagner();

    let specs = vec![
        RepoSpec {
            name: "repo1".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/no-core".to_string(),
        },
        RepoSpec {
            name: "repo2".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/no-core".to_string(),
        },
    ];

    let task = wagner
        .create_task("no-core-task", &specs, Some("main"))
        .unwrap();

    // No pane should have the task name as repo_name (that's what core_repo() would produce)
    for pane in &task.panes {
        assert_ne!(
            pane.repo_name, "no-core-task",
            "No pane should use synthetic core_repo (task name as repo_name). Found pane '{}' with repo_name '{}'",
            pane.name, pane.repo_name
        );
    }

    // All panes should correspond to actual repos in task.repos
    let repo_names: Vec<&str> = task.repos.iter().map(|r| r.name.as_str()).collect();
    for pane in &task.panes {
        assert!(
            repo_names.contains(&pane.repo_name.as_str()),
            "Pane repo_name '{}' should be in task.repos: {:?}",
            pane.repo_name,
            repo_names
        );
    }
}

// VAL-PANE-004: Single-repo add_pane uses that repo's worktree
#[test]
fn test_add_pane_single_repo_uses_worktree() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/single-add".to_string(),
    };

    wagner
        .create_task("single-add-task", &[spec], None)
        .unwrap();

    // Add a second pane (session already exists)
    wagner.add_pane("single-add-task", None, None).unwrap();

    let task = wagner.get_task("single-add-task").unwrap();
    let repo_worktree = &task.repos[0].worktree;

    // The new pane should be created in the repo's worktree
    let created_panes = wagner.terminal.get_created_panes();
    assert!(
        !created_panes.is_empty(),
        "Should have created at least one additional pane"
    );

    let last_pane = &created_panes[created_panes.len() - 1];
    assert_eq!(
        &last_pane.1, repo_worktree,
        "add_pane on single-repo should use repo.worktree"
    );
}

// VAL-PANE-005: Single-repo create_session_with_panes uses worktree
#[test]
fn test_create_session_single_repo_uses_worktree() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/single-session".to_string(),
    };

    let task = wagner
        .create_task("single-session-task", &[spec], None)
        .unwrap();

    let created_sessions = wagner.terminal.get_created_sessions();
    assert!(!created_sessions.is_empty(), "Should have created a session");

    let session_dir = &created_sessions[0].1;
    let repo_worktree = &task.repos[0].worktree;

    assert_eq!(
        session_dir, repo_worktree,
        "Single-repo session should be created in repos[0].worktree"
    );
}

// VAL-PANE-011: Multi-repo creates pane per repo with correct worktrees
#[test]
fn test_create_session_multi_repo_all_panes_have_correct_worktrees() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();
    let wagner = ctx.wagner();

    let specs = vec![
        RepoSpec {
            name: "repo1".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/all-panes".to_string(),
        },
        RepoSpec {
            name: "repo2".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/all-panes".to_string(),
        },
    ];

    let task = wagner
        .create_task("all-panes-task", &specs, Some("main"))
        .unwrap();

    // Should have exactly 2 panes (one per repo), not 3 (no synthetic core pane)
    assert_eq!(
        task.panes.len(),
        2,
        "Should have exactly 2 panes (one per repo)"
    );

    // Each pane should correspond to a real repo
    assert_eq!(task.panes[0].repo_name, "repo1");
    assert_eq!(task.panes[1].repo_name, "repo2");

    // The second pane (repo2) should be created via create_pane with correct worktree
    let created_panes = wagner.terminal.get_created_panes();
    assert_eq!(
        created_panes.len(),
        1,
        "Should have 1 additional pane created (first pane comes with session)"
    );

    let repo2_worktree = &task.repos[1].worktree;
    assert_eq!(
        &created_panes[0].1, repo2_worktree,
        "Additional pane should be created in repo2's worktree"
    );
}

// VAL-PANE-012: add_pane with explicit repo_name uses that repo's worktree
#[test]
fn test_add_pane_explicit_repo_uses_correct_worktree() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();
    let wagner = ctx.wagner();

    let specs = vec![
        RepoSpec {
            name: "repo1".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/explicit".to_string(),
        },
        RepoSpec {
            name: "repo2".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/explicit".to_string(),
        },
    ];

    let task = wagner
        .create_task("explicit-repo-task", &specs, Some("main"))
        .unwrap();

    // Add a pane specifically for repo2
    wagner
        .add_pane("explicit-repo-task", Some("repo2"), None)
        .unwrap();

    let created_panes = wagner.terminal.get_created_panes();
    let repo2_worktree = &task.repos[1].worktree;

    // The last created pane should be for repo2's worktree
    let last_pane = &created_panes[created_panes.len() - 1];
    assert_eq!(
        &last_pane.1, repo2_worktree,
        "Explicit repo_name='repo2' should create pane in repo2's worktree"
    );
}

// VAL-PANE-013: add_pane multi-repo default selects first repo, not core_repo
#[test]
fn test_add_pane_multi_repo_default_selects_real_repo() {
    let ctx = TestContext::new();
    let repo2_path = ctx.add_second_repo();
    let wagner = ctx.wagner();

    let specs = vec![
        RepoSpec {
            name: "repo1".to_string(),
            source: RepoSource::Local(ctx.repo_path.clone()),
            branch: "feature/default-repo".to_string(),
        },
        RepoSpec {
            name: "repo2".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/default-repo".to_string(),
        },
    ];

    let task = wagner
        .create_task("default-repo-task", &specs, Some("main"))
        .unwrap();

    // add_pane with repo_name=None on multi-repo should default to repos[0]
    wagner.add_pane("default-repo-task", None, None).unwrap();

    let updated_task = wagner.get_task("default-repo-task").unwrap();

    // The newly added pane (last one) should be for repos[0], not a synthetic core_repo
    let last_pane = updated_task.panes.last().unwrap();
    assert_eq!(
        last_pane.repo_name, "repo1",
        "Default repo_name on multi-repo should be repos[0].name ('repo1'), not task name"
    );

    // The pane should be created in repo1's worktree
    let repo1_worktree = &task.repos[0].worktree;
    let created_panes = wagner.terminal.get_created_panes();
    let last_created = &created_panes[created_panes.len() - 1];
    assert_eq!(
        &last_created.1, repo1_worktree,
        "Default pane on multi-repo should use repos[0].worktree"
    );
}

// VAL-PANE-001 (with_engine variant): Session recreation with engine uses repo worktree
#[test]
fn test_add_pane_with_engine_session_recreation_uses_repo_worktree() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    let spec = RepoSpec {
        name: "main".to_string(),
        source: RepoSource::Local(ctx.repo_path.clone()),
        branch: "feature/engine-recreate".to_string(),
    };

    wagner
        .create_task("engine-recreate-task", &[spec], None)
        .unwrap();

    // Kill the session
    let session_name = session_name_for_task("engine-recreate-task");
    wagner
        .terminal
        .kill_session(&SessionHandle(session_name))
        .unwrap();

    // add_pane_with_engine should recreate the session with repo.worktree
    wagner
        .add_pane_with_engine(
            "engine-recreate-task",
            None,
            None,
            Some(Engine::ClaudeCode),
        )
        .unwrap();

    let created_sessions = wagner.terminal.get_created_sessions();
    assert!(
        created_sessions.len() >= 2,
        "Should have at least 2 session creations"
    );

    let task = wagner.get_task("engine-recreate-task").unwrap();
    let repo_worktree = &task.repos[0].worktree;
    let last_session = &created_sessions[created_sessions.len() - 1];
    assert_eq!(
        &last_session.1, repo_worktree,
        "Engine session recreation should use repo.worktree"
    );
    assert_ne!(
        last_session.1, task.path,
        "Engine session recreation must NOT use task.path"
    );
}
