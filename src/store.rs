use crate::config::Config;
use crate::error::{Result, WagnerError};
use crate::model::Task;
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// Resolve the task name for a given working directory by checking both the
/// managed tasks directory and the attached-task registry.
pub fn detect_task_for_cwd(cwd: &Path, config: &Config) -> Option<String> {
    let tasks_root = &config.tasks_root;

    // Check managed tasks under tasks_root
    if cwd.starts_with(tasks_root)
        && let Ok(relative) = cwd.strip_prefix(tasks_root)
        && let Some(task_name) = relative.components().next()
    {
        let task_dir = tasks_root.join(task_name);
        if task_dir.join(".wagner").join("task.json").exists() {
            return Some(task_name.as_os_str().to_string_lossy().to_string());
        }
    }

    // Check attached tasks from the registry
    let registry_path = tasks_root.join(".attached_registry.json");
    if registry_path.exists()
        && let Ok(content) = std::fs::read_to_string(&registry_path)
        && let Ok(registry) = serde_json::from_str::<HashMap<String, PathBuf>>(&content)
    {
        for (name, task_path) in &registry {
            if cwd.starts_with(task_path)
                && task_path.join(".wagner").join("task.json").exists()
            {
                return Some(name.clone());
            }
        }
    }

    None
}

pub struct Store {
    config: Config,
}

impl Store {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    fn attached_registry_path(&self) -> PathBuf {
        self.config.tasks_root.join(".attached_registry.json")
    }

    fn load_attached_registry(&self) -> HashMap<String, PathBuf> {
        let path = self.attached_registry_path();
        if !path.exists() {
            return HashMap::new();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    fn save_attached_registry(&self, registry: &HashMap<String, PathBuf>) -> Result<()> {
        let path = self.attached_registry_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(registry)?;

        // Atomic write: write to a uniquely-named temporary file in the same
        // directory, then rename. Using PID + a monotonic counter ensures
        // concurrent writers never clobber each other's temp files.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = format!(
            "json.tmp.{}.{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        );
        let tmp_path = path.with_extension(unique);
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// Perform a locked read-modify-write on the attached registry file.
    ///
    /// Acquires an exclusive advisory lock (via `libc::flock`) on a `.lock`
    /// sidecar file before reading, calls `mutate` to update the registry
    /// in-place, then writes the result back. The lock is held for the entire
    /// critical section so concurrent CLI/daemon processes cannot lose updates.
    fn with_locked_registry<F>(&self, mutate: F) -> Result<()>
    where
        F: FnOnce(&mut HashMap<String, PathBuf>),
    {
        let registry_path = self.attached_registry_path();
        if let Some(parent) = registry_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Use a sidecar .lock file for the advisory lock so we don't interfere
        // with the atomic-rename write strategy on the main registry file.
        let lock_path = registry_path.with_extension("json.lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;

        // Acquire exclusive lock — blocks until available.
        let fd = lock_file.as_raw_fd();
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
        if ret != 0 {
            return Err(std::io::Error::last_os_error().into());
        }

        // Read current registry (under lock).
        let mut registry = self.load_attached_registry();

        // Let the caller mutate.
        mutate(&mut registry);

        // Write back (under lock).
        let result = self.save_attached_registry(&registry);

        // Release lock (explicit unlock before drop for clarity).
        unsafe { libc::flock(fd, libc::LOCK_UN) };

        result
    }

    pub fn register_attached(&self, name: &str, task_path: &Path) -> Result<()> {
        self.with_locked_registry(|registry| {
            registry.insert(name.to_string(), task_path.to_path_buf());
        })
    }

    pub fn unregister_attached(&self, name: &str) -> Result<()> {
        self.with_locked_registry(|registry| {
            registry.remove(name);
        })
    }

    pub fn save_task(&self, task: &Task) -> Result<()> {
        let metadata_dir = task.metadata_dir();
        std::fs::create_dir_all(&metadata_dir)?;

        let content = serde_json::to_string_pretty(task)?;
        std::fs::write(task.metadata_path(), content)?;

        if task.is_attached() {
            self.register_attached(&task.name, &task.path)?;
        }
        Ok(())
    }

    pub fn load_task(&self, name: &str) -> Result<Task> {
        let task_path = self.config.tasks_root.join(name);
        let metadata_path = task_path.join(".wagner").join("task.json");

        if metadata_path.exists() {
            let content = std::fs::read_to_string(&metadata_path)?;
            let mut task: Task = serde_json::from_str(&content)?;
            task.fixup_pane_names();
            return Ok(task);
        }

        let registry = self.load_attached_registry();
        if let Some(attached_path) = registry.get(name) {
            let metadata_path = attached_path.join(".wagner").join("task.json");
            if metadata_path.exists() {
                let content = std::fs::read_to_string(&metadata_path)?;
                let mut task: Task = serde_json::from_str(&content)?;
                task.fixup_pane_names();
                return Ok(task);
            }
        }

        Err(WagnerError::TaskNotFound(name.to_string()))
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        let tasks_root = &self.config.tasks_root;
        let mut tasks = Vec::new();

        if tasks_root.exists() {
            for entry in std::fs::read_dir(tasks_root)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    let metadata_path = path.join(".wagner").join("task.json");
                    if metadata_path.exists()
                        && let Ok(content) = std::fs::read_to_string(&metadata_path)
                        && let Ok(mut task) = serde_json::from_str::<Task>(&content)
                    {
                        task.fixup_pane_names();
                        tasks.push(task);
                    }
                }
            }
        }

        let registry = self.load_attached_registry();
        for (_name, attached_path) in registry {
            let metadata_path = attached_path.join(".wagner").join("task.json");
            if metadata_path.exists()
                && let Ok(content) = std::fs::read_to_string(&metadata_path)
                && let Ok(mut task) = serde_json::from_str::<Task>(&content)
            {
                task.fixup_pane_names();
                if !tasks.iter().any(|t| t.name == task.name) {
                    tasks.push(task);
                }
            }
        }

        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tasks)
    }

    pub fn delete_task(&self, name: &str) -> Result<()> {
        let task = self.load_task(name)?;

        if task.is_attached() {
            let metadata_dir = task.metadata_dir();
            if metadata_dir.exists() {
                std::fs::remove_dir_all(&metadata_dir)?;
            }
            self.unregister_attached(name)?;
        } else {
            let task_path = self.config.tasks_root.join(name);
            if task_path.exists() {
                std::fs::remove_dir_all(&task_path)?;
            }
        }

        Ok(())
    }

    pub fn task_exists(&self, name: &str) -> bool {
        let task_path = self.config.tasks_root.join(name);
        if task_path.join(".wagner").join("task.json").exists() {
            return true;
        }

        let registry = self.load_attached_registry();
        if let Some(attached_path) = registry.get(name) {
            return attached_path.join(".wagner").join("task.json").exists();
        }

        false
    }
}
