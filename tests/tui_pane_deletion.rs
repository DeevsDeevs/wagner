use std::path::PathBuf;
use tempfile::TempDir;
use wagner::model::TrackedPane;
use wagner::tui::{App, InputMode};
use wagner::{Config, Engine, MockTerminal, PaneHandle, Store, TestAgent, Wagner};

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

#[test]
fn test_tui_delete_pane_updates_tracking() {
    let (_temp_dir, wagner) = setup();

    // Create a task directory and save a task with two tracked panes
    let task_path = wagner.config.tasks_root.join("test-task");
    std::fs::create_dir_all(task_path.join(".wagner")).unwrap();

    let mut task = wagner::Task::new(
        "test-task",
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
        .push(make_tracked_pane("claude-api", "%1", "api"));
    task.panes
        .push(make_tracked_pane("claude-api-2", "%2", "api"));
    wagner.store.save_task(&task).unwrap();

    // Verify task initially has 2 panes
    let loaded = wagner.store.load_task("test-task").unwrap();
    assert_eq!(loaded.panes.len(), 2);

    // Set up mock terminal with matching panes in the session
    let session_name = wagner::terminal::session_name_for_task("test-task");
    {
        let mut sessions = wagner.terminal.sessions.lock().unwrap();
        sessions.insert(
            session_name.clone(),
            vec![
                PaneHandle("%1".into(), "claude-api".into()),
                PaneHandle("%2".into(), "claude-api-2".into()),
            ],
        );
    }

    // Build TUI App and select the task + pane
    let mut app = App::new(wagner);
    app.selected_task = Some("test-task".into());
    app.selected_pane = Some("%1".into());

    // Simulate confirming pane deletion:
    // Set input_mode to Confirm, confirm_action to the pane, input_buffer to "y"
    app.input_mode = InputMode::Confirm;
    app.confirm_action = Some("delete_pane:%1".into());
    app.input_buffer = "y".into();

    // Execute the confirmation (calls confirm_delete internally)
    app.submit_input();

    // Verify: reload task from store and check pane was removed
    let updated_task = app.wagner.store.load_task("test-task").unwrap();
    assert_eq!(
        updated_task.panes.len(),
        1,
        "Expected 1 pane remaining after deletion, got {}",
        updated_task.panes.len()
    );
    assert_eq!(
        updated_task.panes[0].pane_id, "%2",
        "Expected pane %2 to remain, but found {}",
        updated_task.panes[0].pane_id
    );
    assert_eq!(updated_task.panes[0].name, "claude-api-2");

    // Verify selected_pane was reset (either None or reselected by refresh_data)
    // The key invariant is that the deleted pane is NOT the selected pane
    if let Some(ref selected) = app.selected_pane {
        assert_ne!(selected, "%1", "Deleted pane should not remain selected");
    }
}

#[test]
fn test_tui_delete_pane_persisted_to_disk() {
    let (_temp_dir, wagner) = setup();

    // Create a task with one pane
    let task_path = wagner.config.tasks_root.join("persist-test");
    std::fs::create_dir_all(task_path.join(".wagner")).unwrap();

    let mut task = wagner::Task::new(
        "persist-test",
        task_path.clone(),
        vec![wagner::TaskRepo {
            name: "web".into(),
            source: wagner::RepoSource::Local(PathBuf::from("/tmp/web")),
            worktree: PathBuf::from("/tmp/web-wt"),
            branch: "main".into(),
        }],
        None,
    );
    task.panes.push(make_tracked_pane("droid-web", "%5", "web"));
    wagner.store.save_task(&task).unwrap();

    // Set up mock terminal
    let session_name = wagner::terminal::session_name_for_task("persist-test");
    {
        let mut sessions = wagner.terminal.sessions.lock().unwrap();
        sessions.insert(
            session_name,
            vec![PaneHandle("%5".into(), "droid-web".into())],
        );
    }

    let store_config = wagner.config.clone();

    let mut app = App::new(wagner);
    app.selected_task = Some("persist-test".into());
    app.selected_pane = Some("%5".into());
    app.input_mode = InputMode::Confirm;
    app.confirm_action = Some("delete_pane:%5".into());
    app.input_buffer = "y".into();
    app.submit_input();

    // Verify persistence: create a fresh store and read from disk
    let fresh_store = Store::new(store_config);
    let reloaded_task = fresh_store.load_task("persist-test").unwrap();
    assert!(
        reloaded_task.panes.is_empty(),
        "Expected no panes after deletion, got {} panes",
        reloaded_task.panes.len()
    );
}
