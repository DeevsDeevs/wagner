use crate::config::Config;
use crate::error::{Result, WagnerError};
use crate::model::{Session, Task};

pub struct Store {
    config: Config,
}

impl Store {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn save_task(&self, task: &Task) -> Result<()> {
        let metadata_dir = task.metadata_dir();
        std::fs::create_dir_all(&metadata_dir)?;

        let content = serde_json::to_string_pretty(task)?;
        std::fs::write(task.metadata_path(), content)?;
        Ok(())
    }

    pub fn load_task(&self, name: &str) -> Result<Task> {
        let task_path = self.config.tasks_root.join(name);
        let metadata_path = task_path.join(".wagner").join("task.json");

        if !metadata_path.exists() {
            return Err(WagnerError::TaskNotFound(name.to_string()));
        }

        let content = std::fs::read_to_string(&metadata_path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        let tasks_root = &self.config.tasks_root;

        if !tasks_root.exists() {
            return Ok(vec![]);
        }

        let mut tasks = Vec::new();

        for entry in std::fs::read_dir(tasks_root)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let metadata_path = path.join(".wagner").join("task.json");
                if metadata_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&metadata_path) {
                        if let Ok(task) = serde_json::from_str::<Task>(&content) {
                            tasks.push(task);
                        }
                    }
                }
            }
        }

        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tasks)
    }

    pub fn delete_task(&self, name: &str) -> Result<()> {
        let task_path = self.config.tasks_root.join(name);

        if !task_path.exists() {
            return Err(WagnerError::TaskNotFound(name.to_string()));
        }

        std::fs::remove_dir_all(&task_path)?;
        Ok(())
    }

    pub fn task_exists(&self, name: &str) -> bool {
        let task_path = self.config.tasks_root.join(name);
        task_path.join(".wagner").join("task.json").exists()
    }

    pub fn save_sessions(&self, sessions: &[Session]) -> Result<()> {
        let path = Config::sessions_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(&sessions)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn load_sessions(&self) -> Result<Vec<Session>> {
        let path = Config::sessions_path();

        if !path.exists() {
            return Ok(vec![]);
        }

        let content = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    }
}
