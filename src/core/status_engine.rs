use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::config::MonitorConfig;
use crate::model::Task;
use crate::monitor::status::{AgentStatus, PaneStatus, SessionAggregateStatus, WaitReason};
use crate::monitor::watcher::SessionWatcher;
use crate::monitor::StatusMonitor;
use crate::terminal::{PaneHandle, SessionHandle, Terminal, session_name_for_task};
use crate::transport::CoreEvent;

pub struct StatusEngine {
    watcher: SessionWatcher,
    last_statuses: HashMap<String, PaneStatus>,
    last_session_statuses: HashMap<String, SessionAggregateStatus>,
    session_stable_since: HashMap<String, (SessionAggregateStatus, Instant)>,
    startup_time: Instant,
}

impl StatusEngine {
    pub fn new(config: &MonitorConfig) -> Self {
        let fallback = StatusMonitor::with_detectors(vec![]);
        let watcher = SessionWatcher::new(fallback, config);
        Self {
            watcher,
            last_statuses: HashMap::new(),
            last_session_statuses: HashMap::new(),
            session_stable_since: HashMap::new(),
            startup_time: Instant::now(),
        }
    }

    pub fn track_task(&mut self, task: &Task, session_name: &str) {
        self.watcher.track_task(task, session_name);
    }

    /// Poll all tracked sessions, detect status transitions with debounce.
    /// Returns debounced CoreEvents suitable for notification adapters.
    pub fn poll_transitions(
        &mut self,
        terminal: &dyn Terminal,
        tasks: &[Task],
    ) -> Vec<CoreEvent> {
        // Track any new tasks
        for task in tasks {
            let session_name = session_name_for_task(&task.name);
            self.watcher.track_task(task, &session_name);
        }

        // Poll all sessions
        let mut all_sessions: Vec<(String, Vec<PaneHandle>)> = Vec::new();
        for task in tasks {
            let session_name = session_name_for_task(&task.name);
            if terminal.session_exists(&task.name).unwrap_or(false) {
                if let Ok(panes) = terminal.list_panes(&SessionHandle(session_name.clone())) {
                    all_sessions.push((session_name, panes));
                }
            }
        }

        for (session_name, panes) in &all_sessions {
            self.watcher.poll_active(terminal, session_name, panes);
        }

        // Detect transitions and build events
        let mut events = Vec::new();

        for task in tasks {
            let session_name = session_name_for_task(&task.name);

            // Session-level debounced transitions
            if let Some(event) = self.check_session_transition(task, &session_name) {
                events.push(event);
            }

            // Pane-level transitions
            let session_panes = all_sessions
                .iter()
                .find(|(n, _)| n == &session_name)
                .map(|(_, p)| p.as_slice())
                .unwrap_or(&[]);

            for pane in session_panes {
                if let Some(event) = self.check_pane_transition(task, &session_name, pane) {
                    events.push(event);
                }
            }
        }

        events
    }

    fn check_session_transition(&mut self, task: &Task, session_name: &str) -> Option<CoreEvent> {
        let session_status = self.watcher.get_session_status(session_name);
        let last_emitted = self.last_session_statuses.get(&task.name);

        if last_emitted == Some(&session_status) {
            self.session_stable_since.remove(&task.name);
            return None;
        }

        let now = Instant::now();
        match self.session_stable_since.get(&task.name) {
            Some((pending, since)) if *pending == session_status => {
                if since.elapsed() >= Duration::from_secs(1)
                    && self.startup_time.elapsed() >= Duration::from_secs(3)
                {
                    self.last_session_statuses
                        .insert(task.name.clone(), session_status);
                    self.session_stable_since.remove(&task.name);
                    Some(CoreEvent::SessionStatusChanged {
                        task_name: task.name.clone(),
                        status: session_status,
                    })
                } else {
                    None
                }
            }
            _ => {
                self.session_stable_since
                    .insert(task.name.clone(), (session_status, now));
                None
            }
        }
    }

    fn check_pane_transition(
        &mut self,
        task: &Task,
        session_name: &str,
        pane: &PaneHandle,
    ) -> Option<CoreEvent> {
        let pane_id = &pane.0;
        let pane_title = &pane.1;

        let current = self
            .watcher
            .get_pane_status(session_name, pane_id)
            .cloned()
            .unwrap_or(PaneStatus::Unknown);

        let last = self.last_statuses.get(pane_id);
        if last == Some(&current) {
            return None;
        }

        let was_waiting = last.is_some_and(|s| s.is_waiting());
        let was_active = last.is_some_and(|s| s.is_active());
        let is_waiting = current.is_waiting();
        let is_active = current.is_active();
        let is_idle = current.is_idle();

        self.last_statuses.insert(pane_id.clone(), current.clone());

        if is_waiting && !was_waiting {
            let output_tail = self
                .watcher
                .get_pane_context(pane_id)
                .unwrap_or_default();
            let reason = match &current {
                PaneStatus::Agent {
                    status: AgentStatus::Waiting(r),
                    ..
                } => *r,
                _ => WaitReason::Approval,
            };
            Some(CoreEvent::NeedsAttention {
                task_name: task.name.clone(),
                pane_id: pane_id.clone(),
                pane_title: pane_title.clone(),
                reason,
                output_tail,
            })
        } else if is_idle && was_active {
            Some(CoreEvent::AgentIdle {
                task_name: task.name.clone(),
                pane_id: pane_id.clone(),
                pane_title: pane_title.clone(),
                output_tail: String::new(),
            })
        } else if is_active && !was_active {
            Some(CoreEvent::AgentWorking {
                task_name: task.name.clone(),
                pane_id: pane_id.clone(),
                pane_title: pane_title.clone(),
                activity: current.label(),
            })
        } else {
            None
        }
    }

    // --- Low-level access for TUI (active/background polling) ---

    pub fn poll_active(
        &mut self,
        terminal: &dyn Terminal,
        session_name: &str,
        panes: &[PaneHandle],
    ) -> Vec<crate::monitor::StatusUpdate> {
        self.watcher.poll_active(terminal, session_name, panes)
    }

    pub fn poll_background(
        &mut self,
        terminal: &dyn Terminal,
        sessions: &[(String, Vec<PaneHandle>)],
        active_session: Option<&str>,
    ) {
        self.watcher
            .poll_background(terminal, sessions, active_session);
    }

    // --- Getters (raw current state, no debounce) ---

    pub fn get_pane_status(&self, session_name: &str, pane_id: &str) -> Option<&PaneStatus> {
        self.watcher.get_pane_status(session_name, pane_id)
    }

    pub fn get_session_status(&self, session_name: &str) -> SessionAggregateStatus {
        self.watcher.get_session_status(session_name)
    }

    pub fn get_pane_context(&self, pane_id: &str) -> Option<String> {
        self.watcher.get_pane_context(pane_id)
    }
}
