mod tmux;

pub use tmux::Tmux;

use crate::error::Result;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionHandle(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PaneHandle(pub String);

pub trait Terminal: Send + Sync {
    fn create_session(&self, name: &str, cwd: &Path) -> Result<SessionHandle>;
    fn create_pane(&self, session: &SessionHandle, cwd: &Path) -> Result<PaneHandle>;
    fn capture(&self, pane: &PaneHandle, lines: usize) -> Result<String>;
    fn send_keys(&self, pane: &PaneHandle, keys: &str) -> Result<()>;
    fn attach(&self, session: &SessionHandle) -> Result<()>;
    fn list_panes(&self, session: &SessionHandle) -> Result<Vec<PaneHandle>>;
    fn kill_pane(&self, pane: &PaneHandle) -> Result<()>;
    fn kill_session(&self, session: &SessionHandle) -> Result<()>;
    fn session_exists(&self, name: &str) -> Result<bool>;
}
