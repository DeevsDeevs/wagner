use std::path::PathBuf;
use tempfile::TempDir;
use wagner::{
    Config, Engine, MockTerminal, RepoSource, Store, Task, TaskRepo, Terminal, TestAgent, Wagner,
};

struct QuickLaunchCtx {
    _temp_dir: TempDir,
    tasks_root: PathBuf,
    worktree_path: PathBuf,
}

impl QuickLaunchCtx {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let tasks_root = temp_dir.path().join("tasks");
        let worktree_path = temp_dir.path().join("my-worktree");

        std::fs::create_dir_all(&tasks_root).unwrap();
        std::fs::create_dir_all(&worktree_path).unwrap();

        Self {
            _temp_dir: temp_dir,
            tasks_root,
            worktree_path,
        }
    }

    fn config(&self) -> Config {
        Config {
            tasks_root: self.tasks_root.clone(),
            ..Config::default()
        }
    }

    fn store(&self) -> Store {
        Store::new(self.config())
    }

    /// Create and persist a task with one repo pointing at `worktree_path`.
    fn create_persisted_task(&self, task_name: &str) -> Task {
        let repo = TaskRepo {
            name: "myrepo".to_string(),
            source: RepoSource::Local(self.worktree_path.clone()),
            worktree: self.worktree_path.clone(),
            branch: "main".to_string(),
        };

        let task = Task::new_attached(task_name, self.worktree_path.clone(), vec![repo]);
        self.store().save_task(&task).unwrap();
        task
    }

    /// Create some "important data" files inside the worktree and task metadata
    /// directories to verify they survive quick_launch with a dead session.
    fn create_task_data(&self, task_name: &str) {
        // Simulate plugin data inside the worktree
        let plugin_dir = self.worktree_path.join(".wagner").join("plugins");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("important.txt"), "precious data").unwrap();

        // Simulate a WIP file in the worktree
        std::fs::write(self.worktree_path.join("wip.rs"), "fn work_in_progress() {}").unwrap();

        // The task metadata itself (task.json) is stored by create_persisted_task
        // but let's also verify the metadata dir is there
        let metadata_dir = self.worktree_path.join(".wagner");
        assert!(
            metadata_dir.exists(),
            "metadata dir should exist after save_task"
        );

        // For a managed task, metadata would be in tasks_root/task_name/.wagner
        // For an attached task, metadata is in task.path/.wagner (worktree_path here)
        let task_json = metadata_dir.join("task.json");
        assert!(
            task_json.exists(),
            "task.json should exist after save_task, checking: {}",
            task_json.display()
        );
        let _ = task_name;
    }

    fn wagner_with_terminal(
        &self,
        terminal: MockTerminal,
    ) -> Wagner<MockTerminal, TestAgent> {
        Wagner::new(terminal, TestAgent::echo(), self.config())
    }
}

/// VAL-PANE-008: quick_launch finds an existing task but no live tmux session —
/// it must recreate the session, never call store.delete_task(). Task metadata
/// and worktrees remain intact.
#[test]
fn test_quick_launch_dead_session_preserves_task() {
    let ctx = QuickLaunchCtx::new();
    let task_name = "test-task";

    // 1. Pre-create and persist the task
    ctx.create_persisted_task(task_name);
    ctx.create_task_data(task_name);

    // 2. Build a MockTerminal where session_exists returns false (dead session)
    let terminal = MockTerminal::new();
    // MockTerminal has no sessions => session_exists returns false

    let wagner = ctx.wagner_with_terminal(terminal.clone());

    // 3. Call quick_launch with the existing task name
    let result = wagner.quick_launch(Engine::ClaudeCode, Some(task_name));
    assert!(result.is_ok(), "quick_launch should succeed: {:?}", result);

    // 4. Verify task still exists in the store (was NOT deleted)
    assert!(
        ctx.store().task_exists(task_name),
        "Task must still exist after quick_launch with dead session"
    );

    // 5. Verify task metadata file still exists on disk
    let task_json = ctx.worktree_path.join(".wagner").join("task.json");
    assert!(
        task_json.exists(),
        "task.json must survive quick_launch with dead session"
    );

    // 6. Verify the loaded task still has its repos
    let loaded_task = ctx.store().load_task(task_name).unwrap();
    assert_eq!(loaded_task.repos.len(), 1);
    assert_eq!(loaded_task.repos[0].name, "myrepo");
    assert_eq!(loaded_task.repos[0].worktree, ctx.worktree_path);
}

/// VAL-PANE-009: When quick_launch encounters a dead session, filesystem
/// worktrees must not be deleted.
#[test]
fn test_quick_launch_dead_session_preserves_worktrees() {
    let ctx = QuickLaunchCtx::new();
    let task_name = "test-task";

    ctx.create_persisted_task(task_name);
    ctx.create_task_data(task_name);

    let terminal = MockTerminal::new();
    let wagner = ctx.wagner_with_terminal(terminal);

    wagner
        .quick_launch(Engine::ClaudeCode, Some(task_name))
        .unwrap();

    // Worktree directory must still exist
    assert!(
        ctx.worktree_path.exists(),
        "Worktree directory must survive quick_launch with dead session"
    );

    // Plugin data must still exist
    let plugin_file = ctx
        .worktree_path
        .join(".wagner")
        .join("plugins")
        .join("important.txt");
    assert!(
        plugin_file.exists(),
        "Plugin data must survive quick_launch with dead session"
    );
    assert_eq!(
        std::fs::read_to_string(&plugin_file).unwrap(),
        "precious data"
    );

    // WIP code must still exist
    let wip_file = ctx.worktree_path.join("wip.rs");
    assert!(
        wip_file.exists(),
        "WIP code must survive quick_launch with dead session"
    );
    assert_eq!(
        std::fs::read_to_string(&wip_file).unwrap(),
        "fn work_in_progress() {}"
    );
}

/// VAL-PANE-010: After detecting a dead session, quick_launch must create a new
/// tmux session in the correct directory (repo.worktree) and relaunch the agent.
#[test]
fn test_quick_launch_dead_session_recreates_and_launches() {
    let ctx = QuickLaunchCtx::new();
    let task_name = "test-task";

    ctx.create_persisted_task(task_name);

    let terminal = MockTerminal::new();
    let wagner = ctx.wagner_with_terminal(terminal.clone());

    wagner
        .quick_launch(Engine::ClaudeCode, Some(task_name))
        .unwrap();

    // Verify session was created with the correct directory (repo.worktree)
    let created_sessions = terminal.get_created_sessions();
    assert_eq!(
        created_sessions.len(),
        1,
        "Exactly one session should be created"
    );
    assert_eq!(created_sessions[0].0, task_name);
    assert_eq!(
        created_sessions[0].1, ctx.worktree_path,
        "Session must be created in repo.worktree, not task.path"
    );

    // Verify the agent was launched (sent keys for ClaudeCode launch command)
    let sent_keys = terminal.get_sent_keys();
    let has_claude_launch = sent_keys.iter().any(|(_pane, keys)| {
        keys.contains("claude --session-id")
    });
    assert!(
        has_claude_launch,
        "Agent must be relaunched in recreated session. Sent keys: {:?}",
        sent_keys
    );

    // Verify the task now has a tracked pane
    let loaded_task = ctx.store().load_task(task_name).unwrap();
    assert_eq!(
        loaded_task.panes.len(),
        1,
        "Task should have exactly one tracked pane after relaunch"
    );
    assert_eq!(loaded_task.panes[0].engine, Engine::ClaudeCode);
    assert_eq!(loaded_task.panes[0].repo_name, "myrepo");
}

/// Verify quick_launch with a live session still calls resume_dead_agents and
/// attaches without modifying task data.
#[test]
fn test_quick_launch_live_session_resumes_and_attaches() {
    let ctx = QuickLaunchCtx::new();
    let task_name = "test-task";

    let task = ctx.create_persisted_task(task_name);

    // Create a terminal with a pre-existing session
    let terminal = MockTerminal::new();
    // Create the session so session_exists returns true
    terminal
        .create_session(task_name, &ctx.worktree_path)
        .unwrap();

    // Set the pane command to "bash" (not claude) so resume_dead_agents would try to resume
    // But since the task has no tracked panes, resume_dead_agents returns Ok(0)
    let wagner = ctx.wagner_with_terminal(terminal.clone());

    let result = wagner.quick_launch(Engine::ClaudeCode, Some(task_name));
    assert!(result.is_ok(), "quick_launch should succeed: {:?}", result);

    // Verify NO new session was created (the pre-existing one should be reused)
    let created_sessions = terminal.get_created_sessions();
    assert_eq!(
        created_sessions.len(),
        1,
        "Only the initial session should exist (no recreation)"
    );

    // Task data should be untouched
    let loaded_task = ctx.store().load_task(task_name).unwrap();
    assert_eq!(loaded_task.repos.len(), task.repos.len());
    assert_eq!(loaded_task.repos[0].worktree, ctx.worktree_path);
}

/// Verify that quick_launch with a dead session clears stale pane tracking and
/// creates fresh pane entries with the correct engine type.
#[test]
fn test_quick_launch_dead_session_clears_stale_panes() {
    let ctx = QuickLaunchCtx::new();
    let task_name = "test-task";

    // Create a task with stale pane tracking from a previous session
    let mut task = ctx.create_persisted_task(task_name);
    let stale_pane = wagner::TrackedPane {
        name: "old-pane".to_string(),
        repo_name: "myrepo".to_string(),
        engine: Engine::ClaudeCode,
        session_id: "old-session-id".to_string(),
        pane_id: "old-pane-id".to_string(),
        jsonl_path: "pending-discovery".into(),
        launched_at: chrono::Utc::now(),
    };
    task.panes.push(stale_pane);
    ctx.store().save_task(&task).unwrap();

    let terminal = MockTerminal::new();
    let wagner = ctx.wagner_with_terminal(terminal);

    wagner
        .quick_launch(Engine::Codex, Some(task_name))
        .unwrap();

    let loaded_task = ctx.store().load_task(task_name).unwrap();

    // Stale pane should be gone, replaced by fresh tracking
    assert_eq!(
        loaded_task.panes.len(),
        1,
        "Should have exactly one pane after recreation"
    );
    assert_ne!(
        loaded_task.panes[0].session_id, "old-session-id",
        "Pane should have new session_id, not the stale one"
    );
    assert_eq!(
        loaded_task.panes[0].engine,
        Engine::Codex,
        "Pane should use the engine passed to quick_launch"
    );
}

/// Verify that quick_launch correctly relaunches with the specified engine type,
/// not just the default agent engine.
#[test]
fn test_quick_launch_dead_session_uses_specified_engine() {
    let ctx = QuickLaunchCtx::new();
    let task_name = "test-task";

    ctx.create_persisted_task(task_name);

    let terminal = MockTerminal::new();
    let wagner = ctx.wagner_with_terminal(terminal.clone());

    // Launch with Codex engine (not ClaudeCode which is TestAgent's default)
    wagner
        .quick_launch(Engine::Codex, Some(task_name))
        .unwrap();

    let loaded_task = ctx.store().load_task(task_name).unwrap();
    assert_eq!(loaded_task.panes[0].engine, Engine::Codex);

    // Verify the codex launch command was sent
    let sent_keys = terminal.get_sent_keys();
    let has_codex_launch = sent_keys.iter().any(|(_pane, keys)| keys.contains("codex"));
    assert!(
        has_codex_launch,
        "Codex launch command should be sent. Sent keys: {:?}",
        sent_keys
    );
}

/// Verify Terminal engine quick_launch with dead session does NOT send any
/// launch command (terminals don't have agents to launch).
#[test]
fn test_quick_launch_dead_session_terminal_no_launch_command() {
    let ctx = QuickLaunchCtx::new();
    let task_name = "test-task";

    ctx.create_persisted_task(task_name);

    let terminal = MockTerminal::new();
    let wagner = ctx.wagner_with_terminal(terminal.clone());

    wagner
        .quick_launch(Engine::Terminal, Some(task_name))
        .unwrap();

    let loaded_task = ctx.store().load_task(task_name).unwrap();
    assert_eq!(loaded_task.panes[0].engine, Engine::Terminal);

    // For Terminal engine, no launch/resume command should be sent
    // The only sent keys would be from the shell_init_delay + send_text_enter path,
    // but Terminal engine skips that path (engine_type != Engine::Terminal check)
    // Actually: prepare_agent_in_pane_with_engine skips launch for Terminal
    // So no claude/codex commands should appear
    let sent_keys = terminal.get_sent_keys();
    let has_agent_launch = sent_keys.iter().any(|(_pane, keys)| {
        keys.contains("claude") || keys.contains("codex")
    });
    assert!(
        !has_agent_launch,
        "No agent launch command should be sent for Terminal engine. Sent keys: {:?}",
        sent_keys
    );
}
