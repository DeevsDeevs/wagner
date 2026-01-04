use crate::agent::Agent;
use crate::config::Config;
use crate::error::{Result, WagnerError};
use crate::model::{RepoSource, Task, TaskRepo};
use crate::store::Store;
use crate::terminal::{Terminal, SessionHandle, PaneHandle};
use std::path::PathBuf;
use std::process::Command;

pub struct Wagner<T: Terminal, A: Agent> {
    terminal: T,
    agent: A,
    store: Store,
    config: Config,
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

    pub fn create_task(&self, name: &str, repo_specs: &[RepoSpec]) -> Result<Task> {
        if self.store.task_exists(name) {
            return Err(WagnerError::TaskExists(name.to_string()));
        }

        let task_path = self.config.tasks_root.join(name);
        std::fs::create_dir_all(&task_path)?;

        let mut repos = Vec::new();

        for spec in repo_specs {
            let worktree_path = task_path.join(&spec.name);

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
                    let clone_path = self.clone_repo(url, &task_path)?;
                    self.create_worktree(&clone_path, &worktree_path, &spec.branch)?;
                }
            }

            self.agent.setup_hooks(&worktree_path)?;

            repos.push(TaskRepo {
                name: spec.name.clone(),
                source: spec.source.clone(),
                worktree: worktree_path,
                branch: spec.branch.clone(),
            });
        }

        let task = Task::new(name, task_path, repos);
        self.store.save_task(&task)?;

        self.terminal.create_session(name, &task.path)?;

        Ok(task)
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        self.store.list_tasks()
    }

    pub fn get_task(&self, name: &str) -> Result<Task> {
        self.store.load_task(name)
    }

    pub fn delete_task(&self, name: &str, force: bool) -> Result<()> {
        let task = self.store.load_task(name)?;

        if self.terminal.session_exists(name)? {
            self.terminal.kill_session(&SessionHandle(format!("wagner_{}", name)))?;
        }

        for repo in &task.repos {
            if repo.worktree.exists() {
                self.remove_worktree(&repo.worktree)?;
            }

            if force {
                self.delete_branch(&repo.worktree, &repo.branch)?;
            }
        }

        self.store.delete_task(name)
    }

    pub fn add_pane(&self, task_name: &str, repo_name: Option<&str>) -> Result<PaneHandle> {
        let task = self.store.load_task(task_name)?;
        let session = SessionHandle(format!("wagner_{}", task_name));

        let repo = match repo_name {
            Some(name) => task
                .repos
                .iter()
                .find(|r| r.name == name)
                .ok_or_else(|| WagnerError::RepoNotFound(name.to_string(), PathBuf::new()))?,
            None => task
                .repos
                .first()
                .ok_or_else(|| WagnerError::TaskNotFound(task_name.to_string()))?,
        };

        self.terminal.create_pane(&session, &repo.worktree)
    }

    pub fn attach(&self, task_name: &str) -> Result<()> {
        let session = SessionHandle(format!("wagner_{}", task_name));
        self.terminal.attach(&session)
    }

    fn create_worktree(&self, repo: &PathBuf, worktree: &PathBuf, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args([
                "-C",
                &repo.to_string_lossy(),
                "worktree",
                "add",
                "-b",
                branch,
                &worktree.to_string_lossy(),
            ])
            .output()?;

        if !output.status.success() {
            let output = Command::new("git")
                .args([
                    "-C",
                    &repo.to_string_lossy(),
                    "worktree",
                    "add",
                    &worktree.to_string_lossy(),
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

    fn remove_worktree(&self, worktree: &PathBuf) -> Result<()> {
        Command::new("git")
            .args(["worktree", "remove", "--force", &worktree.to_string_lossy()])
            .output()?;
        Ok(())
    }

    fn delete_branch(&self, worktree: &PathBuf, branch: &str) -> Result<()> {
        Command::new("git")
            .args(["-C", &worktree.to_string_lossy(), "branch", "-D", branch])
            .output()?;
        Ok(())
    }

    fn clone_repo(&self, url: &str, target_dir: &PathBuf) -> Result<PathBuf> {
        let repo_name = url
            .split('/')
            .last()
            .unwrap_or("repo")
            .trim_end_matches(".git");

        let clone_path = target_dir.join(format!(".{}_clone", repo_name));

        let output = Command::new("git")
            .args(["clone", url, &clone_path.to_string_lossy()])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WagnerError::Git(stderr.to_string()));
        }

        Ok(clone_path)
    }
}

#[derive(Debug, Clone)]
pub struct RepoSpec {
    pub name: String,
    pub source: RepoSource,
    pub branch: String,
}

impl RepoSpec {
    pub fn parse(s: &str) -> Result<Self> {
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
                branch: "main".to_string(),
            }),
            _ => Err(WagnerError::InvalidRepoSpec(format!(
                "Expected format: name:source:branch or name:source, got: {}",
                s
            ))),
        }
    }
}
