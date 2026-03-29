use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;
use wagner::{Config, MockTerminal, Store, TestAgent, Wagner};

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
}

// =============================================================================
// Fix 2: ensure_gitignore modifies worktree, not source repo
// =============================================================================

#[test]
fn test_gitignore_modifies_worktree() {
    let ctx = TestContext::new();
    let wagner = ctx.wagner();

    // Create a task with a worktree that differs from the source repo
    let specs = vec![wagner::RepoSpec::parse(
        &format!("myrepo:{}:feature/test-gitignore", ctx.repo_path.display()),
        None,
    )
    .unwrap()];

    let task = wagner.create_task("test-gitignore", &specs, None).unwrap();

    let worktree_path = &task.repos[0].worktree;
    let source_path = &ctx.repo_path;

    // The worktree's .gitignore should contain .wagner/
    let worktree_gitignore = worktree_path.join(".gitignore");
    if worktree_gitignore.exists() {
        let content = std::fs::read_to_string(&worktree_gitignore).unwrap();
        assert!(
            content.contains(".wagner/"),
            "Worktree .gitignore should contain .wagner/: {}",
            content
        );
    }

    // The source repo's .gitignore should NOT have been modified to include .wagner/
    let source_gitignore = source_path.join(".gitignore");
    if source_gitignore.exists() {
        let content = std::fs::read_to_string(&source_gitignore).unwrap();
        assert!(
            !content.contains(".wagner/"),
            "Source repo .gitignore should NOT contain .wagner/ after the fix: {}",
            content
        );
    }
}

// =============================================================================
// Fix 3: Store attached registry uses atomic writes (temp file + rename)
// =============================================================================

#[test]
fn test_registry_atomic_write() {
    let temp_dir = TempDir::new().unwrap();
    let tasks_root = temp_dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_root).unwrap();

    let config = Config {
        tasks_root: tasks_root.clone(),
        ..Config::default()
    };
    let store = Store::new(config);

    // Register an attached task
    let task_path = temp_dir.path().join("my-project");
    std::fs::create_dir_all(&task_path).unwrap();
    store
        .register_attached("test-task", &task_path)
        .expect("register should succeed");

    // Verify the registry file exists (not a .tmp file)
    let registry_path = tasks_root.join(".attached_registry.json");
    assert!(registry_path.exists(), "Registry file should exist");

    // Verify no leftover tmp file
    let tmp_path = tasks_root.join(".attached_registry.json.tmp");
    assert!(
        !tmp_path.exists(),
        "Temp file should be cleaned up after atomic rename"
    );

    // Verify content is valid JSON with our entry
    let content = std::fs::read_to_string(&registry_path).unwrap();
    let registry: HashMap<String, PathBuf> = serde_json::from_str(&content).unwrap();
    assert_eq!(registry.get("test-task"), Some(&task_path));
}

#[test]
fn test_registry_concurrent_access() {
    // Verify that multiple register/unregister operations don't corrupt the file.
    let temp_dir = TempDir::new().unwrap();
    let tasks_root = temp_dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_root).unwrap();

    let config = Config {
        tasks_root: tasks_root.clone(),
        ..Config::default()
    };
    let store = Store::new(config);

    // Register multiple tasks sequentially
    for i in 0..10 {
        let task_path = temp_dir.path().join(format!("project-{}", i));
        std::fs::create_dir_all(&task_path).unwrap();
        store
            .register_attached(&format!("task-{}", i), &task_path)
            .expect("register should succeed");
    }

    // Unregister some
    for i in (0..10).step_by(2) {
        store
            .unregister_attached(&format!("task-{}", i))
            .expect("unregister should succeed");
    }

    // Verify final state
    let registry_path = tasks_root.join(".attached_registry.json");
    let content = std::fs::read_to_string(&registry_path).unwrap();
    let registry: HashMap<String, PathBuf> = serde_json::from_str(&content).unwrap();

    // Only odd-numbered tasks should remain
    assert_eq!(registry.len(), 5);
    for i in (1..10).step_by(2) {
        assert!(
            registry.contains_key(&format!("task-{}", i)),
            "task-{} should be in registry",
            i
        );
    }
    for i in (0..10).step_by(2) {
        assert!(
            !registry.contains_key(&format!("task-{}", i)),
            "task-{} should NOT be in registry",
            i
        );
    }

    // No tmp file left
    let tmp_path = tasks_root.join(".attached_registry.json.tmp");
    assert!(!tmp_path.exists(), "No leftover temp file");
}

// =============================================================================
// Fix 4: home_dir() never returns "."
// =============================================================================

#[test]
fn test_home_dir_no_dot_fallback() {
    // When HOME is unset, Config::default() should not use "." as the home directory.
    // The tasks_root and repos_root should be absolute paths, never relative ".".

    // Temporarily unset HOME for this test
    let original_home = std::env::var("HOME").ok();
    unsafe { std::env::remove_var("HOME") };

    let config = Config::default();

    // Restore HOME
    if let Some(home) = original_home {
        unsafe { std::env::set_var("HOME", home) };
    }

    // tasks_root should not start with "." — it should be an absolute fallback
    assert!(
        config.tasks_root.is_absolute(),
        "tasks_root should be absolute when HOME is unset, got: {:?}",
        config.tasks_root
    );

    // repos_root should not start with "." — it should be an absolute fallback
    assert!(
        config.repos_root.is_absolute(),
        "repos_root should be absolute when HOME is unset, got: {:?}",
        config.repos_root
    );

    // Neither path should contain "." as a component (which would indicate the old fallback)
    let tasks_str = config.tasks_root.to_string_lossy();
    assert!(
        !tasks_str.starts_with("./") && tasks_str != ".",
        "tasks_root should not be relative '.': {:?}",
        config.tasks_root
    );
}

#[test]
fn test_home_dir_with_home_set() {
    // When HOME is set, everything works normally
    let config = Config::default();

    // With HOME set, tasks_root and repos_root should be absolute
    assert!(
        config.tasks_root.is_absolute(),
        "tasks_root should be absolute: {:?}",
        config.tasks_root
    );
    assert!(
        config.repos_root.is_absolute(),
        "repos_root should be absolute: {:?}",
        config.repos_root
    );
}

#[test]
fn test_config_dir_no_dot_fallback() {
    // Config::config_dir() should also never use "." as fallback
    let original_home = std::env::var("HOME").ok();
    let original_xdg = std::env::var("XDG_CONFIG_HOME").ok();

    unsafe {
        std::env::remove_var("HOME");
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let config_dir = Config::config_dir();

    // Restore env
    if let Some(home) = original_home {
        unsafe { std::env::set_var("HOME", home) };
    }
    if let Some(xdg) = original_xdg {
        unsafe { std::env::set_var("XDG_CONFIG_HOME", xdg) };
    }

    assert!(
        config_dir.is_absolute(),
        "config_dir should be absolute when HOME is unset, got: {:?}",
        config_dir
    );
}
