use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::config::MonitorConfig;
use crate::model::Task;
use crate::monitor::StatusMonitor;
use crate::monitor::status::{AgentStatus, PaneStatus, SessionAggregateStatus, WaitReason};
use crate::monitor::watcher::SessionWatcher;
use crate::terminal::{PaneHandle, SessionHandle, Terminal, session_name_for_task};
use crate::transport::{CoreEvent, ProgressStep};

pub struct StatusEngine {
    watcher: SessionWatcher,
    last_statuses: HashMap<String, PaneStatus>,
    last_session_statuses: HashMap<String, SessionAggregateStatus>,
    session_stable_since: HashMap<String, (SessionAggregateStatus, Instant)>,
    last_action_seqs: HashMap<String, u64>,
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
            last_action_seqs: HashMap::new(),
            startup_time: Instant::now(),
        }
    }

    pub fn track_task(&mut self, task: &Task, session_name: &str) {
        self.watcher.track_task(task, session_name);
    }

    pub fn poll_transitions(&mut self, terminal: &dyn Terminal, tasks: &[Task]) -> Vec<CoreEvent> {
        for task in tasks {
            let session_name = session_name_for_task(&task.name);
            self.watcher.track_task(task, &session_name);
        }

        let mut all_sessions: Vec<(String, Vec<PaneHandle>)> = Vec::new();
        for task in tasks {
            let session_name = session_name_for_task(&task.name);
            if terminal.session_exists(&task.name).unwrap_or(false)
                && let Ok(panes) = terminal.list_panes(&SessionHandle(session_name.clone()))
            {
                all_sessions.push((session_name, panes));
            }
        }

        for (session_name, panes) in &all_sessions {
            self.watcher.poll_active(terminal, session_name, panes);
        }

        let mut events = Vec::new();

        for task in tasks {
            let session_name = session_name_for_task(&task.name);

            if let Some(event) = self.check_session_transition(task, &session_name) {
                events.push(event);
            }

            let session_panes = all_sessions
                .iter()
                .find(|(n, _)| n == &session_name)
                .map(|(_, p)| p.as_slice())
                .unwrap_or(&[]);

            for pane in session_panes {
                if let Some(event) = self.check_pane_transition(task, &session_name, pane) {
                    events.push(event);
                }
                if let Some(event) = self.check_pane_progress(task, &session_name, pane) {
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
                    let panes: Vec<(String, String)> = task
                        .panes
                        .iter()
                        .filter_map(|tp| {
                            let status = self.watcher.get_pane_status(session_name, &tp.pane_id)?;
                            Some((tp.name.clone(), status.label()))
                        })
                        .collect();
                    Some(CoreEvent::SessionStatusChanged {
                        task_name: task.name.clone(),
                        status: session_status,
                        panes,
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

        let last = self.last_statuses.get(pane_id).cloned();
        if last.as_ref() == Some(&current) {
            // Status unchanged — but the agent may have gone Idle→Active→Idle
            // within a single poll cycle. Check for a pending response.
            if current.is_idle()
                && let Some(response) = self.watcher.take_pane_response(pane_id)
                && !response.is_empty()
            {
                let pane_name = task
                    .panes
                    .iter()
                    .find(|tp| tp.pane_id == *pane_id)
                    .map(|tp| tp.name.clone())
                    .unwrap_or_else(|| pane_title.clone());
                return Some(CoreEvent::AgentIdle {
                    task_name: task.name.clone(),
                    pane_name,
                    pane_id: pane_id.clone(),
                    output_tail: String::new(),
                    response_text: Some(response),
                });
            }
            return None;
        }

        let was_waiting = last.as_ref().is_some_and(|s| s.is_waiting());
        let was_active = last.as_ref().is_some_and(|s| s.is_active());
        let is_waiting = current.is_waiting();
        let is_active = current.is_active();
        let is_idle = current.is_idle();

        self.last_statuses.insert(pane_id.clone(), current.clone());

        let pane_name = task
            .panes
            .iter()
            .find(|tp| tp.pane_id == *pane_id)
            .map(|tp| tp.name.clone())
            .unwrap_or_else(|| pane_title.clone());

        if is_waiting && !was_waiting {
            let output_tail = self.watcher.get_pane_context(pane_id).unwrap_or_default();
            let reason = match &current {
                PaneStatus::Agent {
                    status: AgentStatus::Waiting(r),
                    ..
                } => *r,
                _ => WaitReason::Approval,
            };
            let question_data = if reason == WaitReason::Question {
                self.watcher.get_pane_question_data(pane_id)
            } else {
                None
            };
            Some(CoreEvent::NeedsAttention {
                task_name: task.name.clone(),
                pane_name,
                pane_id: pane_id.clone(),
                reason,
                output_tail,
                question_data,
            })
        } else if is_idle && (was_active || was_waiting) {
            let response_text = self.watcher.take_pane_response(pane_id);
            Some(CoreEvent::AgentIdle {
                task_name: task.name.clone(),
                pane_name,
                pane_id: pane_id.clone(),
                output_tail: String::new(),
                response_text,
            })
        } else if is_active && !was_active {
            Some(CoreEvent::AgentWorking {
                task_name: task.name.clone(),
                pane_name,
                pane_id: pane_id.clone(),
                activity: current.label(),
            })
        } else {
            None
        }
    }

    fn check_pane_progress(
        &mut self,
        task: &Task,
        _session_name: &str,
        pane: &PaneHandle,
    ) -> Option<CoreEvent> {
        let pane_id = &pane.0;
        let action_seq = self.watcher.get_pane_action_seq(pane_id);
        let last_seq = self.last_action_seqs.get(pane_id).copied().unwrap_or(0);

        if action_seq == last_seq {
            return None;
        }

        self.last_action_seqs.insert(pane_id.clone(), action_seq);

        let completed = self.watcher.get_pane_completed_steps(pane_id);
        let pending = self.watcher.get_pane_pending_tool(pane_id);

        let steps: Vec<ProgressStep> = completed
            .iter()
            .map(|s| ProgressStep {
                tool_name: s.tool_name.clone(),
                context: s.context.clone(),
                done: true,
                ok: s.ok,
            })
            .collect();

        let pending_step = pending.map(|(name, ctx)| ProgressStep {
            tool_name: name,
            context: ctx,
            done: false,
            ok: true,
        });

        let step_count = steps.len() + if pending_step.is_some() { 1 } else { 0 };

        let pane_name = task
            .panes
            .iter()
            .find(|tp| tp.pane_id == *pane_id)
            .map(|tp| tp.name.clone())
            .unwrap_or_else(|| pane.1.clone());

        Some(CoreEvent::AgentProgress {
            task_name: task.name.clone(),
            pane_name,
            pane_id: pane_id.clone(),
            steps,
            pending: pending_step,
            step_count,
        })
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

    pub fn get_pane_question_data(
        &self,
        pane_id: &str,
    ) -> Option<Vec<crate::monitor::events::QuestionData>> {
        self.watcher.get_pane_question_data(pane_id)
    }
}
