use crate::config::Config;
use crate::error::{Result, WagnerError};
use crate::model::Task;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

        // Atomic write: write to a temporary file in the same directory,
        // then rename. This prevents corruption from concurrent writes or
        // crashes mid-write.
        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    pub fn register_attached(&self, name: &str, task_path: &Path) -> Result<()> {
        let mut registry = self.load_attached_registry();
        registry.insert(name.to_string(), task_path.to_path_buf());
        self.save_attached_registry(&registry)
    }

    pub fn unregister_attached(&self, name: &str) -> Result<()> {
        let mut registry = self.load_attached_registry();
        registry.remove(name);
        self.save_attached_registry(&registry)
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
