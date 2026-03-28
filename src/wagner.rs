use crate::agent::Agent;
use crate::attach::get_current_branch;
use crate::config::Config;
use crate::error::{Result, WagnerError};
use crate::model::{RepoSource, Task, TaskRepo, TrackedPane};
use crate::plugins::builtin_plugins;
use crate::store::Store;
use crate::terminal::{PaneHandle, SessionHandle, Terminal, session_name_for_task};
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::debug;
use uuid::Uuid;

pub struct Wagner<T: Terminal, A: Agent> {
    pub terminal: T,
    pub agent: A,
    pub store: Store,
    pub config: Config,
}

impl<T: Terminal, A: Agent> Wagner<T, A> {
    pub fn new(terminal: T, agent: A, config: Config) -> Self {
        let store = Store::new(config.clone());
        Self {
            terminal,
            agent,
            store,
            config,
        }
    }

    pub fn create_task(
        &self,
        name: &str,
        repo_specs: &[RepoSpec],
        base_branch: Option<&str>,
    ) -> Result<Task> {
        if self.store.task_exists(name) {
            return Err(WagnerError::TaskExists(name.to_string()));
        }

        let task_path = self.config.tasks_root.join(name);
        std::fs::create_dir_all(&task_path)?;

        let mut repos = Vec::new();
        let mut created_worktrees: Vec<(PathBuf, PathBuf)> = Vec::new();

        let result = (|| -> Result<()> {
            for spec in repo_specs {
                let worktree_path = task_path.join(&spec.name);

                let main_repo = match &spec.source {
                    RepoSource::Local(source_path) => {
                        if !source_path.exists() {
                            return Err(WagnerError::RepoNotFound(
                                spec.name.clone(),
                                source_path.clone(),
                            ));
                        }

                        if let Some(base) = base_branch {
                            self.fetch_and_update_branch(source_path, base);
                        }

                        self.create_worktree(source_path, &worktree_path, &spec.branch)?;
                        source_path.clone()
                    }
                    RepoSource::Remote(url) => {
                        let clone_path = self.clone_repo(url)?;
                        self.create_worktree(&clone_path, &worktree_path, &spec.branch)?;
                        clone_path
                    }
                };

                created_worktrees.push((main_repo, worktree_path.clone()));

                repos.push(TaskRepo {
                    name: spec.name.clone(),
                    source: spec.source.clone(),
                    worktree: worktree_path,
                    branch: spec.branch.clone(),
                });
            }
            Ok(())
        })();

        if let Err(e) = result {
            self.cleanup_partial_task(&task_path, &created_worktrees);
            return Err(e);
        }

        let mut task = Task::new(
            name,
            task_path.clone(),
            repos,
            base_branch.map(String::from),
        );
        if let Err(e) = self.store.save_task(&task) {
            self.cleanup_partial_task(&task_path, &created_worktrees);
            return Err(e);
        }

        self.setup_plugin_symlinks(&task)?;

        let session = self.create_session_with_panes(name, &mut task)?;
        let _ = session; // session handle not needed after pane setup

        self.store.save_task(&task)?;
        Ok(task)
    }

    pub fn attach_task(&self, name: &str, repo_paths: Vec<PathBuf>) -> Result<Task> {
        if self.store.task_exists(name) {
            return Err(WagnerError::TaskExists(name.to_string()));
        }

        let mut repos = Vec::new();
        for path in &repo_paths {
            let canonical = path.canonicalize().map_err(|e| {
                WagnerError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Cannot resolve path {}: {}", path.display(), e),
                ))
            })?;

            let repo_name = canonical
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "repo".to_string());

            let branch = get_current_branch(&canonical).unwrap_or_else(|| "HEAD".to_string());

            repos.push(TaskRepo {
                name: repo_name,
                source: RepoSource::Local(canonical.clone()),
                worktree: canonical,
                branch,
            });
        }

        let task_path = if repos.len() == 1 {
            repos[0].worktree.clone()
        } else {
            repos
                .first()
                .and_then(|r| r.worktree.parent())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| repos[0].worktree.clone())
        };

        let mut task = Task::new_attached(name, task_path, repos);
        self.store.save_task(&task)?;
        self.setup_plugin_symlinks_attached(&task)?;

        let session = self.create_session_with_panes(name, &mut task)?;
        let _ = session;

        self.store.save_task(&task)?;
        Ok(task)
    }

    pub fn quick_launch(&self, engine: crate::model::Engine, name: Option<&str>) -> Result<()> {
        let cwd = std::env::current_dir()?.canonicalize()?;
        let dir_name = cwd
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "task".to_string());
        let task_name = name.unwrap_or(&dir_name);

        if self.store.task_exists(task_name) {
            let session_name = session_name_for_task(task_name);
            if self.terminal.session_exists(task_name)? {
                match self.resume_dead_agents(task_name) {
                    Ok(n) if n > 0 => {
                        debug!(count = n, "Resumed dead agents");
                    }
                    _ => {}
                }
                return self.terminal.attach(&SessionHandle(session_name));
            }
            self.store.delete_task(task_name)?;
        }

        let branch = crate::attach::get_current_branch(&cwd).unwrap_or_else(|| "none".to_string());
        let repo = TaskRepo {
            name: dir_name.clone(),
            source: RepoSource::Local(cwd.clone()),
            worktree: cwd.clone(),
            branch,
        };

        let mut task = Task::new_attached(task_name, cwd.clone(), vec![repo.clone()]);
        self.store.save_task(&task)?;

        let session = self.terminal.create_session(task_name, &cwd)?;
        if let Ok(panes) = self.terminal.list_panes(&session)
            && let Some(pane) = panes.first()
        {
            self.prepare_agent_in_pane_with_engine(&mut task, pane, &repo, None, engine)?;
        }

        self.store.save_task(&task)?;
        self.terminal.attach(&session)
    }

    pub fn detach_task(&self, name: &str) -> Result<()> {
        let _task = self.store.load_task(name)?;

        if self.terminal.session_exists(name)? {
            self.terminal
                .kill_session(&SessionHandle(session_name_for_task(name)))?;
        }

        self.store.delete_task(name)
    }

    fn setup_plugin_symlinks_attached(&self, task: &Task) -> Result<()> {
        let enabled_plugins: Vec<_> = builtin_plugins()
            .into_iter()
            .filter(|p| p.is_enabled(&self.config))
            .collect();

        if enabled_plugins.is_empty() {
            return Ok(());
        }

        for repo in &task.repos {
            let repo_wagner_dir = repo.worktree.join(".wagner");
            let repo_plugins_dir = repo_wagner_dir.join("plugins");
            std::fs::create_dir_all(&repo_plugins_dir)?;

            self.ensure_gitignore_has_wagner(&repo.worktree)?;

            for plugin in &enabled_plugins {
                let plugin_data_dir = repo_plugins_dir.join(plugin.data_dir());
                std::fs::create_dir_all(&plugin_data_dir)?;
                debug!(plugin = %plugin.id(), dir = %plugin_data_dir.display(), "Created plugin data dir");
            }

            let claude_dir = repo.worktree.join(".claude");
            std::fs::create_dir_all(&claude_dir)?;

            for plugin in &enabled_plugins {
                if plugin.id() == "chains" {
                    let chains_link = claude_dir.join("chains");
                    if !chains_link.exists() {
                        let target = repo_plugins_dir.join("chains");
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(&target, &chains_link)?;
                        debug!(link = %chains_link.display(), target = %target.display(), "Created chains symlink");
                    }
                }
            }
        }

        Ok(())
    }

    fn fetch_and_update_branch(&self, repo: &Path, branch: &str) {
        let _ = Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "fetch", "origin", branch])
            .output();

        let _ = Command::new("git")
            .args([
                "-C",
                &repo.to_string_lossy(),
                "branch",
                "-f",
                branch,
                &format!("origin/{}", branch),
            ])
            .output();
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        self.store.list_tasks()
    }

    pub fn get_task(&self, name: &str) -> Result<Task> {
        self.store.load_task(name)
    }

    pub fn delete_task(&self, name: &str, force: bool) -> Result<()> {
        let task = self.store.load_task(name)?;

        if task.is_attached() {
            return Err(WagnerError::CannotDeleteAttached(name.to_string()));
        }

        if self.terminal.session_exists(name)? {
            self.terminal
                .kill_session(&SessionHandle(session_name_for_task(name)))?;
        }

        for repo in &task.repos {
            let main_repo = self.get_main_repo(&repo.worktree, &repo.source);

            if repo.worktree.exists() {
                self.remove_worktree(&main_repo, &repo.worktree)?;
            }

            self.prune_worktrees(&main_repo);

            if force {
                self.delete_branch(&main_repo, &repo.branch)?;
            }
        }

        self.store.delete_task(name)
    }

    fn get_main_repo(&self, worktree: &Path, source: &RepoSource) -> PathBuf {
        if worktree.exists() {
            let output = Command::new("git")
                .args([
                    "-C",
                    &worktree.to_string_lossy(),
                    "rev-parse",
                    "--git-common-dir",
                ])
                .output();

            if let Ok(output) = output
                && output.status.success()
            {
                let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let git_path = PathBuf::from(&git_dir);

                let git_path = if git_path.is_relative() {
                    worktree.join(&git_path).canonicalize().unwrap_or(git_path)
                } else {
                    git_path
                };

                if git_path.join("HEAD").exists() {
                    return git_path;
                }
                if let Some(parent) = git_path.parent()
                    && (parent.join(".git").exists() || parent.join("HEAD").exists())
                {
                    return parent.to_path_buf();
                }
            }
        }

        match source {
            RepoSource::Local(path) => path.clone(),
            RepoSource::Remote(url) => self.config.repos_root.join(url_to_repo_path(url)),
        }
    }

    pub fn add_pane_with_engine(
        &self,
        task_name: &str,
        repo_name: Option<&str>,
        pane_name: Option<&str>,
        engine: Option<crate::model::Engine>,
    ) -> Result<PaneHandle> {
        let mut task = self.store.load_task(task_name)?;
        let session = SessionHandle(session_name_for_task(task_name));

        let repo = match repo_name {
            Some(name) => task
                .repos
                .iter()
                .find(|r| r.name == name)
                .ok_or_else(|| WagnerError::RepoNotFound(name.to_string(), PathBuf::new()))?
                .clone(),
            None => task
                .repos
                .first()
                .cloned()
                .unwrap_or_else(|| task.core_repo()),
        };

        let created_session = if !self.terminal.session_exists(task_name)? {
            self.terminal.create_session(task_name, &repo.worktree)?;
            true
        } else {
            false
        };

        let pane = if created_session {
            let panes = self.terminal.list_panes(&session)?;
            panes
                .into_iter()
                .next()
                .ok_or_else(|| WagnerError::Terminal("Session created but no panes found".into()))?
        } else {
            self.terminal.create_pane(&session, &repo.worktree)?
        };

        if let Some(engine_type) = engine {
            self.prepare_agent_in_pane_with_engine(
                &mut task,
                &pane,
                &repo,
                pane_name,
                engine_type,
            )?;
        } else {
            self.prepare_agent_in_pane(&mut task, &pane, &repo, pane_name)?;
        }
        self.store.save_task(&task)?;
        Ok(pane)
    }

    pub fn add_pane(
        &self,
        task_name: &str,
        repo_name: Option<&str>,
        pane_name: Option<&str>,
    ) -> Result<PaneHandle> {
        let mut task = self.store.load_task(task_name)?;
        let session = SessionHandle(session_name_for_task(task_name));

        let repo = match repo_name {
            Some(name) => task
                .repos
                .iter()
                .find(|r| r.name == name)
                .ok_or_else(|| WagnerError::RepoNotFound(name.to_string(), PathBuf::new()))?
                .clone(),
            None => task
                .repos
                .first()
                .cloned()
                .unwrap_or_else(|| task.core_repo()),
        };

        let created_session = if !self.terminal.session_exists(task_name)? {
            self.terminal.create_session(task_name, &repo.worktree)?;
            true
        } else {
            false
        };

        let pane = if created_session {
            let panes = self.terminal.list_panes(&session)?;
            panes
                .into_iter()
                .next()
                .ok_or_else(|| WagnerError::Terminal("Session created but no panes found".into()))?
        } else {
            self.terminal.create_pane(&session, &repo.worktree)?
        };

        self.prepare_agent_in_pane(&mut task, &pane, &repo, pane_name)?;
        self.store.save_task(&task)?;
        Ok(pane)
    }

    fn create_session_with_panes(
        &self,
        name: &str,
        task: &mut Task,
    ) -> Result<SessionHandle> {
        let session_dir = task
            .repos
            .first()
            .map(|r| r.worktree.clone())
            .unwrap_or_else(|| task.path.clone());

        let session = self.terminal.create_session(name, &session_dir)?;

        if task.repos.len() > 1 {
            let repos: Vec<_> = task.repos.clone();
            // First pane (already created with session) gets first repo
            if let Ok(panes) = self.terminal.list_panes(&session)
                && let Some(pane) = panes.first()
            {
                let _ = self.prepare_agent_in_pane(task, pane, &repos[0], None);
            }
            // Additional repos get new panes
            for repo in &repos[1..] {
                let pane = self.terminal.create_pane(&session, &repo.worktree)?;
                let _ = self.prepare_agent_in_pane(task, &pane, repo, None);
            }
        } else if let Ok(panes) = self.terminal.list_panes(&session)
            && let Some(pane) = panes.first()
        {
            let first_repo = task.repos[0].clone();
            let _ = self.prepare_agent_in_pane(task, pane, &first_repo, None);
        }

        Ok(session)
    }

    fn prepare_agent_in_pane_with_engine(
        &self,
        task: &mut Task,
        pane: &PaneHandle,
        repo: &TaskRepo,
        name_override: Option<&str>,
        engine_type: crate::model::Engine,
    ) -> Result<TrackedPane> {
        use crate::model::{Engine, PENDING_DISCOVERY};
        let session_id = Uuid::new_v4().to_string();

        let pane_name = match name_override {
            Some(n) => n.to_string(),
            None => {
                let base = match engine_type {
                    Engine::ClaudeCode => format!("claude-{}", repo.name),
                    Engine::Codex => format!("codex-{}", repo.name),
                    Engine::Terminal => repo.name.clone(),
                };
                task.next_pane_name(&base)
            }
        };

        if engine_type != Engine::Terminal {
            self.terminal.shell_init_delay();
            let cmd = engine_type.launch_command(&session_id);
            self.terminal
                .send_text_enter(pane, &cmd, engine_type.enter_delay_ms())?;
        }

        let jsonl_path = match engine_type {
            Engine::ClaudeCode => {
                let project_id = repo.worktree.to_string_lossy().replace(['/', '.'], "-");
                if let Ok(home) = std::env::var("HOME") {
                    std::path::PathBuf::from(home)
                        .join(".claude")
                        .join("projects")
                        .join(project_id)
                        .join(format!("{session_id}.jsonl"))
                } else {
                    PathBuf::from(PENDING_DISCOVERY)
                }
            }
            _ => PathBuf::from(PENDING_DISCOVERY),
        };

        let tracked = TrackedPane {
            name: pane_name,
            repo_name: repo.name.clone(),
            engine: engine_type,
            session_id,
            pane_id: pane.0.clone(),
            jsonl_path,
            launched_at: Utc::now(),
        };

        task.panes.push(tracked.clone());
        Ok(tracked)
    }

    fn prepare_agent_in_pane(
        &self,
        task: &mut Task,
        pane: &PaneHandle,
        repo: &TaskRepo,
        name_override: Option<&str>,
    ) -> Result<TrackedPane> {
        let session_id = Uuid::new_v4().to_string();
        let engine = self.agent.engine();
        let cwd = &repo.worktree;

        let pane_name = match name_override {
            Some(n) => n.to_string(),
            None => task.next_pane_name(&repo.name),
        };

        self.terminal.shell_init_delay();
        let cmd = self.agent.launch_command(&session_id);
        self.terminal.send_keys(pane, &cmd)?;

        let jsonl_path = self
            .agent
            .predict_jsonl_path(&session_id, cwd)
            .unwrap_or_else(|| PathBuf::from("pending-discovery"));

        let tracked = TrackedPane {
            name: pane_name,
            repo_name: repo.name.clone(),
            engine,
            session_id,
            pane_id: pane.0.clone(),
            jsonl_path,
            launched_at: Utc::now(),
        };

        task.panes.push(tracked.clone());
        Ok(tracked)
    }

    pub fn resume_dead_agents(&self, task_name: &str) -> Result<usize> {
        let task = self.store.load_task(task_name)?;
        let session = SessionHandle(session_name_for_task(task_name));
        let panes = self.terminal.list_panes(&session).unwrap_or_default();
        let mut resumed = 0;

        for tracked in &task.panes {
            let Some(pane) = panes.iter().find(|p| p.0 == tracked.pane_id) else {
                continue;
            };

            let pane_cmd = self
                .terminal
                .get_pane_command(pane)
                .unwrap_or_default()
                .to_ascii_lowercase();

            if pane_cmd.contains(tracked.engine.process_name()) {
                continue;
            }

            let resume_cmd = tracked.engine.resume_command(&tracked.session_id);
            self.terminal.send_keys(pane, &resume_cmd)?;
            resumed += 1;
        }

        Ok(resumed)
    }

    pub fn attach(&self, task_name: &str, pane_id: Option<&str>) -> Result<()> {
        let session = SessionHandle(session_name_for_task(task_name));
        if let Some(id) = pane_id {
            let pane = PaneHandle(id.to_string(), String::new());
            self.terminal.select_pane(&pane)?;
        }
        self.terminal.attach(&session)
    }

    pub fn add_repo_to_task(&self, task_name: &str, spec: &RepoSpec) -> Result<()> {
        let mut task = self.store.load_task(task_name)?;

        if task.repos.iter().any(|r| r.name == spec.name) {
            return Err(WagnerError::Git(format!(
                "Repo '{}' already exists in task",
                spec.name
            )));
        }

        let worktree_path = task.path.join(&spec.name);

        match &spec.source {
            RepoSource::Local(source_path) => {
                if !source_path.exists() {
                    return Err(WagnerError::RepoNotFound(
                        spec.name.clone(),
                        source_path.clone(),
                    ));
                }
                self.create_worktree(source_path, &worktree_path, &spec.branch)?;
            }
            RepoSource::Remote(url) => {
                let clone_path = self.clone_repo(url)?;
                self.create_worktree(&clone_path, &worktree_path, &spec.branch)?;
            }
        }

        task.repos.push(TaskRepo {
            name: spec.name.clone(),
            source: spec.source.clone(),
            worktree: worktree_path,
            branch: spec.branch.clone(),
        });

        self.store.save_task(&task)
    }

    pub fn remove_repo_from_task(&self, task_name: &str, repo_name: &str) -> Result<()> {
        let mut task = self.store.load_task(task_name)?;

        let repo_idx = task
            .repos
            .iter()
            .position(|r| r.name == repo_name)
            .ok_or_else(|| WagnerError::RepoNotFound(repo_name.to_string(), PathBuf::new()))?;

        let repo = &task.repos[repo_idx];
        let main_repo = self.get_main_repo(&repo.worktree, &repo.source);

        if repo.worktree.exists() {
            self.remove_worktree(&main_repo, &repo.worktree)?;
        }
        self.prune_worktrees(&main_repo);

        task.repos.remove(repo_idx);
        self.store.save_task(&task)
    }

    fn create_worktree(&self, repo: &Path, worktree: &Path, branch: &str) -> Result<()> {
        let start_point = self.get_default_ref(repo);
        let repo_str = repo.to_string_lossy();
        let worktree_str = worktree.to_string_lossy();

        let mut args = vec![
            "-C",
            repo_str.as_ref(),
            "worktree",
            "add",
            "-b",
            branch,
            worktree_str.as_ref(),
        ];
        if let Some(ref sp) = start_point {
            args.push(sp);
        }

        let output = Command::new("git").args(&args).output()?;

        if !output.status.success() {
            let output = Command::new("git")
                .args([
                    "-C",
                    repo_str.as_ref(),
                    "worktree",
                    "add",
                    worktree_str.as_ref(),
                    branch,
                ])
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(WagnerError::Git(stderr.to_string()));
            }
        }

        Ok(())
    }

    fn get_default_ref(&self, repo: &Path) -> Option<String> {
        let is_bare = Command::new("git")
            .args([
                "-C",
                &repo.to_string_lossy(),
                "rev-parse",
                "--is-bare-repository",
            ])
            .output()
            .ok()
            .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false);

        if is_bare {
            for branch in ["origin/main", "origin/master"] {
                let output = Command::new("git")
                    .args([
                        "-C",
                        &repo.to_string_lossy(),
                        "rev-parse",
                        "--verify",
                        branch,
                    ])
                    .output()
                    .ok()?;
                if output.status.success() {
                    return Some(branch.to_string());
                }
            }
            return None;
        }

        let output = Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "symbolic-ref", "HEAD"])
            .output()
            .ok()?;

        if output.status.success() {
            return None;
        }

        for branch in ["origin/main", "origin/master"] {
            let output = Command::new("git")
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "rev-parse",
                    "--verify",
                    branch,
                ])
                .output()
                .ok()?;
            if output.status.success() {
                return Some(branch.to_string());
            }
        }

        None
    }

    fn remove_worktree(&self, main_repo: &Path, worktree: &Path) -> Result<()> {
        let output = Command::new("git")
            .args([
                "-C",
                &main_repo.to_string_lossy(),
                "worktree",
                "remove",
                "--force",
                &worktree.to_string_lossy(),
            ])
            .output()?;

        if !output.status.success() && worktree.exists() {
            std::fs::remove_dir_all(worktree)?;
        }

        Ok(())
    }

    fn prune_worktrees(&self, main_repo: &Path) {
        let _ = Command::new("git")
            .args(["-C", &main_repo.to_string_lossy(), "worktree", "prune"])
            .output();
    }

    fn delete_branch(&self, main_repo: &Path, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["-C", &main_repo.to_string_lossy(), "branch", "-D", branch])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("not found") {
                return Err(WagnerError::Git(format!(
                    "Failed to delete branch '{}': {}",
                    branch, stderr
                )));
            }
        }

        Ok(())
    }

    fn clone_repo(&self, url: &str) -> Result<PathBuf> {
        let clone_path = self.config.repos_root.join(url_to_repo_path(url));

        if clone_path.exists() {
            self.fetch_repo(&clone_path)?;
            return Ok(clone_path);
        }

        if let Some(parent) = clone_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let output = Command::new("git")
            .args(["clone", "--bare", url, &clone_path.to_string_lossy()])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WagnerError::Git(stderr.to_string()));
        }

        Ok(clone_path)
    }

    fn fetch_repo(&self, repo_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args([
                "-C",
                &repo_path.to_string_lossy(),
                "fetch",
                "--all",
                "--prune",
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WagnerError::Git(format!("fetch failed: {}", stderr)));
        }

        Ok(())
    }

    fn cleanup_partial_task(&self, task_path: &Path, created_worktrees: &[(PathBuf, PathBuf)]) {
        for (main_repo, worktree) in created_worktrees {
            let _ = self.remove_worktree(main_repo, worktree);
            self.prune_worktrees(main_repo);
        }

        if task_path.exists() {
            let _ = std::fs::remove_dir_all(task_path);
        }
    }

    fn setup_plugin_symlinks(&self, task: &Task) -> Result<()> {
        let enabled_plugins: Vec<_> = builtin_plugins()
            .into_iter()
            .filter(|p| p.is_enabled(&self.config))
            .collect();

        if enabled_plugins.is_empty() {
            return Ok(());
        }

        let first_repo = match task.repos.first() {
            Some(r) => r,
            None => return Ok(()),
        };

        let source_repo = match &first_repo.source {
            RepoSource::Local(path) => path.clone(),
            RepoSource::Remote(_) => return Ok(()),
        };

        let repo_wagner_dir = source_repo.join(".wagner");
        let repo_plugins_dir = repo_wagner_dir.join("plugins");

        std::fs::create_dir_all(&repo_plugins_dir)?;

        self.ensure_gitignore_has_wagner(&source_repo)?;

        for plugin in &enabled_plugins {
            let plugin_data_dir = repo_plugins_dir.join(plugin.data_dir());
            std::fs::create_dir_all(&plugin_data_dir)?;
            debug!(plugin = %plugin.id(), dir = %plugin_data_dir.display(), "Created plugin data dir");
        }

        let task_wagner_dir = task.path.join(".wagner");
        std::fs::create_dir_all(&task_wagner_dir)?;

        let task_plugins_link = task_wagner_dir.join("plugins");
        if !task_plugins_link.exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&repo_plugins_dir, &task_plugins_link)?;
            debug!(link = %task_plugins_link.display(), target = %repo_plugins_dir.display(), "Created plugins symlink");
        }

        for repo in &task.repos {
            let claude_dir = repo.worktree.join(".claude");
            std::fs::create_dir_all(&claude_dir)?;

            for plugin in &enabled_plugins {
                if plugin.id() == "chains" {
                    let chains_link = claude_dir.join("chains");
                    if !chains_link.exists() {
                        let target = task_plugins_link.join("chains");
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(&target, &chains_link)?;
                        debug!(link = %chains_link.display(), target = %target.display(), "Created chains symlink");
                    }
                }
            }
        }

        Ok(())
    }

    fn ensure_gitignore_has_wagner(&self, repo_path: &Path) -> Result<()> {
        let gitignore_path = repo_path.join(".gitignore");

        let content = if gitignore_path.exists() {
            std::fs::read_to_string(&gitignore_path)?
        } else {
            String::new()
        };

        if content
            .lines()
            .any(|line| line.trim() == ".wagner/" || line.trim() == ".wagner")
        {
            return Ok(());
        }

        let new_content = if content.is_empty() || content.ends_with('\n') {
            format!("{}.wagner/\n", content)
        } else {
            format!("{}\n.wagner/\n", content)
        };

        std::fs::write(&gitignore_path, new_content)?;
        debug!(path = %gitignore_path.display(), "Added .wagner/ to .gitignore");

        Ok(())
    }
}

fn url_to_repo_path(url: &str) -> PathBuf {
    let url = url.strip_suffix(".git").unwrap_or(url);

    if let Some(rest) = url.strip_prefix("ssh://") {
        let without_user = rest.split('@').next_back().unwrap_or(rest);
        let normalized = if let Some(colon_pos) = without_user.find(':') {
            let after_colon = &without_user[colon_pos + 1..];
            if after_colon.starts_with(|c: char| c.is_ascii_digit()) {
                if let Some(slash_pos) = after_colon.find('/') {
                    format!(
                        "{}{}",
                        &without_user[..colon_pos],
                        &after_colon[slash_pos..]
                    )
                } else {
                    without_user.to_string()
                }
            } else {
                without_user.replace(':', "/")
            }
        } else {
            without_user.to_string()
        };
        return PathBuf::from(normalized.trim_start_matches('/'));
    }

    if let Some(rest) = url.strip_prefix("git@") {
        let normalized = rest.replace(':', "/");
        return PathBuf::from(normalized);
    }

    if let Some(rest) = url.strip_prefix("https://") {
        return PathBuf::from(rest);
    }

    if let Some(rest) = url.strip_prefix("http://") {
        return PathBuf::from(rest);
    }

    PathBuf::from(url)
}

pub fn default_branch_for_task(task_name: &str) -> String {
    format!("feature/{}", task_name)
}

#[derive(Debug, Clone)]
pub struct RepoSpec {
    pub name: String,
    pub source: RepoSource,
    pub branch: String,
}

impl RepoSpec {
    pub fn parse(s: &str, default_branch: Option<&str>) -> Result<Self> {
        let parts: Vec<&str> = s.split(':').collect();

        match parts.len() {
            3 => Ok(Self {
                name: parts[0].to_string(),
                source: RepoSource::parse(parts[1]),
                branch: parts[2].to_string(),
            }),
            2 => Ok(Self {
                name: parts[0].to_string(),
                source: RepoSource::parse(parts[1]),
                branch: default_branch.unwrap_or("main").to_string(),
            }),
            _ => Err(WagnerError::InvalidRepoSpec(format!(
                "Expected format: name:source:branch or name:source, got: {}",
                s
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_to_repo_path_ssh_with_git_suffix() {
        let result = url_to_repo_path("git@github.com:user/repo.git");
        assert_eq!(result, PathBuf::from("github.com/user/repo"));
    }

    #[test]
    fn url_to_repo_path_ssh_without_git_suffix() {
        let result = url_to_repo_path("git@github.com:user/repo");
        assert_eq!(result, PathBuf::from("github.com/user/repo"));
    }

    #[test]
    fn url_to_repo_path_https_with_git_suffix() {
        let result = url_to_repo_path("https://github.com/user/repo.git");
        assert_eq!(result, PathBuf::from("github.com/user/repo"));
    }

    #[test]
    fn url_to_repo_path_https_without_git_suffix() {
        let result = url_to_repo_path("https://github.com/user/repo");
        assert_eq!(result, PathBuf::from("github.com/user/repo"));
    }

    #[test]
    fn url_to_repo_path_http_with_git_suffix() {
        let result = url_to_repo_path("http://gitlab.com/user/repo.git");
        assert_eq!(result, PathBuf::from("gitlab.com/user/repo"));
    }

    #[test]
    fn url_to_repo_path_http_without_git_suffix() {
        let result = url_to_repo_path("http://gitlab.com/user/repo");
        assert_eq!(result, PathBuf::from("gitlab.com/user/repo"));
    }

    #[test]
    fn url_to_repo_path_nested_path() {
        let result = url_to_repo_path("git@github.com:org/subgroup/repo.git");
        assert_eq!(result, PathBuf::from("github.com/org/subgroup/repo"));
    }

    #[test]
    fn url_to_repo_path_https_nested_path() {
        let result = url_to_repo_path("https://gitlab.com/org/subgroup/repo.git");
        assert_eq!(result, PathBuf::from("gitlab.com/org/subgroup/repo"));
    }

    #[test]
    fn url_to_repo_path_self_hosted() {
        let result = url_to_repo_path("git@git.company.com:team/project.git");
        assert_eq!(result, PathBuf::from("git.company.com/team/project"));
    }

    #[test]
    fn url_to_repo_path_unknown_format_passthrough() {
        let result = url_to_repo_path("some/local/path");
        assert_eq!(result, PathBuf::from("some/local/path"));
    }

    #[test]
    fn url_to_repo_path_strips_single_git_suffix() {
        let result = url_to_repo_path("https://github.com/user/repo.git.git");
        assert_eq!(result, PathBuf::from("github.com/user/repo.git"));
    }

    #[test]
    fn url_to_repo_path_ssh_protocol() {
        let result = url_to_repo_path("ssh://git@github.com/user/repo.git");
        assert_eq!(result, PathBuf::from("github.com/user/repo"));
    }

    #[test]
    fn url_to_repo_path_ssh_protocol_with_port() {
        let result = url_to_repo_path("ssh://git@github.com:22/user/repo.git");
        assert_eq!(result, PathBuf::from("github.com/user/repo"));
    }
}
