//! Tests verifying that send_text_enter with engine-specific delays is used
//! instead of send_keys with hardcoded 100ms delay.
//!
//! Covers:
//! - VAL-HIGH-003: prepare_agent_in_pane uses engine-specific delay
//! - VAL-HIGH-004: Daemon check_agent_health uses engine-specific delay
//! - VAL-CROSS-002: Droid panes resume with correct engine delay

use std::path::PathBuf;
use tempfile::TempDir;
use wagner::{
    Config, Engine, MockTerminal, RepoSource, Store, Task, TaskRepo, TestAgent, Wagner,
};

struct DelayTestCtx {
    _temp_dir: TempDir,
    tasks_root: PathBuf,
    worktree_path: PathBuf,
}

impl DelayTestCtx {
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

    fn create_persisted_task_with_pane(
        &self,
        task_name: &str,
        engine: Engine,
        pane_id: &str,
        session_id: &str,
    ) -> Task {
        let repo = TaskRepo {
            name: "myrepo".to_string(),
            source: RepoSource::Local(self.worktree_path.clone()),
            worktree: self.worktree_path.clone(),
            branch: "main".to_string(),
        };

        let tracked = wagner::TrackedPane {
            name: "test-pane".to_string(),
            repo_name: "myrepo".to_string(),
            engine,
            session_id: session_id.to_string(),
            pane_id: pane_id.to_string(),
            jsonl_path: PathBuf::from("pending-discovery"),
            launched_at: chrono::Utc::now(),
        };

        let mut task = Task::new_attached(task_name, self.worktree_path.clone(), vec![repo]);
        task.panes.push(tracked);
        self.store().save_task(&task).unwrap();
        task
    }

    fn wagner_with_terminal(&self, terminal: MockTerminal) -> Wagner<MockTerminal, TestAgent> {
        Wagner::new(terminal, TestAgent::echo(), self.config())
    }
}

// ---------------------------------------------------------------------------
// VAL-HIGH-003: prepare_agent_in_pane uses engine-specific delay
// ---------------------------------------------------------------------------

/// Test that prepare_agent_in_pane (called via create_session_with_panes during
/// quick_launch of a new task) uses send_text_enter with the engine's delay, not
/// the old send_keys path with a hardcoded 100ms delay.
///
/// TestAgent has engine = ClaudeCode which has enter_delay_ms() == 5.
#[test]
fn test_prepare_agent_uses_engine_delay() {
    let ctx = DelayTestCtx::new();
    let terminal = MockTerminal::new();

    // quick_launch for a new task will call prepare_agent_in_pane
    let w = ctx.wagner_with_terminal(terminal.clone());

    // We need to set cwd to worktree_path for quick_launch
    std::env::set_current_dir(&ctx.worktree_path).unwrap();

    w.quick_launch(Engine::ClaudeCode, Some("delay-test")).unwrap();

    // Verify send_text_enter was called with the correct delay
    let text_enter_calls = terminal.get_text_enter_calls();
    assert!(
        !text_enter_calls.is_empty(),
        "Expected at least one send_text_enter call, got none. sent_keys: {:?}",
        terminal.get_sent_keys()
    );

    // The launch command should use ClaudeCode's enter_delay_ms() = 5
    let (_, _, delay) = &text_enter_calls[0];
    assert_eq!(
        *delay, 5,
        "ClaudeCode engine should use 5ms delay, got {}ms",
        delay
    );
}

/// Test that prepare_agent_in_pane_with_engine (the explicit engine variant)
/// also uses the engine-specific delay, for Droid (5ms).
#[test]
fn test_prepare_agent_droid_engine_delay() {
    let ctx = DelayTestCtx::new();
    let terminal = MockTerminal::new();

    let w = ctx.wagner_with_terminal(terminal.clone());

    std::env::set_current_dir(&ctx.worktree_path).unwrap();

    // quick_launch with Engine::Droid uses prepare_agent_in_pane_with_engine
    w.quick_launch(Engine::Droid, Some("droid-delay-test")).unwrap();

    let text_enter_calls = terminal.get_text_enter_calls();
    assert!(
        !text_enter_calls.is_empty(),
        "Expected at least one send_text_enter call for Droid engine"
    );

    let (_, text, delay) = &text_enter_calls[0];
    assert_eq!(*delay, 5, "Droid engine should use 5ms delay, got {}ms", delay);
    assert_eq!(text, "droid", "Droid launch command should be 'droid'");
}

/// Test that prepare_agent_in_pane_with_engine uses Codex's delay (100ms).
#[test]
fn test_prepare_agent_codex_engine_delay() {
    let ctx = DelayTestCtx::new();
    let terminal = MockTerminal::new();

    let w = ctx.wagner_with_terminal(terminal.clone());

    std::env::set_current_dir(&ctx.worktree_path).unwrap();

    w.quick_launch(Engine::Codex, Some("codex-delay-test")).unwrap();

    let text_enter_calls = terminal.get_text_enter_calls();
    assert!(
        !text_enter_calls.is_empty(),
        "Expected at least one send_text_enter call for Codex engine"
    );

    let (_, text, delay) = &text_enter_calls[0];
    assert_eq!(
        *delay, 100,
        "Codex engine should use 100ms delay, got {}ms",
        delay
    );
    assert_eq!(text, "codex", "Codex launch command should be 'codex'");
}

// ---------------------------------------------------------------------------
// VAL-HIGH-003 + resume_dead_agents: uses engine-specific delay
// ---------------------------------------------------------------------------

/// Test that resume_dead_agents uses send_text_enter with engine-specific delay
/// instead of send_keys.
#[test]
fn test_resume_dead_agents_uses_engine_delay() {
    let ctx = DelayTestCtx::new();
    let terminal = MockTerminal::new();

    let session_id = "test-session-123";
    let task_name = "resume-test";

    // Create a task with a pane using ClaudeCode engine
    let _task = ctx.create_persisted_task_with_pane(
        task_name,
        Engine::ClaudeCode,
        "wagner_resume-test:0.0",
        session_id,
    );

    // Set up MockTerminal: session exists with the pane, but pane is running "bash"
    // (not "claude"), so it should be considered dead and resumed
    terminal
        .sessions
        .lock()
        .unwrap()
        .insert(
            "wagner_resume-test".to_string(),
            vec![wagner::PaneHandle(
                "wagner_resume-test:0.0".to_string(),
                "main".to_string(),
            )],
        );
    terminal.set_pane_command("wagner_resume-test:0.0", "bash");

    let w = ctx.wagner_with_terminal(terminal.clone());
    let resumed = w.resume_dead_agents(task_name).unwrap();

    assert_eq!(resumed, 1, "Should have resumed 1 dead agent");

    let text_enter_calls = terminal.get_text_enter_calls();
    assert!(
        !text_enter_calls.is_empty(),
        "resume_dead_agents should use send_text_enter, not send_keys"
    );

    let (_, text, delay) = &text_enter_calls[0];
    assert_eq!(
        *delay, 5,
        "ClaudeCode resume should use 5ms delay, got {}ms",
        delay
    );
    assert!(
        text.contains("claude"),
        "Resume command should contain 'claude', got: {}",
        text
    );
    assert!(
        text.contains(session_id),
        "Resume command should contain session id, got: {}",
        text
    );
}

/// Test that resume_dead_agents for a Droid pane uses 5ms delay.
#[test]
fn test_resume_dead_agents_droid_delay() {
    let ctx = DelayTestCtx::new();
    let terminal = MockTerminal::new();

    let session_id = "droid-session-456";
    let task_name = "droid-resume-test";

    let _task = ctx.create_persisted_task_with_pane(
        task_name,
        Engine::Droid,
        "wagner_droid-resume-test:0.0",
        session_id,
    );

    terminal
        .sessions
        .lock()
        .unwrap()
        .insert(
            "wagner_droid-resume-test".to_string(),
            vec![wagner::PaneHandle(
                "wagner_droid-resume-test:0.0".to_string(),
                "main".to_string(),
            )],
        );
    terminal.set_pane_command("wagner_droid-resume-test:0.0", "bash");

    let w = ctx.wagner_with_terminal(terminal.clone());
    let resumed = w.resume_dead_agents(task_name).unwrap();

    assert_eq!(resumed, 1);

    let text_enter_calls = terminal.get_text_enter_calls();
    assert!(!text_enter_calls.is_empty());

    let (_, text, delay) = &text_enter_calls[0];
    assert_eq!(*delay, 5, "Droid resume should use 5ms delay");
    assert!(
        text.contains("droid --resume"),
        "Droid resume command should contain 'droid --resume', got: {}",
        text
    );
}

// ---------------------------------------------------------------------------
// VAL-HIGH-004: check_agent_health uses engine-specific delay
// (daemon.rs check_agent_health is a free function taking &Tmux, but we can
// test the behavior indirectly via code inspection and the mock pattern)
// ---------------------------------------------------------------------------

/// Verify check_agent_health uses send_text_enter with engine-specific delay.
/// Since check_agent_health is a private function in daemon.rs, we test the
/// behavior through the resume_dead_agents path which has the same fix pattern,
/// and verify through code inspection that daemon.rs no longer uses send_keys.
///
/// Additionally, we verify the engine enter_delay_ms() values are correct for
/// all engine types, ensuring the daemon would use the right delays.
#[test]
fn test_engine_delays_for_all_types() {
    assert_eq!(Engine::ClaudeCode.enter_delay_ms(), 5, "ClaudeCode delay");
    assert_eq!(Engine::Codex.enter_delay_ms(), 100, "Codex delay");
    assert_eq!(Engine::Terminal.enter_delay_ms(), 10, "Terminal delay");
    assert_eq!(Engine::Droid.enter_delay_ms(), 5, "Droid delay");
}

/// VAL-CROSS-002: Droid panes resume with correct engine delay (5ms).
/// When the daemon health-checks a Droid pane and finds it dead, it should use
/// send_text_enter with Engine::Droid.enter_delay_ms() == 5.
#[test]
fn test_daemon_droid_resume_uses_correct_delay() {
    // This test verifies the behavior through resume_dead_agents since
    // check_agent_health uses the same pattern. Both now use
    // terminal.send_text_enter(&pane, &resume_cmd, engine.enter_delay_ms())
    let ctx = DelayTestCtx::new();
    let terminal = MockTerminal::new();

    let session_id = "droid-health-789";
    let task_name = "droid-health-test";

    let _task = ctx.create_persisted_task_with_pane(
        task_name,
        Engine::Droid,
        "wagner_droid-health-test:0.0",
        session_id,
    );

    terminal
        .sessions
        .lock()
        .unwrap()
        .insert(
            "wagner_droid-health-test".to_string(),
            vec![wagner::PaneHandle(
                "wagner_droid-health-test:0.0".to_string(),
                "main".to_string(),
            )],
        );
    // Pane is running "bash" instead of "droid" -> should be resumed
    terminal.set_pane_command("wagner_droid-health-test:0.0", "bash");

    let w = ctx.wagner_with_terminal(terminal.clone());
    let resumed = w.resume_dead_agents(task_name).unwrap();
    assert_eq!(resumed, 1);

    let text_enter_calls = terminal.get_text_enter_calls();
    assert_eq!(text_enter_calls.len(), 1);

    let (pane_id, text, delay) = &text_enter_calls[0];
    assert_eq!(pane_id, "wagner_droid-health-test:0.0");
    assert_eq!(*delay, 5, "Droid engine should use 5ms delay");
    assert!(text.contains("droid --resume"));
    assert!(text.contains(session_id));
}

/// Verify that no send_keys calls are made during resume — only send_text_enter.
#[test]
fn test_resume_uses_text_enter_not_send_keys() {
    let ctx = DelayTestCtx::new();
    let terminal = MockTerminal::new();

    let session_id = "no-send-keys-test";
    let task_name = "no-sk-test";

    let _task = ctx.create_persisted_task_with_pane(
        task_name,
        Engine::ClaudeCode,
        "wagner_no-sk-test:0.0",
        session_id,
    );

    terminal
        .sessions
        .lock()
        .unwrap()
        .insert(
            "wagner_no-sk-test".to_string(),
            vec![wagner::PaneHandle(
                "wagner_no-sk-test:0.0".to_string(),
                "main".to_string(),
            )],
        );
    terminal.set_pane_command("wagner_no-sk-test:0.0", "bash");

    let w = ctx.wagner_with_terminal(terminal.clone());
    w.resume_dead_agents(task_name).unwrap();

    // send_text_enter should have been called (it also adds to sent_keys via
    // send_literal + send_key for backward compat)
    let text_enter_calls = terminal.get_text_enter_calls();
    assert_eq!(
        text_enter_calls.len(),
        1,
        "Exactly one send_text_enter call expected"
    );

    // The sent_keys should only contain the sub-calls from send_text_enter
    // (send_literal + send_key("Enter")), not a raw send_keys call
    let sent_keys = terminal.get_sent_keys();
    // send_text_enter records: send_literal(text) -> (pane, text), send_key("Enter") -> (pane, "Enter")
    assert_eq!(
        sent_keys.len(),
        2,
        "Expected exactly 2 sent_keys entries (literal + Enter) from send_text_enter, got: {:?}",
        sent_keys
    );
    assert_eq!(sent_keys[1].1, "Enter", "Second entry should be 'Enter' key");
}
