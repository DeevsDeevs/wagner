/// Parity test: proves that the `command_executor` AddPane path and the direct
/// `Wagner::add_pane_with_engine` path produce equivalent tracked pane/session
/// outcomes under the same fixture.
///
/// Addresses the scrutiny finding that the dedup feature lacks explicit proof
/// that both code paths produce identical results.
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

use wagner::config::Config;
use wagner::core::WagnerCore;
use wagner::model::{Engine, PENDING_DISCOVERY};
use wagner::store::Store;
use wagner::transport::{CoreCommand, CoreResponse};
use wagner::{MockTerminal, RepoSource, RepoSpec, Terminal, TestAgent, Wagner};

/// Shared test fixture: creates a temp dir with a git repo and a tasks_root.
struct ParityFixture {
    _temp_dir: TempDir,
    tasks_root: PathBuf,
    repo_path: PathBuf,
}

impl ParityFixture {
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

        std::fs::write(repo_path.join("README.md"), "# Parity Test Repo").unwrap();

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

    /// Create a task via Wagner with a single repo (uses create_task for managed tasks).
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

    /// Add a pane via the direct Wagner::add_pane_with_engine path.
    fn add_pane_direct(
        &self,
        terminal: &MockTerminal,
        task_name: &str,
        pane_name: Option<&str>,
        engine: Option<Engine>,
    ) -> wagner::TrackedPane {
        let wagner = Wagner::new(terminal.clone(), TestAgent::echo(), self.config());
        let _pane_handle = wagner
            .add_pane_with_engine(task_name, None, pane_name, engine)
            .unwrap();

        // Load the freshly saved task and return the last pane
        let task = wagner.get_task(task_name).unwrap();
        task.panes.last().cloned().unwrap()
    }

    /// Add a pane via command_executor's CoreCommand::AddPane path.
    fn add_pane_executor(
        &self,
        terminal: &MockTerminal,
        task_name: &str,
        pane_name: Option<&str>,
        agent: Option<&str>,
    ) -> wagner::TrackedPane {
        let config = self.config();
        let store = Store::new(config.clone());
        let core = WagnerCore::new(config);
        let tasks = store.list_tasks().unwrap_or_default();

        let resp = core.execute(
            terminal,
            &store,
            &CoreCommand::AddPane {
                task_name: task_name.to_string(),
                pane_name: pane_name.map(String::from),
                agent: agent.map(String::from),
                repo_name: None,
            },
            &tasks,
        );

        match resp {
            CoreResponse::Confirmation { message } => {
                assert!(
                    !message.is_empty(),
                    "Expected non-empty confirmation for AddPane"
                );
            }
            other => panic!("Expected Confirmation from AddPane executor, got: {other:?}"),
        }

        // Load updated task and return the last pane
        let store = Store::new(self.config());
        let task = store.load_task(task_name).unwrap();
        task.panes.last().cloned().unwrap()
    }
}

/// Engine string used in CoreCommand::AddPane for a given Engine variant.
fn engine_to_agent_string(engine: Engine) -> Option<&'static str> {
    match engine {
        Engine::ClaudeCode => Some("claude"),
        Engine::Codex => Some("codex"),
        Engine::Droid => Some("droid"),
        Engine::Terminal => Some("terminal"),
    }
}

/// Assert structural equivalence between two TrackedPane values, ignoring
/// fields that are expected to differ (session_id, pane_id, launched_at, jsonl_path suffix).
fn assert_pane_parity(direct: &wagner::TrackedPane, executor: &wagner::TrackedPane, label: &str) {
    // Same engine
    assert_eq!(
        direct.engine, executor.engine,
        "[{label}] Engine mismatch: direct={:?}, executor={:?}",
        direct.engine, executor.engine,
    );

    // Same repo name
    assert_eq!(
        direct.repo_name, executor.repo_name,
        "[{label}] repo_name mismatch: direct={}, executor={}",
        direct.repo_name, executor.repo_name,
    );

    // Same pane name pattern (both auto-generated with same prefix, or same explicit name)
    // Note: the exact numeric suffix may differ due to dedup counting, but the base pattern
    // should match. For explicit names, they should be identical.
    let direct_base = pane_name_base(&direct.name);
    let executor_base = pane_name_base(&executor.name);
    assert_eq!(
        direct_base, executor_base,
        "[{label}] Pane name base pattern differs: direct='{}' (base='{direct_base}'), executor='{}' (base='{executor_base}')",
        direct.name, executor.name,
    );

    // JSONL path structure: both should have the same directory structure and pattern
    // (differs only in the UUID-based session_id filename).
    let direct_jsonl_parent = direct.jsonl_path.parent().map(|p| p.to_path_buf());
    let executor_jsonl_parent = executor.jsonl_path.parent().map(|p| p.to_path_buf());
    assert_eq!(
        direct_jsonl_parent, executor_jsonl_parent,
        "[{label}] JSONL path parent directory differs: direct={:?}, executor={:?}",
        direct.jsonl_path, executor.jsonl_path,
    );

    // Both should have .jsonl extension (or both pending-discovery)
    if direct.jsonl_path.to_string_lossy() == PENDING_DISCOVERY {
        assert_eq!(
            executor.jsonl_path.to_string_lossy(),
            PENDING_DISCOVERY,
            "[{label}] Direct has pending-discovery JSONL but executor doesn't"
        );
    } else {
        let direct_ext = direct.jsonl_path.extension().and_then(|e| e.to_str());
        let executor_ext = executor.jsonl_path.extension().and_then(|e| e.to_str());
        assert_eq!(
            direct_ext, executor_ext,
            "[{label}] JSONL extension mismatch: direct={direct_ext:?}, executor={executor_ext:?}"
        );
    }

    // Both session IDs should be valid UUIDs (non-empty, different because they're independently generated)
    assert!(
        !direct.session_id.is_empty(),
        "[{label}] Direct pane session_id is empty"
    );
    assert!(
        !executor.session_id.is_empty(),
        "[{label}] Executor pane session_id is empty"
    );
}

/// Extract the base name from a pane name (strip trailing numeric suffix like "-2", "-3").
fn pane_name_base(name: &str) -> &str {
    // Pane names follow "{base}" or "{base}-{number}" pattern from next_pane_name
    if let Some(pos) = name.rfind('-') {
        let suffix = &name[pos + 1..];
        if suffix.chars().all(|c| c.is_ascii_digit()) && !suffix.is_empty() {
            return &name[..pos];
        }
    }
    name
}

// ──────────────────────────────────────────────────────────────
// Parity tests: same fixture, both paths, structural equivalence
// ──────────────────────────────────────────────────────────────

/// ClaudeCode engine: both paths produce equivalent tracked panes.
#[test]
fn test_parity_claude_add_pane_both_paths_equivalent() {
    let fixture = ParityFixture::new();
    let terminal = MockTerminal::new();
    fixture.create_task(&terminal, "parity-claude");

    let direct = fixture.add_pane_direct(&terminal, "parity-claude", None, Some(Engine::ClaudeCode));
    let executor = fixture.add_pane_executor(&terminal, "parity-claude", None, engine_to_agent_string(Engine::ClaudeCode));

    assert_pane_parity(&direct, &executor, "ClaudeCode");

    // Claude should have a real JSONL path (not pending-discovery)
    assert_ne!(
        direct.jsonl_path.to_string_lossy(),
        PENDING_DISCOVERY,
        "ClaudeCode direct pane should have real JSONL path"
    );
    assert_ne!(
        executor.jsonl_path.to_string_lossy(),
        PENDING_DISCOVERY,
        "ClaudeCode executor pane should have real JSONL path"
    );
}

/// Codex engine: both paths produce equivalent tracked panes.
#[test]
fn test_parity_codex_add_pane_both_paths_equivalent() {
    let fixture = ParityFixture::new();
    let terminal = MockTerminal::new();
    fixture.create_task(&terminal, "parity-codex");

    let direct = fixture.add_pane_direct(&terminal, "parity-codex", None, Some(Engine::Codex));
    let executor = fixture.add_pane_executor(&terminal, "parity-codex", None, engine_to_agent_string(Engine::Codex));

    assert_pane_parity(&direct, &executor, "Codex");

    // Codex should have pending-discovery JSONL (no prediction)
    assert_eq!(
        direct.jsonl_path.to_string_lossy(),
        PENDING_DISCOVERY,
        "Codex direct pane should have pending-discovery JSONL"
    );
    assert_eq!(
        executor.jsonl_path.to_string_lossy(),
        PENDING_DISCOVERY,
        "Codex executor pane should have pending-discovery JSONL"
    );
}

/// Terminal engine: both paths produce equivalent tracked panes (no agent launch).
#[test]
fn test_parity_terminal_add_pane_both_paths_equivalent() {
    let fixture = ParityFixture::new();
    let terminal = MockTerminal::new();
    fixture.create_task(&terminal, "parity-terminal");

    let keys_before = terminal.get_sent_keys().len();

    let direct = fixture.add_pane_direct(&terminal, "parity-terminal", None, Some(Engine::Terminal));
    let keys_after_direct = terminal.get_sent_keys().len();

    let executor = fixture.add_pane_executor(&terminal, "parity-terminal", None, engine_to_agent_string(Engine::Terminal));
    let keys_after_executor = terminal.get_sent_keys().len();

    assert_pane_parity(&direct, &executor, "Terminal");

    // Terminal engine should NOT send any agent launch commands from either path
    // The only sent_keys should be from create_task's initial agent launch.
    // After create_task, neither add_pane path should add new launch keys for Terminal.
    let direct_new_keys = keys_after_direct - keys_before;
    let executor_new_keys = keys_after_executor - keys_after_direct;
    assert_eq!(
        direct_new_keys, executor_new_keys,
        "Terminal pane: both paths should send same number of keys (direct={direct_new_keys}, executor={executor_new_keys})"
    );
}

/// Explicit pane name: both paths respect the user-specified name.
#[test]
fn test_parity_explicit_name_both_paths_equivalent() {
    let fixture = ParityFixture::new();
    let terminal = MockTerminal::new();
    fixture.create_task(&terminal, "parity-name");

    let direct = fixture.add_pane_direct(&terminal, "parity-name", Some("my-custom-pane"), Some(Engine::ClaudeCode));
    // Note: the executor path will encounter "my-custom-pane" already existing
    // and auto-dedup to "my-custom-pane-2". To test true parity with an explicit name,
    // we use a different unique name for the executor path.
    let executor = fixture.add_pane_executor(&terminal, "parity-name", Some("another-custom"), engine_to_agent_string(Engine::ClaudeCode));

    // Both should have the explicit names we gave them (no auto-prefix)
    assert_eq!(direct.name, "my-custom-pane", "Direct path should use explicit name");
    assert_eq!(executor.name, "another-custom", "Executor path should use explicit name");

    // Structural parity still holds (engine, repo, JSONL structure)
    assert_eq!(direct.engine, executor.engine);
    assert_eq!(direct.repo_name, executor.repo_name);
    let direct_jsonl_parent = direct.jsonl_path.parent();
    let executor_jsonl_parent = executor.jsonl_path.parent();
    assert_eq!(direct_jsonl_parent, executor_jsonl_parent);
}

/// Session directory: when session must be recreated, both paths use repo.worktree.
#[test]
fn test_parity_session_directory_both_paths_use_worktree() {
    let fixture = ParityFixture::new();
    let terminal = MockTerminal::new();
    fixture.create_task(&terminal, "parity-dir");

    let store = Store::new(fixture.config());
    let task = store.load_task("parity-dir").unwrap();
    let repo_worktree = task.repos[0].worktree.clone();

    // Track session creation count before our test
    let sessions_before = terminal.get_created_sessions().len();

    // Kill session to force recreation on next add_pane
    let session_name = wagner::terminal::session_name_for_task("parity-dir");
    terminal
        .kill_session(&wagner::SessionHandle(session_name.clone()))
        .unwrap();

    // Direct path: should recreate session with repo.worktree
    let _direct = fixture.add_pane_direct(&terminal, "parity-dir", None, Some(Engine::ClaudeCode));
    let sessions_after_direct = terminal.get_created_sessions();
    let direct_session = sessions_after_direct.last().unwrap();
    assert_eq!(
        direct_session.1, repo_worktree,
        "Direct path: session recreation should use repo.worktree"
    );

    // Kill session again to test executor path
    terminal
        .kill_session(&wagner::SessionHandle(session_name))
        .unwrap();

    // Executor path: should also recreate session with repo.worktree
    let _executor = fixture.add_pane_executor(&terminal, "parity-dir", None, engine_to_agent_string(Engine::ClaudeCode));
    let sessions_after_executor = terminal.get_created_sessions();
    let executor_session = sessions_after_executor.last().unwrap();
    assert_eq!(
        executor_session.1, repo_worktree,
        "Executor path: session recreation should use repo.worktree"
    );

    // Both used the same directory
    assert_eq!(
        direct_session.1, executor_session.1,
        "Both paths should recreate session in the same directory (repo.worktree)"
    );

    // Neither should use task.path
    assert_ne!(
        direct_session.1, task.path,
        "Direct path must not use task.path for session"
    );
    assert_ne!(
        executor_session.1, task.path,
        "Executor path must not use task.path for session"
    );

    // We should have at least 2 new session creations (one per path)
    let new_session_count = sessions_after_executor.len() - sessions_before;
    assert!(
        new_session_count >= 2,
        "Expected at least 2 session recreations, got {new_session_count}"
    );
}

/// Multi-repo: both paths default to repos[0] when no repo_name is specified.
#[test]
fn test_parity_multi_repo_default_both_paths_equivalent() {
    let fixture = ParityFixture::new();
    let terminal = MockTerminal::new();

    // Create a second repo
    let repo2_path = fixture._temp_dir.path().join("test-repo-2");
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

    let wagner = Wagner::new(terminal.clone(), TestAgent::echo(), fixture.config());
    let specs = vec![
        RepoSpec {
            name: "frontend".to_string(),
            source: RepoSource::Local(fixture.repo_path.clone()),
            branch: "feature/parity-multi".to_string(),
        },
        RepoSpec {
            name: "backend".to_string(),
            source: RepoSource::Local(repo2_path),
            branch: "feature/parity-multi".to_string(),
        },
    ];
    wagner.create_task("parity-multi", &specs, None).unwrap();

    // Direct path with repo_name=None should use repos[0] ("frontend")
    let direct = fixture.add_pane_direct(&terminal, "parity-multi", None, Some(Engine::ClaudeCode));

    // Executor path with repo_name=None should also use repos[0] ("frontend")
    let executor = fixture.add_pane_executor(&terminal, "parity-multi", None, engine_to_agent_string(Engine::ClaudeCode));

    assert_eq!(
        direct.repo_name, "frontend",
        "Direct path should default to repos[0] (frontend)"
    );
    assert_eq!(
        executor.repo_name, "frontend",
        "Executor path should default to repos[0] (frontend)"
    );
    assert_pane_parity(&direct, &executor, "MultiRepo default");
}
