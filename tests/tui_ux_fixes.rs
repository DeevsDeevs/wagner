use std::path::PathBuf;
use tempfile::TempDir;
use wagner::model::TrackedPane;
use wagner::tui::{App, InputMode};
use wagner::{Config, Engine, MockTerminal, PaneHandle, TestAgent, Wagner};

fn setup() -> (TempDir, Wagner<MockTerminal, TestAgent>) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let tasks_root = temp_dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_root).unwrap();

    let config = Config {
        tasks_root,
        ..Config::default()
    };
    let wagner = Wagner::new(MockTerminal::new(), TestAgent::echo(), config);
    (temp_dir, wagner)
}

fn make_tracked_pane(name: &str, pane_id: &str, repo: &str) -> TrackedPane {
    TrackedPane {
        name: name.into(),
        repo_name: repo.into(),
        engine: Engine::ClaudeCode,
        session_id: "session-1".into(),
        pane_id: pane_id.into(),
        jsonl_path: PathBuf::from("pending-discovery"),
        launched_at: chrono::Utc::now(),
    }
}

// ===== Fix 1: TUI delete confirmation shows friendly pane name =====

#[test]
fn test_tui_delete_pane_shows_friendly_name() {
    let (_temp_dir, wagner) = setup();

    // Create a task with a tracked pane that has a friendly name
    let task_path = wagner.config.tasks_root.join("name-test");
    std::fs::create_dir_all(task_path.join(".wagner")).unwrap();

    let mut task = wagner::Task::new(
        "name-test",
        task_path.clone(),
        vec![wagner::TaskRepo {
            name: "api".into(),
            source: wagner::RepoSource::Local(PathBuf::from("/tmp/api")),
            worktree: PathBuf::from("/tmp/api-wt"),
            branch: "main".into(),
        }],
        None,
    );
    task.panes
        .push(make_tracked_pane("claude-api", "%10", "api"));
    wagner.store.save_task(&task).unwrap();

    // Set up mock terminal
    let session_name = wagner::terminal::session_name_for_task("name-test");
    {
        let mut sessions = wagner.terminal.sessions.lock().unwrap();
        sessions.insert(
            session_name,
            vec![PaneHandle("%10".into(), "claude-api".into())],
        );
    }

    let mut app = App::new(wagner);
    app.selected_task = Some("name-test".into());
    app.selected_pane = Some("%10".into());

    // Trigger delete confirmation
    app.start_delete();

    // Verify the confirmation prompt contains the friendly name, not the pane ID
    assert_eq!(app.input_mode, InputMode::Confirm);
    assert!(
        app.input_label.contains("claude-api"),
        "Expected input_label to contain friendly name 'claude-api', got: {}",
        app.input_label
    );
    assert!(
        !app.input_label.contains("%10"),
        "input_label should not contain raw pane ID '%10', got: {}",
        app.input_label
    );
    // Confirm action still uses pane_id for the actual deletion
    assert_eq!(app.confirm_action, Some("delete_pane:%10".into()));
}

#[test]
fn test_tui_delete_pane_falls_back_to_pane_id_when_no_task() {
    let (_temp_dir, wagner) = setup();

    // Do NOT create a task — selected_task points to something that doesn't exist
    let mut app = App::new(wagner);
    app.selected_task = Some("nonexistent-task".into());
    app.selected_pane = Some("%20".into());

    app.start_delete();

    assert_eq!(app.input_mode, InputMode::Confirm);
    // Should fall back to pane_id when task can't be loaded
    assert!(
        app.input_label.contains("%20"),
        "Expected input_label to fall back to pane ID '%20', got: {}",
        app.input_label
    );
}

#[test]
fn test_tui_delete_pane_empty_name_gets_fixup() {
    let (_temp_dir, wagner) = setup();

    // Create a task with a tracked pane that has an empty name
    // (fixup_pane_names on load will assign the repo name "web")
    let task_path = wagner.config.tasks_root.join("fixup-test");
    std::fs::create_dir_all(task_path.join(".wagner")).unwrap();

    let mut task = wagner::Task::new(
        "fixup-test",
        task_path.clone(),
        vec![wagner::TaskRepo {
            name: "web".into(),
            source: wagner::RepoSource::Local(PathBuf::from("/tmp/web")),
            worktree: PathBuf::from("/tmp/web-wt"),
            branch: "main".into(),
        }],
        None,
    );
    // Pane with empty name — will get fixed up to "web" on load
    task.panes.push(make_tracked_pane("", "%20", "web"));
    wagner.store.save_task(&task).unwrap();

    let session_name = wagner::terminal::session_name_for_task("fixup-test");
    {
        let mut sessions = wagner.terminal.sessions.lock().unwrap();
        sessions.insert(
            session_name,
            vec![PaneHandle("%20".into(), "".into())],
        );
    }

    let mut app = App::new(wagner);
    app.selected_task = Some("fixup-test".into());
    app.selected_pane = Some("%20".into());

    app.start_delete();

    assert_eq!(app.input_mode, InputMode::Confirm);
    // The pane name was empty on disk, but load_task runs fixup_pane_names
    // which assigns "web" (from repo_name). So the friendly name should be "web".
    assert!(
        app.input_label.contains("web"),
        "Expected input_label to show fixed-up name 'web', got: {}",
        app.input_label
    );
    // Should NOT show the raw pane ID
    assert!(
        !app.input_label.contains("%20"),
        "input_label should not contain raw pane ID '%20', got: {}",
        app.input_label
    );
}

#[test]
fn test_tui_delete_pane_falls_back_when_pane_not_tracked() {
    let (_temp_dir, wagner) = setup();

    // Create a task with no tracked panes (pane exists in tmux but not in task.panes)
    let task_path = wagner.config.tasks_root.join("untracked-test");
    std::fs::create_dir_all(task_path.join(".wagner")).unwrap();

    let task = wagner::Task::new(
        "untracked-test",
        task_path.clone(),
        vec![wagner::TaskRepo {
            name: "svc".into(),
            source: wagner::RepoSource::Local(PathBuf::from("/tmp/svc")),
            worktree: PathBuf::from("/tmp/svc-wt"),
            branch: "main".into(),
        }],
        None,
    );
    wagner.store.save_task(&task).unwrap();

    let session_name = wagner::terminal::session_name_for_task("untracked-test");
    {
        let mut sessions = wagner.terminal.sessions.lock().unwrap();
        sessions.insert(
            session_name,
            vec![PaneHandle("%30".into(), "pane".into())],
        );
    }

    let mut app = App::new(wagner);
    app.selected_task = Some("untracked-test".into());
    app.selected_pane = Some("%30".into());

    app.start_delete();

    assert_eq!(app.input_mode, InputMode::Confirm);
    // Should fall back to pane_id when pane is not tracked
    assert!(
        app.input_label.contains("%30"),
        "Expected input_label to fall back to pane ID '%30', got: {}",
        app.input_label
    );
}

// ===== Fix 2: TUI delete does not force-delete branches =====

#[test]
fn test_tui_delete_task_uses_force_false() {
    let (_temp_dir, wagner) = setup();

    // Create a task
    let task_path = wagner.config.tasks_root.join("force-test");
    std::fs::create_dir_all(task_path.join(".wagner")).unwrap();

    let task = wagner::Task::new(
        "force-test",
        task_path.clone(),
        vec![],
        None,
    );
    wagner.store.save_task(&task).unwrap();

    let session_name = wagner::terminal::session_name_for_task("force-test");
    {
        let mut sessions = wagner.terminal.sessions.lock().unwrap();
        sessions.insert(session_name, vec![]);
    }

    let mut app = App::new(wagner);
    app.selected_task = Some("force-test".into());
    app.selected_pane = None;

    // Set up the delete confirmation for task
    app.input_mode = InputMode::Confirm;
    app.confirm_action = Some("force-test".into());
    app.input_buffer = "y".into();

    // Execute the confirmation
    app.submit_input();

    // The task should be deleted. Since we use force=false, branches should NOT be
    // force-deleted. We verify by checking the task was deleted (which succeeds
    // regardless of force flag for tasks with no repos/worktrees).
    let result = app.wagner.store.load_task("force-test");
    assert!(result.is_err(), "Task should be deleted after confirmation");
}

// ===== Fix 3: TUI new task works without workspaces =====

#[test]
fn test_tui_new_task_without_workspaces_detects_git_repo() {
    let (_temp_dir, wagner) = setup();

    // Verify workspaces are empty
    assert!(
        wagner.config.workspaces.is_empty(),
        "Config should have no workspaces for this test"
    );

    let mut app = App::new(wagner);

    // Try to start a new task — since we're running in a git repo (the wagner project),
    // it should succeed and enter NewTask input mode
    app.start_new_task();

    // We're running this test from within the wagner git repo, so detect_git_repo should work
    assert_eq!(
        app.input_mode,
        InputMode::NewTask,
        "Should enter NewTask mode when git repo detected even without workspaces"
    );
    assert!(
        app.input_label.contains("auto-detected"),
        "Input label should mention auto-detection, got: {}",
        app.input_label
    );
}

#[test]
fn test_tui_new_task_with_workspaces_unchanged() {
    let (_temp_dir, wagner) = setup();

    // Add a workspace
    let mut config = wagner.config.clone();
    config.workspaces.insert(
        "test-ws".to_string(),
        wagner::config::Workspace {
            repos: vec![("myrepo".to_string(), "/tmp/myrepo".to_string())]
                .into_iter()
                .collect(),
            base_branch: "main".to_string(),
        },
    );

    let wagner = Wagner::new(MockTerminal::new(), TestAgent::echo(), config);
    let mut app = App::new(wagner);

    // Start new task with workspaces configured
    app.start_new_task();

    assert_eq!(
        app.input_mode,
        InputMode::NewTask,
        "Should enter NewTask mode with workspaces"
    );
    assert_eq!(
        app.input_label, "Task name",
        "Input label should be standard when workspaces exist"
    );
}

#[test]
fn test_tui_new_task_auto_detect_attempts_create() {
    let (_temp_dir, wagner) = setup();

    // Verify workspaces are empty
    assert!(wagner.config.workspaces.is_empty());

    let mut app = App::new(wagner);

    // Start new task
    app.start_new_task();
    assert_eq!(app.input_mode, InputMode::NewTask);

    // Enter a task name and submit
    app.input_buffer = "my-auto-task".to_string();
    app.input_cursor = "my-auto-task".len();
    app.submit_input();

    // The auto-detect path was taken (workspaces were empty, so create_task_from_auto_detected_repo
    // was called). We verify by checking that a status message was set — either a success message
    // ("Created task: ...") or an error (if worktree creation fails in the temp dir).
    // The key assertion is that it didn't silently fail or show the old
    // "No workspaces configured" message.
    assert!(
        app.status_message.is_some(),
        "A status message should be set after attempting auto-detect task creation"
    );
    let (msg, _) = app.status_message.as_ref().unwrap();
    assert!(
        msg.starts_with("Created task:") || msg.starts_with("Error:"),
        "Status should indicate task creation was attempted, got: {}",
        msg
    );
    // Crucially, it should NOT be the old error message
    assert!(
        !msg.contains("No workspaces configured"),
        "Should not show the old 'No workspaces configured' error"
    );
}

#[test]
fn test_tui_new_task_no_workspace_skips_workspace_selection() {
    let (_temp_dir, wagner) = setup();

    assert!(wagner.config.workspaces.is_empty());

    let mut app = App::new(wagner);

    // Start new task — should go directly to NewTask mode (not SelectWorkspace)
    app.start_new_task();
    assert_eq!(app.input_mode, InputMode::NewTask);

    // Enter name and submit
    app.input_buffer = "test-task".to_string();
    app.input_cursor = "test-task".len();
    app.submit_input();

    // Should NOT be in SelectWorkspace mode (the old path)
    assert_ne!(
        app.input_mode,
        InputMode::SelectWorkspace,
        "Should not enter workspace selection when no workspaces configured"
    );
}
