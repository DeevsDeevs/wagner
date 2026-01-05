mod ansi;
mod detector;
mod detectors;
pub mod status;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::terminal::{PaneHandle, Terminal};

pub use ansi::strip_ansi;
pub use detector::AgentDetector;
pub use detectors::{ClaudeCodeDetector, TerminalDetector};
pub use status::{
    Activity, ActivityKind, AgentStatus, AgentType, ClaudeActivity, PaneStatus,
    SessionAggregateStatus, TerminalStatus, TrackedPane, WaitReason,
};

pub struct StatusMonitor {
    detectors: Vec<Box<dyn AgentDetector>>,
    tracked_panes: HashMap<String, TrackedPane>,
    idle_threshold: Duration,
}

impl StatusMonitor {
    pub fn new() -> Self {
        Self {
            detectors: vec![Box::new(ClaudeCodeDetector::new())],
            tracked_panes: HashMap::new(),
            idle_threshold: Duration::from_secs(2),
        }
    }

    pub fn with_idle_threshold(mut self, threshold: Duration) -> Self {
        self.idle_threshold = threshold;
        self
    }

    pub fn poll<T: Terminal>(&mut self, terminal: &T, panes: &[PaneHandle]) -> Vec<StatusUpdate> {
        let mut updates = vec![];

        for pane in panes {
            let pane_id = pane.0.clone();

            let output = match terminal.capture(pane, 100) {
                Ok(o) => o,
                Err(_) => continue,
            };

            let pane_command = terminal.get_pane_command(pane).unwrap_or_default();

            let clean_output = strip_ansi(&output);
            let hash = Self::hash(&clean_output);

            let (output_changed, since_change, current_agent, current_status) = {
                let tracked = self
                    .tracked_panes
                    .entry(pane_id.clone())
                    .or_insert_with(|| TrackedPane::new(pane.clone()));

                let output_changed = hash != tracked.output_hash;
                if output_changed {
                    tracked.output_hash = hash;
                    tracked.last_change = Instant::now();
                }

                (
                    output_changed,
                    tracked.last_change.elapsed(),
                    tracked.agent_type.clone(),
                    tracked.status.clone(),
                )
            };

            let agent_type = match current_agent {
                Some(at) => Some(at),
                None => self.detect_agent(&pane_command, &clean_output),
            };

            let new_status = self.detect_status(
                agent_type.as_ref(),
                &clean_output,
                output_changed,
                since_change,
            );

            let tracked = self.tracked_panes.get_mut(&pane_id).unwrap();
            tracked.agent_type = agent_type;

            if new_status != current_status {
                tracked.status = new_status.clone();
                updates.push(StatusUpdate {
                    pane: pane.clone(),
                    status: new_status,
                });
            }
        }

        self.tracked_panes
            .retain(|id, _| panes.iter().any(|p| p.0 == *id));

        updates
    }

    pub fn get_pane_status(&self, pane: &PaneHandle) -> Option<&PaneStatus> {
        self.tracked_panes.get(&pane.0).map(|t| &t.status)
    }

    pub fn get_tracked_panes(&self) -> Vec<&TrackedPane> {
        self.tracked_panes.values().collect()
    }

    pub fn aggregate_status(&self) -> SessionAggregateStatus {
        let panes: Vec<_> = self.tracked_panes.values().cloned().collect();
        SessionAggregateStatus::from_panes(&panes)
    }

    fn detect_agent(&self, pane_command: &str, output: &str) -> Option<AgentType> {
        for detector in &self.detectors {
            if detector.detect_agent(pane_command, output) {
                return Some(detector.agent_type());
            }
        }
        None
    }

    fn detect_status(
        &self,
        agent_type: Option<&AgentType>,
        output: &str,
        output_changed: bool,
        since_change: Duration,
    ) -> PaneStatus {
        match agent_type {
            Some(at) => {
                let detector = self.detectors.iter().find(|d| &d.agent_type() == at);
                match detector {
                    Some(d) => PaneStatus::Agent {
                        agent_type: at.clone(),
                        status: d.detect_status(output, output_changed, since_change),
                    },
                    None => TerminalDetector::detect_status(output_changed, since_change),
                }
            }
            None => TerminalDetector::detect_status(output_changed, since_change),
        }
    }

    fn hash(content: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        hasher.finalize().into()
    }
}

impl Default for StatusMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct StatusUpdate {
    pub pane: PaneHandle,
    pub status: PaneStatus,
}
