pub mod agent;
pub mod attach;
pub mod config;
pub mod error;
pub mod git;
pub mod model;
pub mod monitor;
pub mod plugins;
pub mod store;
pub mod terminal;
pub mod transport;
pub mod tui;
pub mod wagner;

pub use agent::{
    Agent, AgentChoice, ClaudeCode, ClaudeCodeDetector, Codex, CodexDetector, TestAgent,
};
pub use attach::{AttachDetection, derive_task_name, detect_attach_mode};
pub use config::Config;
pub use error::{Result, WagnerError};
pub use model::{Engine, RepoSource, Task, TaskKind, TaskRepo, TrackedPane};
pub use store::Store;
pub use terminal::{MockTerminal, PaneHandle, SessionHandle, Terminal, Tmux};
pub use wagner::{RepoSpec, Wagner, default_branch_for_task};
