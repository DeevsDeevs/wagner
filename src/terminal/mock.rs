use super::{PaneHandle, SessionHandle, Terminal};
use crate::error::Result;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct MockTerminal {
    pub sessions: Arc<Mutex<HashMap<String, Vec<PaneHandle>>>>,
    pub sent_keys: Arc<Mutex<Vec<(String, String)>>>,
}

impl MockTerminal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_sent_keys(&self) -> Vec<(String, String)> {
        self.sent_keys.lock().unwrap().clone()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    pub fn pane_count(&self, session: &str) -> usize {
        self.sessions
            .lock()
            .unwrap()
            .get(session)
            .map(|p| p.len())
            .unwrap_or(0)
    }
}

impl Terminal for MockTerminal {
    fn create_session(&self, name: &str, _cwd: &Path) -> Result<SessionHandle> {
        let session_name = format!("wagner_{}", name);
        let pane = PaneHandle(format!("{}:0.0", session_name), "main".to_string());
        self.sessions
            .lock()
            .unwrap()
            .insert(session_name.clone(), vec![pane]);
        Ok(SessionHandle(session_name))
    }

    fn create_pane(&self, session: &SessionHandle, _cwd: &Path) -> Result<PaneHandle> {
        let mut sessions = self.sessions.lock().unwrap();
        let panes = sessions.entry(session.0.clone()).or_default();
        let pane_id = format!("{}:0.{}", session.0, panes.len());
        let pane = PaneHandle(pane_id, "pane".to_string());
        panes.push(pane.clone());
        Ok(pane)
    }

    fn capture(&self, _pane: &PaneHandle, _lines: usize) -> Result<String> {
        Ok(String::new())
    }

    fn send_keys(&self, pane: &PaneHandle, keys: &str) -> Result<()> {
        self.sent_keys
            .lock()
            .unwrap()
            .push((pane.0.clone(), keys.to_string()));
        Ok(())
    }

    fn send_key(&self, pane: &PaneHandle, key: &str) -> Result<()> {
        self.send_keys(pane, key)
    }

    fn send_literal(&self, pane: &PaneHandle, text: &str) -> Result<()> {
        self.send_keys(pane, text)
    }

    fn attach(&self, _session: &SessionHandle) -> Result<()> {
        Ok(())
    }

    fn list_panes(&self, session: &SessionHandle) -> Result<Vec<PaneHandle>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .get(&session.0)
            .cloned()
            .unwrap_or_default())
    }

    fn kill_pane(&self, pane: &PaneHandle) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        for panes in sessions.values_mut() {
            panes.retain(|p| p.0 != pane.0);
        }
        Ok(())
    }

    fn kill_session(&self, session: &SessionHandle) -> Result<()> {
        self.sessions.lock().unwrap().remove(&session.0);
        Ok(())
    }

    fn session_exists(&self, name: &str) -> Result<bool> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .contains_key(&format!("wagner_{}", name)))
    }

    fn get_pane_command(&self, _pane: &PaneHandle) -> Result<String> {
        Ok("bash".to_string())
    }
}
