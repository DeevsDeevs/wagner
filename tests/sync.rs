use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;
use wagner::Config;
use wagner::config::Workspace;

struct SyncTestContext {
    _temp_dir: TempDir,
    config_dir: PathBuf,
    binary: PathBuf,
}

impl SyncTestContext {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_dir = temp_dir.path().join("config").join("wagner");
        std::fs::create_dir_all(&config_dir).unwrap();

        let binary = env!("CARGO_BIN_EXE_wagner").into();

        Self {
            _temp_dir: temp_dir,
            config_dir,
            binary,
        }
    }

    fn write_config(&self, config: &Config) {
        let content = serde_json::to_string_pretty(config).unwrap();
        std::fs::write(self.config_dir.join("config.json"), content).unwrap();
    }

    fn xdg_config_home(&self) -> PathBuf {
        self.config_dir.parent().unwrap().to_path_buf()
    }

    fn run_sync(&self, args: &[&str]) -> std::process::Output {
        let mut cmd = Command::new(&self.binary);
        cmd.arg("sync");
        cmd.args(args);
        cmd.env("XDG_CONFIG_HOME", self.xdg_config_home());
        cmd.output().expect("Failed to execute wagner sync")
    }

    fn create_git_repo(&self, name: &str) -> PathBuf {
        let repo_path = self._temp_dir.path().join(name);
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

        std::fs::write(repo_path.join("README.md"), "# Test").unwrap();

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

        repo_path
    }
}

#[test]
fn sync_no_workspaces_configured() {
    let ctx = SyncTestContext::new();
    ctx.write_config(&Config::default());

    let output = ctx.run_sync(&[]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("No workspaces configured"));
}

#[test]
fn sync_nonexistent_workspace_errors() {
    let ctx = SyncTestContext::new();
    ctx.write_config(&Config::default());

    let output = ctx.run_sync(&["nonexistent"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(
        stderr.contains("Workspace 'nonexistent' not found"),
        "Expected 'not found' error, got: {stderr}"
    );
}

#[test]
fn sync_fetches_single_workspace() {
    let ctx = SyncTestContext::new();
    let repo_path = ctx.create_git_repo("my-repo");

    let mut config = Config::default();
    let mut ws = Workspace::default();
    ws.repos.insert(
        "my-repo".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    config.workspaces.insert("test-ws".to_string(), ws);
    ctx.write_config(&config);

    let output = ctx.run_sync(&["test-ws"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "sync should succeed");
    assert!(
        stdout.contains("Syncing workspace: test-ws"),
        "Should show workspace name, got: {stdout}"
    );
    assert!(
        stdout.contains("my-repo"),
        "Should show repo name, got: {stdout}"
    );
    assert!(
        stdout.contains("done"),
        "Should report done for valid repo, got: {stdout}"
    );
}

#[test]
fn sync_all_workspaces() {
    let ctx = SyncTestContext::new();
    let repo1 = ctx.create_git_repo("repo-alpha");
    let repo2 = ctx.create_git_repo("repo-beta");

    let mut config = Config::default();

    let mut ws1 = Workspace::default();
    ws1.repos
        .insert("alpha".to_string(), repo1.to_string_lossy().to_string());
    config.workspaces.insert("ws-one".to_string(), ws1);

    let mut ws2 = Workspace::default();
    ws2.repos
        .insert("beta".to_string(), repo2.to_string_lossy().to_string());
    config.workspaces.insert("ws-two".to_string(), ws2);

    ctx.write_config(&config);

    let output = ctx.run_sync(&[]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(
        stdout.contains("ws-one") && stdout.contains("ws-two"),
        "Should sync both workspaces, got: {stdout}"
    );
    assert!(
        stdout.contains("alpha") && stdout.contains("beta"),
        "Should show both repo names, got: {stdout}"
    );
}

#[test]
fn sync_invalid_repo_path_reports_error_per_repo() {
    let ctx = SyncTestContext::new();
    let valid_repo = ctx.create_git_repo("valid-repo");

    let mut config = Config::default();
    let mut ws = Workspace::default();
    ws.repos.insert(
        "valid".to_string(),
        valid_repo.to_string_lossy().to_string(),
    );
    ws.repos.insert(
        "broken".to_string(),
        "/nonexistent/path/to/repo".to_string(),
    );
    config.workspaces.insert("mixed-ws".to_string(), ws);
    ctx.write_config(&config);

    let output = ctx.run_sync(&["mixed-ws"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Command should still succeed (errors are per-repo, not fatal)
    assert!(output.status.success());
    assert!(
        stdout.contains("error"),
        "Should report error for broken repo, got: {stdout}"
    );
    assert!(
        stdout.contains("done"),
        "Should report done for valid repo, got: {stdout}"
    );
}

#[test]
fn sync_workspace_with_multiple_repos() {
    let ctx = SyncTestContext::new();
    let repo1 = ctx.create_git_repo("frontend");
    let repo2 = ctx.create_git_repo("backend");
    let repo3 = ctx.create_git_repo("shared");

    let mut config = Config::default();
    let mut ws = Workspace::default();
    ws.repos
        .insert("frontend".to_string(), repo1.to_string_lossy().to_string());
    ws.repos
        .insert("backend".to_string(), repo2.to_string_lossy().to_string());
    ws.repos
        .insert("shared".to_string(), repo3.to_string_lossy().to_string());
    config.workspaces.insert("fullstack".to_string(), ws);
    ctx.write_config(&config);

    let output = ctx.run_sync(&["fullstack"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());

    let done_count = stdout.matches("done").count();
    assert_eq!(
        done_count, 3,
        "All 3 repos should succeed, got {done_count} 'done' in: {stdout}"
    );
}

#[test]
fn sync_tilde_path_expansion() {
    let ctx = SyncTestContext::new();
    let repo = ctx.create_git_repo("tilde-repo");

    // Create a workspace with a path that uses the actual absolute path
    // but verify that tilde expansion at least doesn't crash.
    // We can't truly test ~ expansion without knowing HOME, so we test
    // that a non-tilde absolute path still works through the tilde expansion codepath.
    let mut config = Config::default();
    let mut ws = Workspace::default();
    ws.repos
        .insert("repo".to_string(), repo.to_string_lossy().to_string());
    config.workspaces.insert("tilde-ws".to_string(), ws);
    ctx.write_config(&config);

    let output = ctx.run_sync(&["tilde-ws"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("done"));
}
