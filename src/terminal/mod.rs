mod mock;
mod tmux;

pub use mock::MockTerminal;
pub use tmux::Tmux;

use crate::error::Result;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionHandle(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PaneHandle(pub String, pub String); // (pane_id, title)

pub trait Terminal: Send + Sync {
    fn create_session(&self, name: &str, cwd: &Path) -> Result<SessionHandle>;
    fn create_pane(&self, session: &SessionHandle, cwd: &Path) -> Result<PaneHandle>;
    fn capture(&self, pane: &PaneHandle, lines: usize) -> Result<String>;
    fn send_keys(&self, pane: &PaneHandle, keys: &str) -> Result<()>;
    fn send_key(&self, pane: &PaneHandle, key: &str) -> Result<()>;
    fn send_literal(&self, pane: &PaneHandle, text: &str) -> Result<()>;
    fn attach(&self, session: &SessionHandle) -> Result<()>;
    fn list_panes(&self, session: &SessionHandle) -> Result<Vec<PaneHandle>>;
    fn kill_pane(&self, pane: &PaneHandle) -> Result<()>;
    fn kill_session(&self, session: &SessionHandle) -> Result<()>;
    fn session_exists(&self, name: &str) -> Result<bool>;
    fn get_pane_command(&self, pane: &PaneHandle) -> Result<String>;
}
