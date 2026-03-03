use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config::Config;
use crate::transport::PaneOutputMode;

#[derive(Debug, Serialize, Deserialize)]
pub struct SerializableFocus {
    pub task_name: String,
    pub pane_name: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AdapterState {
    pub telegram_offset: i32,
    pub pane_modes: HashMap<String, PaneOutputMode>,
    pub focus: Option<SerializableFocus>,
    pub entity_registry: HashMap<u16, (String, String)>,
    pub task_registry: HashMap<u16, String>,
    pub next_entity_id: u16,
    pub next_task_id: u16,
    pub message_to_pane: Vec<(i32, (String, String))>,
}

fn state_path() -> PathBuf {
    Config::config_dir().join("telegram_state.json")
}

impl AdapterState {
    pub fn load() -> Self {
        let path = state_path();
        match std::fs::read_to_string(&path) {
            Ok(data) => match serde_json::from_str(&data) {
                Ok(state) => {
                    info!("loaded telegram state from {}", path.display());
                    state
                }
                Err(e) => {
                    warn!(%e, "failed to parse telegram state, starting fresh");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = state_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    warn!(%e, "failed to write telegram state");
                }
            }
            Err(e) => {
                warn!(%e, "failed to serialize telegram state");
            }
        }
    }

    pub fn rebuild_entity_reverse(&self) -> HashMap<(String, String), u16> {
        self.entity_registry
            .iter()
            .map(|(&id, pair)| (pair.clone(), id))
            .collect()
    }

    pub fn rebuild_task_reverse(&self) -> HashMap<String, u16> {
        self.task_registry
            .iter()
            .map(|(&id, name)| (name.clone(), id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_roundtrips() {
        let state = AdapterState::default();
        let json = serde_json::to_string(&state).unwrap();
        let back: AdapterState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.telegram_offset, 0);
        assert_eq!(back.next_entity_id, 0);
        assert!(back.pane_modes.is_empty());
        assert!(back.focus.is_none());
    }

    #[test]
    fn state_with_data_roundtrips() {
        let mut state = AdapterState::default();
        state.telegram_offset = 42;
        state.next_entity_id = 5;
        state.next_task_id = 3;
        state.pane_modes.insert("k".into(), PaneOutputMode::Stream);
        state
            .entity_registry
            .insert(1, ("task".into(), "pane".into()));
        state.task_registry.insert(1, "my-task".into());
        state
            .message_to_pane
            .push((100, ("task".into(), "pane".into())));
        state.focus = Some(SerializableFocus {
            task_name: "t".into(),
            pane_name: Some("p".into()),
        });

        let json = serde_json::to_string(&state).unwrap();
        let back: AdapterState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.telegram_offset, 42);
        assert_eq!(back.next_entity_id, 5);
        assert_eq!(back.next_task_id, 3);
        assert_eq!(back.message_to_pane.len(), 1);
        assert!(back.focus.is_some());

        let entity_rev = back.rebuild_entity_reverse();
        assert_eq!(entity_rev.get(&("task".into(), "pane".into())), Some(&1));
        let task_rev = back.rebuild_task_reverse();
        assert_eq!(task_rev.get("my-task"), Some(&1));
    }
}
