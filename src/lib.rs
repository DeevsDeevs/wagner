pub mod agent;
pub mod config;
pub mod error;
pub mod model;
pub mod monitor;
pub mod store;
pub mod terminal;
pub mod tui;
pub mod wagner;

pub use agent::{Agent, ClaudeCode};
pub use config::Config;
pub use error::{Result, WagnerError};
pub use model::{RepoSource, Session, SessionStatus, Task, TaskRepo};
pub use store::Store;
pub use terminal::{PaneHandle, SessionHandle, Terminal, Tmux};
pub use wagner::{RepoSpec, Wagner};
