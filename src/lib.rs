pub mod agent;
pub mod config;
pub mod error;
pub mod git;
pub mod model;
pub mod monitor;
pub mod store;
pub mod terminal;
pub mod tui;
pub mod wagner;

pub use agent::{Agent, ClaudeCode, ClaudeCodeDetector};
pub use config::Config;
pub use error::{Result, WagnerError};
pub use model::{RepoSource, Task, TaskRepo};
pub use store::Store;
pub use terminal::{PaneHandle, SessionHandle, Terminal, Tmux};
pub use wagner::{RepoSpec, Wagner, default_branch_for_task};
