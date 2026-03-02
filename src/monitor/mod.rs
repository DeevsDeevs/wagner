mod ansi;
pub mod claude_events;
pub mod codex_events;
pub mod deriver;
mod detector;
mod detectors;
pub mod events;
pub mod status;
pub mod watcher;

use std::collections::HashMap;
use std::time::{Duration, Instant};

const STATUS_HYSTERESIS: Duration = Duration::from_millis(500);

use sha2::{Digest, Sha256};

use crate::terminal::{PaneHandle, Terminal};

pub use ansi::strip_ansi;
pub use detector::{AgentDetector, IDLE_THRESHOLD};
pub use detectors::TerminalDetector;
pub use status::{
    Activity, ActivityKind, AgentStatus, AgentType, ClaudeActivity, CodexActivity, PaneStatus,
    STUCK_THRESHOLD, SessionAggregateStatus, TerminalStatus, TrackedPane, WaitReason,
};
pub use watcher::SessionWatcher;

struct TrackedSession {
    panes: HashMap<String, TrackedPane>,
    last_poll: Instant,
}

impl TrackedSession {
    fn new() -> Self {
        Self {
            panes: HashMap::new(),
            last_poll: Instant::now(),
        }
    }

    fn aggregate(&self) -> SessionAggregateStatus {
        let panes: Vec<_> = self.panes.values().cloned().collect();
        SessionAggregateStatus::from_panes(&panes)
    }
}

pub struct StatusMonitor {
    detectors: Vec<Box<dyn AgentDetector>>,
    sessions: HashMap<String, TrackedSession>,
    background_interval: Duration,
    background_index: usize,
}

impl StatusMonitor {
    pub fn new(detector: Box<dyn AgentDetector>) -> Self {
        Self::with_detectors(vec![detector])
    }

    pub fn with_detectors(detectors: Vec<Box<dyn AgentDetector>>) -> Self {
        Self {
            detectors,
            sessions: HashMap::new(),
            background_interval: Duration::from_secs(2),
            background_index: 0,
        }
    }

    pub fn with_background_interval(mut self, interval: Duration) -> Self {
        self.background_interval = interval;
        self
    }

    pub fn poll_active(
        &mut self,
        terminal: &dyn Terminal,
        session_name: &str,
        panes: &[PaneHandle],
    ) -> Vec<StatusUpdate> {
        self.poll_session(terminal, session_name, panes)
    }

    pub fn poll_background(
        &mut self,
        terminal: &dyn Terminal,
        sessions: &[(String, Vec<PaneHandle>)],
        active_session: Option<&str>,
    ) {
        if sessions.is_empty() {
            return;
        }

        let now = Instant::now();
        let background: Vec<_> = sessions
            .iter()
            .filter(|(name, _)| active_session.is_none_or(|a| a != name))
            .collect();

        if background.is_empty() {
            return;
        }

        self.background_index %= background.len();
        let (session_name, panes) = &background[self.background_index];

        let session = self
            .sessions
            .entry(session_name.clone())
            .or_insert_with(TrackedSession::new);
        if now.duration_since(session.last_poll) >= self.background_interval {
            self.poll_session(terminal, session_name, panes);
        }
        self.background_index = (self.background_index + 1) % background.len();
    }

    fn poll_session(
        &mut self,
        terminal: &dyn Terminal,
        session_name: &str,
        panes: &[PaneHandle],
    ) -> Vec<StatusUpdate> {
        let mut updates = vec![];

        self.sessions
            .entry(session_name.to_string())
            .or_insert_with(TrackedSession::new);

        let captures: Vec<_> = panes
            .iter()
            .filter_map(|pane| {
                let output = terminal.capture(pane, 100).ok()?;
                let command = terminal.get_pane_command(pane).unwrap_or_default();
                Some((pane.clone(), output, command))
            })
            .collect();

        for (pane, output, pane_command) in captures {
            let pane_id = pane.0.clone();
            let clean_output = strip_ansi(&output);
            let hash = Self::hash(&clean_output);

            let (output_changed, since_change, current_agent, current_status) = {
                let session = self.sessions.get_mut(session_name).unwrap();
                session.last_poll = Instant::now();

                let tracked = session
                    .panes
                    .entry(pane_id.clone())
                    .or_insert_with(|| TrackedPane::new(pane.clone()));

                let is_first_poll = tracked.output_hash == [0u8; 32];
                let output_changed = hash != tracked.output_hash;
                if output_changed {
                    tracked.output_hash = hash;
                    if !is_first_poll {
                        tracked.last_change = Instant::now();
                    }
                }

                (
                    output_changed && !is_first_poll,
                    tracked.last_change.elapsed(),
                    tracked.agent_type,
                    tracked.status.clone(),
                )
            };

            let detected_agent = self.detect_agent(&pane_command, &clean_output);
            let agent_type = match (current_agent, detected_agent) {
                (Some(current), Some(detected)) if current != detected => Some(detected),
                (Some(current), _) => Some(current),
                (None, Some(detected)) => Some(detected),
                (None, None) => None,
            };
            let mut new_status = self.detect_status(
                agent_type.as_ref(),
                &output,
                &clean_output,
                output_changed,
                since_change,
            );

            let session = self.sessions.get_mut(session_name).unwrap();
            let tracked = session.panes.get_mut(&pane_id).unwrap();
            tracked.agent_type = agent_type;

            if new_status.is_active() && since_change > STUCK_THRESHOLD {
                new_status = match &agent_type {
                    Some(at) => PaneStatus::Agent {
                        agent_type: *at,
                        status: AgentStatus::Idle,
                    },
                    None => PaneStatus::Terminal(TerminalStatus::Idle),
                };
            }

            let dominated_by_hysteresis = current_status.is_active()
                && new_status.is_idle()
                && since_change < STATUS_HYSTERESIS;

            if new_status != current_status && !dominated_by_hysteresis {
                tracked.status = new_status.clone();
                updates.push(StatusUpdate {
                    pane: pane.clone(),
                    status: new_status,
                });
            }
        }

        if let Some(session) = self.sessions.get_mut(session_name) {
            session
                .panes
                .retain(|id, _| panes.iter().any(|p| p.0 == *id));
        }
        updates
    }

    pub fn get_session_status(&self, session_name: &str) -> SessionAggregateStatus {
        self.sessions
            .get(session_name)
            .map(|s| s.aggregate())
            .unwrap_or(SessionAggregateStatus::Empty)
    }

    pub fn get_pane_status(&self, session_name: &str, pane_id: &str) -> Option<&PaneStatus> {
        self.sessions
            .get(session_name)
            .and_then(|s| s.panes.get(pane_id))
            .map(|t| &t.status)
    }

    fn detect_agent(&self, pane_command: &str, output: &str) -> Option<AgentType> {
        let command = pane_command.to_ascii_lowercase();
        if command.contains("codex") {
            return Some(AgentType::Codex);
        }
        if command.contains("claude") {
            return Some(AgentType::ClaudeCode);
        }
        self.detectors
            .iter()
            .find(|d| d.detect_agent(pane_command, output))
            .map(|d| d.agent_type())
    }

    fn detect_status(
        &self,
        agent_type: Option<&AgentType>,
        raw_output: &str,
        clean_output: &str,
        output_changed: bool,
        since_change: Duration,
    ) -> PaneStatus {
        match agent_type {
            Some(at) => {
                let detector = self.detectors.iter().find(|d| &d.agent_type() == at);
                match detector {
                    Some(d) => PaneStatus::Agent {
                        agent_type: *at,
                        status: d.detect_status(
                            raw_output,
                            clean_output,
                            output_changed,
                            since_change,
                        ),
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

#[derive(Debug, Clone)]
pub struct StatusUpdate {
    pub pane: PaneHandle,
    pub status: PaneStatus,
}
