use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, WagnerError>;

#[derive(Error, Debug)]
pub enum WagnerError {
    #[error("Task '{0}' not found")]
    TaskNotFound(String),

    #[error("Task '{0}' already exists")]
    TaskExists(String),

    #[error("Repository '{0}' not found at {1}")]
    RepoNotFound(String, PathBuf),

    #[error("Terminal error: {0}")]
    Terminal(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid repo spec: {0}")]
    InvalidRepoSpec(String),
}
