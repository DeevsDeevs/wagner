use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use wagner::config::MonitorConfig;
use wagner::core::status_engine::StatusEngine;
use wagner::model::{RepoSource, TaskKind, TaskRepo, TrackedPane};
use wagner::monitor::status::SessionAggregateStatus;
use wagner::{Config, Engine, MockTerminal, PaneHandle, Task};

// =============================================================================
// detect_task_for_cwd tests
// =============================================================================

#[test]
fn test_detect_task_attached_registry() {
    // An attached task whose path is outside tasks_root should be found
    // by detect_task_for_cwd when cwd is inside the attached path.
    let temp_dir = TempDir::new().unwrap();
    let tasks_root = temp_dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_root).unwrap();

    // Create an "attached" project directory outside tasks_root
    let project_dir = temp_dir.path().join("my-project");
    std::fs::create_dir_all(project_dir.join(".wagner")).unwrap();

    // Write a minimal task.json
    let task = Task {
        name: "my-attached-task".into(),
        path: project_dir.clone(),
        repos: vec![TaskRepo {
            name: "repo1".into(),
            source: RepoSource::Local(project_dir.clone()),
            worktree: project_dir.clone(),
            branch: "main".into(),
        }],
        created_at: chrono::Utc::now(),
        diff_base: None,
        kind: TaskKind::Attached,
        panes: vec![],
    };
    let task_json = serde_json::to_string_pretty(&task).unwrap();
    std::fs::write(project_dir.join(".wagner").join("task.json"), &task_json).unwrap();

    // Write attached registry
    let mut registry = HashMap::new();
    registry.insert("my-attached-task".to_string(), project_dir.clone());
    let registry_json = serde_json::to_string_pretty(&registry).unwrap();
    std::fs::write(tasks_root.join(".attached_registry.json"), &registry_json).unwrap();

    let config = Config {
        tasks_root: tasks_root.clone(),
        ..Config::default()
    };

    // cwd is the project root itself
    let result = wagner::detect_task_for_cwd(&project_dir, &config);
    assert_eq!(
        result,
        Some("my-attached-task".to_string()),
        "Should find attached task when cwd is the project root"
    );

    // cwd is a subdirectory inside the attached task path
    let sub_dir = project_dir.join("src").join("lib");
    std::fs::create_dir_all(&sub_dir).unwrap();
    let result = wagner::detect_task_for_cwd(&sub_dir, &config);
    assert_eq!(
        result,
        Some("my-attached-task".to_string()),
        "Should find attached task when cwd is a subdirectory"
    );
}

#[test]
fn test_detect_task_managed_still_works() {
    // Managed tasks under tasks_root should continue to be found.
    let temp_dir = TempDir::new().unwrap();
    let tasks_root = temp_dir.path().join("tasks");
    let task_dir = tasks_root.join("my-managed-task");
    std::fs::create_dir_all(task_dir.join(".wagner")).unwrap();

    let task = Task {
        name: "my-managed-task".into(),
        path: task_dir.clone(),
        repos: vec![],
        created_at: chrono::Utc::now(),
        diff_base: None,
        kind: TaskKind::Managed,
        panes: vec![],
    };
    let task_json = serde_json::to_string_pretty(&task).unwrap();
    std::fs::write(task_dir.join(".wagner").join("task.json"), &task_json).unwrap();

    let config = Config {
        tasks_root: tasks_root.clone(),
        ..Config::default()
    };

    let result = wagner::detect_task_for_cwd(&task_dir, &config);
    assert_eq!(result, Some("my-managed-task".to_string()));

    // Subdirectory inside the task
    let sub = task_dir.join("src");
    std::fs::create_dir_all(&sub).unwrap();
    let result = wagner::detect_task_for_cwd(&sub, &config);
    assert_eq!(result, Some("my-managed-task".to_string()));
}

#[test]
fn test_detect_task_unrelated_cwd_returns_none() {
    let temp_dir = TempDir::new().unwrap();
    let tasks_root = temp_dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_root).unwrap();

    let config = Config {
        tasks_root: tasks_root.clone(),
        ..Config::default()
    };

    // cwd is completely outside tasks_root with no attached tasks
    let unrelated = temp_dir.path().join("random");
    std::fs::create_dir_all(&unrelated).unwrap();
    let result = wagner::detect_task_for_cwd(&unrelated, &config);
    assert_eq!(result, None);
}

#[test]
fn test_detect_task_attached_no_registry_returns_none() {
    // When there's no attached registry, only tasks_root is checked.
    let temp_dir = TempDir::new().unwrap();
    let tasks_root = temp_dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_root).unwrap();

    let config = Config {
        tasks_root: tasks_root.clone(),
        ..Config::default()
    };

    let project_dir = temp_dir.path().join("my-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let result = wagner::detect_task_for_cwd(&project_dir, &config);
    assert_eq!(result, None);
}

#[test]
fn test_detect_task_attached_stale_registry_entry() {
    // If the registry points to a path that no longer has task.json, return None.
    let temp_dir = TempDir::new().unwrap();
    let tasks_root = temp_dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_root).unwrap();

    let project_dir = temp_dir.path().join("my-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    // No .wagner/task.json here!

    let mut registry = HashMap::new();
    registry.insert("stale-task".to_string(), project_dir.clone());
    let registry_json = serde_json::to_string_pretty(&registry).unwrap();
    std::fs::write(tasks_root.join(".attached_registry.json"), &registry_json).unwrap();

    let config = Config {
        tasks_root: tasks_root.clone(),
        ..Config::default()
    };

    let result = wagner::detect_task_for_cwd(&project_dir, &config);
    assert_eq!(result, None, "Stale registry entry should not match");
}

// =============================================================================
// NeedsAttention debounce tests
// =============================================================================

fn make_test_task(pane_id: &str, jsonl_path: PathBuf) -> Task {
    Task {
        name: "debounce-test".into(),
        path: PathBuf::from("/tmp/debounce-test"),
        repos: vec![TaskRepo {
            name: "repo1".into(),
            source: RepoSource::Local(PathBuf::from("/tmp/repo1")),
            worktree: PathBuf::from("/tmp/debounce-test/repo1"),
            branch: "main".into(),
        }],
        created_at: chrono::Utc::now(),
        diff_base: None,
        kind: TaskKind::Managed,
        panes: vec![TrackedPane {
            name: "claude-repo1".into(),
            repo_name: "repo1".into(),
            engine: Engine::ClaudeCode,
            session_id: "sess-1".into(),
            pane_id: pane_id.into(),
            jsonl_path,
            launched_at: chrono::Utc::now(),
        }],
    }
}

#[test]
fn test_needs_attention_not_suppressed_by_debounce() {
    // NeedsAttention session transitions should use a shorter debounce
    // (100ms) instead of the normal 1-second debounce, ensuring the
    // notification is emitted promptly.

    let config = MonitorConfig {
        approval_timeout_ms: 10,
        idle_threshold_ms: 5000,
        max_lines_per_poll: 1000,
        ..Default::default()
    };

    let mut engine = StatusEngine::new_for_test(&config);

    // Create a JSONL file with a tool_use (triggers approval waiting after timeout)
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{}}}}]}}}}"#
    )
    .unwrap();
    file.flush().unwrap();

    let task = make_test_task("%50", file.path().to_path_buf());
    let session_name = wagner::terminal::session_name_for_task(&task.name);

    // Set up MockTerminal with the session and pane
    let terminal = MockTerminal::new();
    terminal.sessions.lock().unwrap().insert(
        session_name.clone(),
        vec![PaneHandle("%50".into(), "claude-repo1".into())],
    );

    engine.track_task(&task, &session_name);

    // First poll — reads JSONL, pane becomes Active (tool proposed)
    let _events = engine.poll_transitions(&terminal, &[task.clone()]);

    // Wait for approval timeout so pane transitions to Waiting
    std::thread::sleep(Duration::from_millis(20));

    // Second poll — pane is now Waiting, session becomes NeedsAttention
    // This call starts the debounce timer for the session status change
    let events = engine.poll_transitions(&terminal, &[task.clone()]);
    // The pane-level NeedsAttention event may be emitted here (no debounce on pane events)
    let pane_needs_attention = events
        .iter()
        .any(|e| matches!(e, wagner::transport::CoreEvent::NeedsAttention { .. }));
    assert!(
        pane_needs_attention,
        "Pane-level NeedsAttention should be emitted immediately"
    );

    // Wait just over the NeedsAttention debounce (100ms) but well under
    // the normal session debounce (1s)
    std::thread::sleep(Duration::from_millis(150));

    // Third poll — should emit SessionStatusChanged with NeedsAttention
    let events = engine.poll_transitions(&terminal, &[task.clone()]);
    let session_changed = events.iter().any(|e| {
        matches!(
            e,
            wagner::transport::CoreEvent::SessionStatusChanged {
                status: SessionAggregateStatus::NeedsAttention,
                ..
            }
        )
    });
    assert!(
        session_changed,
        "SessionStatusChanged(NeedsAttention) should be emitted after 150ms (within 100ms debounce), \
         not delayed by the normal 1-second debounce. Events: {:?}",
        events
    );
}

#[test]
fn test_working_status_uses_normal_debounce() {
    // Verify that non-NeedsAttention transitions still use the 1-second debounce.
    // A Working status change should NOT emit within 200ms.

    let config = MonitorConfig {
        approval_timeout_ms: 5000,
        idle_threshold_ms: 5000,
        max_lines_per_poll: 1000,
        ..Default::default()
    };

    let mut engine = StatusEngine::new_for_test(&config);

    // Create a JSONL file that makes the pane Active (Working session status)
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"{{"type":"user","message":{{"role":"user","content":"do stuff"}}}}"#
    )
    .unwrap();
    file.flush().unwrap();

    let task = make_test_task("%51", file.path().to_path_buf());
    let session_name = wagner::terminal::session_name_for_task(&task.name);

    let terminal = MockTerminal::new();
    terminal.sessions.lock().unwrap().insert(
        session_name.clone(),
        vec![PaneHandle("%51".into(), "claude-repo1".into())],
    );

    engine.track_task(&task, &session_name);

    // First poll — reads JSONL, pane becomes Active
    let _events = engine.poll_transitions(&terminal, &[task.clone()]);

    // Second poll — starts debounce timer for Working session status
    let _events = engine.poll_transitions(&terminal, &[task.clone()]);

    // Wait 200ms (more than NeedsAttention debounce, less than normal debounce)
    std::thread::sleep(Duration::from_millis(200));

    // Third poll — should NOT emit SessionStatusChanged yet (needs 1s)
    let events = engine.poll_transitions(&terminal, &[task.clone()]);
    let session_changed = events.iter().any(|e| {
        matches!(
            e,
            wagner::transport::CoreEvent::SessionStatusChanged {
                status: SessionAggregateStatus::Working,
                ..
            }
        )
    });
    assert!(
        !session_changed,
        "Working status should NOT emit within 200ms (uses 1-second debounce). Events: {:?}",
        events
    );
}
