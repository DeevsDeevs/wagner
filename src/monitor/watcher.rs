use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::MonitorConfig;
use crate::model::{Engine, PENDING_DISCOVERY, Task, TrackedPane};
use crate::terminal::{PaneHandle, Terminal};

use super::StatusMonitor;
use super::StatusUpdate;
use super::claude_events::parse_claude_event;
use super::codex_events::parse_codex_event;
use super::droid_events::parse_droid_event;
use super::deriver::StatusDeriver;
use super::status::{PaneStatus, SessionAggregateStatus};

pub struct SessionWatcher {
    pane_watchers: HashMap<String, PaneWatcher>,
    fallback: StatusMonitor,
    session_panes: HashMap<String, Vec<String>>,
    pane_statuses: HashMap<String, PaneStatus>,
    max_lines_per_poll: usize,
    approval_timeout: Duration,
    idle_threshold: Duration,
    path_updates: Vec<(String, PathBuf)>,
}

struct PaneWatcher {
    engine: Engine,
    jsonl_path: PathBuf,
    project_dir: Option<PathBuf>,
    deriver: StatusDeriver,
    file_offset: u64,
    last_status: PaneStatus,
    last_data_at: Instant,
    session_check_interval: Duration,
    path_changed: bool,
}

impl PaneWatcher {
    fn new(
        engine: Engine,
        jsonl_path: PathBuf,
        approval_timeout: Duration,
        idle_threshold: Duration,
    ) -> Self {
        let deriver = StatusDeriver::new(engine)
            .with_approval_timeout(approval_timeout)
            .with_idle_threshold(idle_threshold);

        let project_dir = if engine == Engine::ClaudeCode
            && jsonl_path.as_os_str() != PENDING_DISCOVERY
        {
            jsonl_path.parent().map(PathBuf::from)
        } else {
            None
        };

        let now = Instant::now();
        Self {
            engine,
            jsonl_path,
            project_dir,
            deriver,
            file_offset: 0,
            last_status: PaneStatus::Unknown,
            last_data_at: now,
            session_check_interval: Duration::from_secs(10),
            path_changed: false,
        }
    }

    fn poll(&mut self, max_lines: usize) -> Option<PaneStatus> {
        if self.jsonl_path.as_os_str() == PENDING_DISCOVERY {
            let status = self.deriver.tick();
            return self.maybe_update(status);
        }

        let file_len = match std::fs::metadata(&self.jsonl_path) {
            Ok(meta) => meta.len(),
            Err(_) => {
                let status = self.deriver.tick();
                return self.maybe_update(status);
            }
        };

        if file_len < self.file_offset {
            self.file_offset = 0;
            self.deriver.reset();
        }

        let was_initial = self.file_offset == 0;
        let offset_before = self.file_offset;
        if file_len > self.file_offset {
            self.read_new_lines(max_lines);
        }

        let got_new_data = self.file_offset > offset_before;
        if got_new_data {
            self.last_data_at = Instant::now();
        } else if self.project_dir.is_some()
            && self.last_data_at.elapsed() > self.session_check_interval
            && !self.last_status.is_active()
            && !self.last_status.is_waiting()
        {
            self.try_discover_newer_jsonl();
            self.last_data_at = Instant::now();
        }

        // After initial read of existing JSONL, discard stale response/progress
        // to avoid emitting old responses on daemon restart or path resolution.
        if was_initial && self.file_offset > 0 {
            self.deriver.take_response_text();
            self.deriver.clear_steps();
        }

        let status = self.deriver.tick();
        self.maybe_update(status)
    }

    fn try_discover_newer_jsonl(&mut self) {
        let Some(dir) = self.project_dir.as_ref() else {
            return;
        };

        let current_mtime = std::fs::metadata(&self.jsonl_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

        // If the current file was recently modified, the agent is still active —
        // don't scan for a replacement even if we haven't read new data yet.
        if current_mtime
            .elapsed()
            .is_ok_and(|e| e < self.session_check_interval)
        {
            return;
        }

        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if meta.len() == 0 {
                    continue;
                }
                let mtime = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                if newest.as_ref().is_none_or(|(_, t)| mtime > *t) {
                    newest = Some((path, mtime));
                }
            }
        }

        if let Some((path, new_mtime)) = newest {
            // Only swap if the candidate is a different file, has a newer mtime,
            // AND was modified very recently (actively being written to by a new session).
            // This prevents swapping when the agent is just idle but still alive.
            let candidate_is_active = new_mtime
                .elapsed()
                .is_ok_and(|e| e < self.session_check_interval);

            if path != self.jsonl_path && new_mtime > current_mtime && candidate_is_active {
                tracing::info!(
                    old = %self.jsonl_path.display(),
                    new = %path.display(),
                    "detected newer JSONL session, hot-swapping watcher"
                );
                self.jsonl_path = path;
                self.file_offset = 0;
                self.deriver.reset();
                self.last_data_at = Instant::now();
                self.path_changed = true;
            }
        }
    }

    fn read_new_lines(&mut self, max_lines: usize) {
        let Ok(mut file) = File::open(&self.jsonl_path) else {
            return;
        };
        if file.seek(SeekFrom::Start(self.file_offset)).is_err() {
            return;
        }

        let mut reader = BufReader::new(file);
        let mut line_buf = String::new();
        let mut lines_processed = 0;

        while lines_processed < max_lines {
            line_buf.clear();
            let Ok(bytes_read) = reader.read_line(&mut line_buf) else {
                break;
            };
            if bytes_read == 0 {
                break;
            }

            // Skip partial lines (file still being written)
            if !line_buf.ends_with('\n') {
                break;
            }

            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                self.file_offset += bytes_read as u64;
                continue;
            }

            let event = match self.engine {
                Engine::ClaudeCode => parse_claude_event(trimmed),
                Engine::Codex => parse_codex_event(trimmed),
                Engine::Droid => parse_droid_event(trimmed),
                Engine::Terminal => None,
            };

            if let Some(event) = event {
                self.deriver.process(&event);
            }
            lines_processed += 1;
            self.file_offset += bytes_read as u64;
        }
    }

    fn maybe_update(&mut self, status: PaneStatus) -> Option<PaneStatus> {
        if status != self.last_status {
            self.last_status = status;
            Some(self.last_status.clone())
        } else {
            None
        }
    }
}

fn resolve_jsonl_path(tracked: &TrackedPane, task: &Task) -> PathBuf {
    if !tracked.is_discovery_pending() || tracked.engine != Engine::ClaudeCode {
        return tracked.jsonl_path.clone();
    }

    let repo = task.repos.iter().find(|r| r.name == tracked.repo_name);
    if let Some(repo) = repo {
        let project_id = repo.worktree.to_string_lossy().replace(['/', '.'], "-");
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".claude")
                .join("projects")
                .join(project_id)
                .join(format!("{}.jsonl", tracked.session_id));
        }
    }

    tracked.jsonl_path.clone()
}

impl SessionWatcher {
    pub fn new(fallback: StatusMonitor, config: &MonitorConfig) -> Self {
        Self {
            pane_watchers: HashMap::new(),
            fallback,
            session_panes: HashMap::new(),
            pane_statuses: HashMap::new(),
            max_lines_per_poll: config.max_lines_per_poll,
            approval_timeout: Duration::from_millis(config.approval_timeout_ms),
            idle_threshold: Duration::from_millis(config.idle_threshold_ms),
            path_updates: Vec::new(),
        }
    }

    pub fn track_task(&mut self, task: &Task, _session_name: &str) {
        for tracked in &task.panes {
            if let Some(watcher) = self.pane_watchers.get_mut(&tracked.pane_id) {
                if watcher.jsonl_path.as_os_str() == PENDING_DISCOVERY {
                    let resolved = resolve_jsonl_path(tracked, task);
                    if resolved.as_os_str() != PENDING_DISCOVERY {
                        watcher.jsonl_path = resolved;
                        if watcher.project_dir.is_none()
                            && tracked.engine == Engine::ClaudeCode
                        {
                            watcher.project_dir =
                                watcher.jsonl_path.parent().map(PathBuf::from);
                        }
                    }
                }
            } else {
                let jsonl_path = resolve_jsonl_path(tracked, task);
                let watcher = PaneWatcher::new(
                    tracked.engine,
                    jsonl_path,
                    self.approval_timeout,
                    self.idle_threshold,
                );
                self.pane_watchers.insert(tracked.pane_id.clone(), watcher);
            }
        }
    }

    pub fn poll_active(
        &mut self,
        terminal: &dyn Terminal,
        session_name: &str,
        panes: &[PaneHandle],
    ) -> Vec<StatusUpdate> {
        let mut updates = Vec::new();

        self.session_panes.insert(
            session_name.to_string(),
            panes.iter().map(|p| p.0.clone()).collect(),
        );

        let mut untracked_panes = Vec::new();

        for pane in panes {
            if let Some(watcher) = self.pane_watchers.get_mut(&pane.0) {
                if watcher.jsonl_path.as_os_str() == PENDING_DISCOVERY {
                    untracked_panes.push(pane.clone());
                } else {
                    if let Some(new_status) = watcher.poll(self.max_lines_per_poll) {
                        self.pane_statuses
                            .insert(pane.0.clone(), new_status.clone());
                        updates.push(StatusUpdate {
                            pane: pane.clone(),
                            status: new_status,
                        });
                    }
                    Self::collect_path_change(
                        &mut self.path_updates,
                        &pane.0,
                        watcher,
                    );
                }
            } else {
                untracked_panes.push(pane.clone());
            }
        }

        if !untracked_panes.is_empty() {
            let fallback_updates =
                self.fallback
                    .poll_active(terminal, session_name, &untracked_panes);
            for update in fallback_updates {
                self.pane_statuses
                    .insert(update.pane.0.clone(), update.status.clone());
                updates.push(update);
            }
        }

        updates
    }

    pub fn poll_background(
        &mut self,
        terminal: &dyn Terminal,
        sessions: &[(String, Vec<PaneHandle>)],
        active_session: Option<&str>,
    ) {
        let mut untracked_sessions: Vec<(String, Vec<PaneHandle>)> = Vec::new();

        for (session_name, panes) in sessions {
            if active_session.is_some_and(|a| a == session_name) {
                continue;
            }

            self.session_panes.insert(
                session_name.clone(),
                panes.iter().map(|p| p.0.clone()).collect(),
            );

            let mut untracked_panes = Vec::new();
            for pane in panes {
                if let Some(watcher) = self.pane_watchers.get_mut(&pane.0) {
                    if let Some(new_status) = watcher.poll(self.max_lines_per_poll) {
                        self.pane_statuses.insert(pane.0.clone(), new_status);
                    }
                    Self::collect_path_change(
                        &mut self.path_updates,
                        &pane.0,
                        watcher,
                    );
                } else {
                    untracked_panes.push(pane.clone());
                }
            }
            if !untracked_panes.is_empty() {
                untracked_sessions.push((session_name.clone(), untracked_panes));
            }
        }

        if !untracked_sessions.is_empty() {
            self.fallback
                .poll_background(terminal, &untracked_sessions, active_session);
        }
    }

    pub fn get_session_status(&self, session_name: &str) -> SessionAggregateStatus {
        match self.session_panes.get(session_name) {
            Some(pane_ids) if !pane_ids.is_empty() => {
                let get_status = |id: &String| -> Option<&PaneStatus> {
                    self.pane_statuses
                        .get(id)
                        .or_else(|| self.fallback.get_pane_status(session_name, id))
                };

                let has_waiting = pane_ids
                    .iter()
                    .any(|id| get_status(id).is_some_and(|s| s.is_waiting()));
                let has_active = pane_ids
                    .iter()
                    .any(|id| get_status(id).is_some_and(|s| s.is_active()));

                if has_waiting {
                    SessionAggregateStatus::NeedsAttention
                } else if has_active {
                    SessionAggregateStatus::Working
                } else {
                    SessionAggregateStatus::Idle
                }
            }
            _ => self.fallback.get_session_status(session_name),
        }
    }

    pub fn get_pane_context(&self, pane_id: &str) -> Option<String> {
        self.pane_watchers
            .get(pane_id)?
            .deriver
            .last_context()
            .map(String::from)
    }

    pub fn get_pane_action_seq(&self, pane_id: &str) -> u64 {
        self.pane_watchers
            .get(pane_id)
            .map(|w| w.deriver.action_seq())
            .unwrap_or(0)
    }

    pub fn get_pane_completed_steps(&self, pane_id: &str) -> Vec<super::deriver::CompletedStep> {
        self.pane_watchers
            .get(pane_id)
            .map(|w| w.deriver.completed_steps().to_vec())
            .unwrap_or_default()
    }

    pub fn get_pane_pending_tool(&self, pane_id: &str) -> Option<(String, Option<String>)> {
        self.pane_watchers
            .get(pane_id)?
            .deriver
            .pending_tool_info()
            .map(|(name, ctx)| (name.to_string(), ctx.map(String::from)))
    }

    pub fn get_pane_question_data(
        &self,
        pane_id: &str,
    ) -> Option<Vec<super::events::QuestionData>> {
        self.pane_watchers
            .get(pane_id)?
            .deriver
            .pending_question_data()
            .map(|s| s.to_vec())
    }

    pub fn take_pane_response(&mut self, pane_id: &str) -> Option<String> {
        self.pane_watchers
            .get_mut(pane_id)?
            .deriver
            .take_response_text()
    }

    pub fn get_pane_status(&self, session_name: &str, pane_id: &str) -> Option<&PaneStatus> {
        self.pane_statuses
            .get(pane_id)
            .or_else(|| self.fallback.get_pane_status(session_name, pane_id))
    }

    pub fn take_path_updates(&mut self) -> Vec<(String, PathBuf)> {
        std::mem::take(&mut self.path_updates)
    }

    fn collect_path_change(
        path_updates: &mut Vec<(String, PathBuf)>,
        pane_id: &str,
        watcher: &mut PaneWatcher,
    ) {
        if watcher.path_changed {
            path_updates.push((pane_id.to_string(), watcher.jsonl_path.clone()));
            watcher.path_changed = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackedPane as ModelTrackedPane;
    use crate::model::{RepoSource, TaskKind, TaskRepo};
    use chrono::Utc;
    use std::io::Write;

    fn test_config() -> MonitorConfig {
        MonitorConfig {
            approval_timeout_ms: 50,
            idle_threshold_ms: 100,
            max_lines_per_poll: 1000,
            ..Default::default()
        }
    }

    fn make_task_with_pane(pane_id: &str, jsonl_path: PathBuf) -> Task {
        Task {
            name: "test-task".into(),
            path: PathBuf::from("/tmp/test"),
            repos: vec![TaskRepo {
                name: "repo1".into(),
                source: RepoSource::Local(PathBuf::from("/tmp/repo1")),
                worktree: PathBuf::from("/tmp/test/repo1"),
                branch: "test".into(),
            }],
            created_at: Utc::now(),
            diff_base: None,
            kind: TaskKind::Managed,
            panes: vec![ModelTrackedPane {
                name: "repo1".into(),
                repo_name: "repo1".into(),
                engine: Engine::ClaudeCode,
                session_id: "abc-123".into(),
                pane_id: pane_id.into(),
                jsonl_path,
                launched_at: Utc::now(),
            }],
        }
    }

    #[test]
    fn pane_watcher_reads_claude_events() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"user","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{}}}}]}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        let status = watcher.poll(1000);
        assert!(status.is_some());
        assert!(status.unwrap().is_active());
    }

    #[test]
    fn pane_watcher_nonexistent_file() {
        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            PathBuf::from("/nonexistent/path.jsonl"),
            Duration::from_millis(50),
            Duration::from_millis(100),
        );

        let status = watcher.poll(1000);
        // Deriver starts Idle, last_status starts Unknown → transition to Idle
        assert!(status.is_some());
        assert!(status.unwrap().is_idle());
    }

    #[test]
    fn pane_watcher_incremental_reads() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"user","message":{{"role":"user","content":"hi"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        let status = watcher.poll(1000).unwrap();
        assert!(status.is_active());
        let offset_after_first = watcher.file_offset;
        assert!(offset_after_first > 0);

        // No new data → no change
        assert!(watcher.poll(1000).is_none());

        // Write TurnComplete
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"done"}}]}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let status = watcher.poll(1000).unwrap();
        assert!(status.is_idle());
        assert!(watcher.file_offset > offset_after_first);
    }

    #[test]
    fn pane_watcher_pending_discovery() {
        let mut watcher = PaneWatcher::new(
            Engine::Codex,
            PathBuf::from("pending-discovery"),
            Duration::from_millis(50),
            Duration::from_millis(100),
        );

        let status = watcher.poll(1000);
        // Idle != Unknown → returns Some(Idle)
        assert!(status.is_some());
        assert!(status.unwrap().is_idle());
    }

    #[test]
    fn pane_watcher_handles_partial_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");

        {
            let mut file = File::create(&path).unwrap();
            // Complete line + partial line (no trailing newline)
            write!(
                file,
                "{}\n{}",
                r#"{"type":"user","message":{"role":"user","content":"hello"}}"#,
                r#"{"type":"assistant","message":{"role":"as"#
            )
            .unwrap();
        }

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            path,
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        let status = watcher.poll(1000).unwrap();
        assert!(status.is_active()); // Only UserMessage processed
    }

    #[test]
    fn pane_watcher_codex_events() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"session_meta","payload":{{"id":"thread-1","model":"o3","cwd":"/tmp"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"response_item","payload":{{"type":"function_call","name":"exec_command","call_id":"c1","arguments":"{{}}"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::Codex,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        let status = watcher.poll(1000).unwrap();
        assert!(status.is_active());
    }

    #[test]
    fn pane_watcher_reads_droid_events() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"session_start","id":"sess-abc","model":"opus"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"cargo test"}}}}]}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::Droid,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        let status = watcher.poll(1000).unwrap();
        assert!(status.is_active());
        assert_eq!(watcher.deriver.last_tool_name(), Some("Bash"));
    }

    #[test]
    fn full_droid_session_lifecycle() {
        let mut file = tempfile::NamedTempFile::new().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::Droid,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        // 1. Session start
        writeln!(
            file,
            r#"{{"type":"session_start","id":"sess-abc","model":"opus"}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let status = watcher.poll(1000).unwrap();
        assert!(status.is_active(), "SessionStarted → Active");

        // 2. User sends message
        writeln!(
            file,
            r#"{{"type":"message","message":{{"role":"user","content":"fix the bug"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // 3. Agent thinks
        writeln!(file, r#"{{"type":"message","message":{{"role":"assistant","stop_reason":null,"content":[{{"type":"thinking","thinking":"analyzing..."}}]}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // 4. Agent proposes Bash tool
        writeln!(file, r#"{{"type":"message","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"toolu_001","name":"Bash","input":{{"command":"cargo test"}}}}]}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);
        assert_eq!(watcher.deriver.last_tool_name(), Some("Bash"));

        // 5. Todo state (progress)
        writeln!(
            file,
            r#"{{"type":"todo_state","todos":[{{"id":"1","text":"Fix","status":"in_progress"}}]}}"#
        )
        .unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // 6. Tool result
        writeln!(file, r#"{{"type":"message","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_001","is_error":false,"content":"test passed"}}]}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);
        assert!(
            watcher.deriver.last_tool_name().is_none(),
            "Tool cleared after result"
        );

        // 7. Agent responds with end_turn
        writeln!(file, r#"{{"type":"message","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"Fixed!"}}]}}}}"#).unwrap();
        file.flush().unwrap();
        let status = watcher.poll(1000).unwrap();
        assert!(status.is_idle(), "end_turn → Idle");
    }

    #[test]
    fn droid_session_end_becomes_idle() {
        let mut file = tempfile::NamedTempFile::new().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::Droid,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        writeln!(
            file,
            r#"{{"type":"session_start","id":"sess-1","model":"opus"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","message":{{"role":"user","content":"do stuff"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        writeln!(file, r#"{{"type":"session_end","reason":"completed"}}"#).unwrap();
        file.flush().unwrap();
        let status = watcher.poll(1000).unwrap();
        assert!(status.is_idle(), "session_end → Idle");
    }

    #[test]
    fn session_watcher_track_task() {
        let config = test_config();
        let fallback = StatusMonitor::with_detectors(vec![]);
        let mut watcher = SessionWatcher::new(fallback, &config);

        let task = make_task_with_pane("%0", PathBuf::from("/nonexistent/session.jsonl"));
        watcher.track_task(&task, "wagner_test-task");

        assert!(watcher.pane_watchers.contains_key("%0"));
    }

    #[test]
    fn session_watcher_track_task_idempotent() {
        let config = test_config();
        let fallback = StatusMonitor::with_detectors(vec![]);
        let mut watcher = SessionWatcher::new(fallback, &config);

        let task = make_task_with_pane("%0", PathBuf::from("/nonexistent/session.jsonl"));
        watcher.track_task(&task, "wagner_test-task");

        // Poll to change internal state
        if let Some(pw) = watcher.pane_watchers.get_mut("%0") {
            pw.poll(1000);
        }

        // Track again — should not reset the watcher
        watcher.track_task(&task, "wagner_test-task");
        let pw = watcher.pane_watchers.get("%0").unwrap();
        assert!(pw.last_status.is_idle()); // Still has the derived state
    }

    #[test]
    fn session_watcher_poll_active_tracked() {
        let config = test_config();
        let fallback = StatusMonitor::with_detectors(vec![]);
        let mut watcher = SessionWatcher::new(fallback, &config);

        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"user","message":{{"role":"user","content":"hi"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let task = make_task_with_pane("%5", file.path().to_path_buf());
        watcher.track_task(&task, "wagner_test");

        let mock = crate::terminal::MockTerminal::new();
        let panes = vec![PaneHandle("%5".into(), "repo1".into())];
        let updates = watcher.poll_active(&mock, "wagner_test", &panes);

        assert_eq!(updates.len(), 1);
        assert!(updates[0].status.is_active());
        assert_eq!(updates[0].pane.0, "%5");
    }

    #[test]
    fn session_watcher_get_session_status() {
        let config = test_config();
        let fallback = StatusMonitor::with_detectors(vec![]);
        let mut watcher = SessionWatcher::new(fallback, &config);

        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"user","message":{{"role":"user","content":"hi"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let task = make_task_with_pane("%5", file.path().to_path_buf());
        watcher.track_task(&task, "wagner_test");

        let mock = crate::terminal::MockTerminal::new();
        let panes = vec![PaneHandle("%5".into(), "repo1".into())];
        watcher.poll_active(&mock, "wagner_test", &panes);

        let status = watcher.get_session_status("wagner_test");
        assert_eq!(status, SessionAggregateStatus::Working);
    }

    #[test]
    fn session_watcher_get_session_status_empty() {
        let config = test_config();
        let fallback = StatusMonitor::with_detectors(vec![]);
        let watcher = SessionWatcher::new(fallback, &config);

        let status = watcher.get_session_status("nonexistent");
        assert_eq!(status, SessionAggregateStatus::Empty);
    }

    #[test]
    fn session_watcher_get_pane_status() {
        let config = test_config();
        let fallback = StatusMonitor::with_detectors(vec![]);
        let mut watcher = SessionWatcher::new(fallback, &config);

        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"user","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let task = make_task_with_pane("%3", file.path().to_path_buf());
        watcher.track_task(&task, "wagner_test");

        let mock = crate::terminal::MockTerminal::new();
        watcher.poll_active(
            &mock,
            "wagner_test",
            &[PaneHandle("%3".into(), "repo1".into())],
        );

        let status = watcher.get_pane_status("wagner_test", "%3");
        assert!(status.is_some());
        assert!(status.unwrap().is_active());
    }

    #[test]
    fn full_claude_session_lifecycle() {
        let mut file = tempfile::NamedTempFile::new().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        // 1. System event (session start)
        writeln!(file, r#"{{"type":"system","sessionId":"abc-123","message":{{"model":"claude-opus-4-20250514"}}}}"#).unwrap();
        file.flush().unwrap();
        let status = watcher.poll(1000).unwrap();
        assert!(status.is_active(), "SessionStarted → Active");

        // 2. User sends message
        writeln!(
            file,
            r#"{{"type":"user","message":{{"role":"user","content":"fix the bug"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        watcher.poll(1000); // stays Active

        // 3. Agent thinks
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":null,"content":[{{"type":"thinking","thinking":"analyzing..."}}]}}}}"#).unwrap();
        file.flush().unwrap();
        let status = watcher.poll(1000);
        // Either None (still Active) or Some(Active)
        if let Some(s) = status {
            assert!(s.is_active());
        }

        // 4. Agent proposes Bash tool
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"toolu_001","name":"Bash","input":{{"command":"cargo test"}}}}]}}}}"#).unwrap();
        file.flush().unwrap();
        let status = watcher.poll(1000);
        if let Some(s) = status {
            assert!(s.is_active());
        }
        assert_eq!(watcher.deriver.last_tool_name(), Some("Bash"));

        // 5. Progress events (ignored for state)
        writeln!(file, r#"{{"type":"progress","data":{{}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"progress","data":{{}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // 6. Tool result (approved, ran successfully)
        writeln!(file, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_001","is_error":false,"content":"test passed"}}]}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);
        assert!(
            watcher.deriver.last_tool_name().is_none(),
            "Tool cleared after result"
        );

        // 7. Agent responds with text + end_turn
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"Fixed!"}}]}}}}"#).unwrap();
        file.flush().unwrap();
        let status = watcher.poll(1000).unwrap();
        assert!(status.is_idle(), "end_turn → Idle");
    }

    #[test]
    fn full_codex_session_lifecycle() {
        let mut file = tempfile::NamedTempFile::new().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::Codex,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        // 1. Session meta
        writeln!(
            file,
            r#"{{"type":"session_meta","payload":{{"id":"thread-xyz","model":"o3","cwd":"/tmp"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let status = watcher.poll(1000).unwrap();
        assert!(status.is_active(), "SessionMeta → Active");

        // 2. User message
        writeln!(
            file,
            r#"{{"type":"event_msg","payload":{{"type":"user_message","content":"fix bug"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // 3. Reasoning
        writeln!(
            file,
            r#"{{"type":"response_item","payload":{{"type":"reasoning","content":"thinking..."}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // 4. Function call
        writeln!(file, r#"{{"type":"response_item","payload":{{"type":"function_call","name":"exec_command","call_id":"call_1","arguments":"{{\"cmd\":\"ls\"}}"}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // 5. Function call output
        writeln!(file, r#"{{"type":"response_item","payload":{{"type":"function_call_output","call_id":"call_1","output":"file1\nfile2"}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // 6. Message output
        writeln!(
            file,
            r#"{{"type":"response_item","payload":{{"type":"message","content":"Done fixing"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // 7. Task complete
        writeln!(
            file,
            r#"{{"type":"event_msg","payload":{{"type":"task_complete","turn_id":"1"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let status = watcher.poll(1000).unwrap();
        assert!(status.is_idle(), "task_complete → Idle");
    }

    #[test]
    fn tool_proposed_timeout_becomes_waiting() {
        let mut file = tempfile::NamedTempFile::new().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            file.path().to_path_buf(),
            Duration::from_millis(30), // Short approval timeout
            Duration::from_millis(5000),
        );

        // Tool proposed, no result
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"toolu_x","name":"Bash","input":{{}}}}]}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // Wait past approval timeout
        std::thread::sleep(Duration::from_millis(40));

        let status = watcher.poll(1000).unwrap();
        assert!(status.is_waiting(), "Pending tool past timeout → Waiting");
    }

    #[test]
    fn tool_rejected_stays_active() {
        let mut file = tempfile::NamedTempFile::new().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        // Tool proposed
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"toolu_r","name":"Bash","input":{{}}}}]}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // User rejects
        writeln!(file, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_r","is_error":true,"content":"User rejected tool use"}}]}}}}"#).unwrap();
        file.flush().unwrap();
        watcher.poll(1000);

        // Tool cleared, still Active
        assert!(watcher.deriver.last_tool_name().is_none());
        assert!(watcher.last_status.is_active());
    }

    #[test]
    fn file_appears_after_watcher_creation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("late-session.jsonl");

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            path.clone(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        // File doesn't exist yet — returns Idle (deriver default)
        let status = watcher.poll(1000).unwrap();
        assert!(status.is_idle());

        // No change on subsequent poll
        assert!(watcher.poll(1000).is_none());

        // File appears with events
        {
            let mut file = File::create(&path).unwrap();
            writeln!(
                file,
                r#"{{"type":"user","message":{{"role":"user","content":"hello"}}}}"#
            )
            .unwrap();
            writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Read","input":{{}}}}]}}}}"#).unwrap();
        }

        let status = watcher.poll(1000).unwrap();
        assert!(status.is_active(), "File appeared → reads events → Active");
    }

    #[test]
    fn session_watcher_multi_pane_aggregate() {
        let config = MonitorConfig {
            approval_timeout_ms: 5000,
            idle_threshold_ms: 5000,
            max_lines_per_poll: 1000,
            ..Default::default()
        };
        let fallback = StatusMonitor::with_detectors(vec![]);
        let mut watcher = SessionWatcher::new(fallback, &config);

        // Create two JSONL files: one active, one idle
        let mut file_active = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file_active,
            r#"{{"type":"user","message":{{"role":"user","content":"hi"}}}}"#
        )
        .unwrap();
        file_active.flush().unwrap();

        let mut file_idle = tempfile::NamedTempFile::new().unwrap();
        writeln!(file_idle, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"done"}}]}}}}"#).unwrap();
        file_idle.flush().unwrap();

        let task = Task {
            name: "multi-pane".into(),
            path: PathBuf::from("/tmp/mp"),
            repos: vec![
                TaskRepo {
                    name: "api".into(),
                    source: RepoSource::Local(PathBuf::from("/tmp/api")),
                    worktree: PathBuf::from("/tmp/mp/api"),
                    branch: "test".into(),
                },
                TaskRepo {
                    name: "web".into(),
                    source: RepoSource::Local(PathBuf::from("/tmp/web")),
                    worktree: PathBuf::from("/tmp/mp/web"),
                    branch: "test".into(),
                },
            ],
            created_at: Utc::now(),
            diff_base: None,
            kind: TaskKind::Managed,
            panes: vec![
                ModelTrackedPane {
                    name: "api".into(),
                    repo_name: "api".into(),
                    engine: Engine::ClaudeCode,
                    session_id: "s1".into(),
                    pane_id: "%10".into(),
                    jsonl_path: file_active.path().to_path_buf(),
                    launched_at: Utc::now(),
                },
                ModelTrackedPane {
                    name: "web".into(),
                    repo_name: "web".into(),
                    engine: Engine::ClaudeCode,
                    session_id: "s2".into(),
                    pane_id: "%11".into(),
                    jsonl_path: file_idle.path().to_path_buf(),
                    launched_at: Utc::now(),
                },
            ],
        };

        watcher.track_task(&task, "wagner_multi-pane");

        let mock = crate::terminal::MockTerminal::new();
        let panes = vec![
            PaneHandle("%10".into(), "api".into()),
            PaneHandle("%11".into(), "web".into()),
        ];
        let updates = watcher.poll_active(&mock, "wagner_multi-pane", &panes);

        assert_eq!(updates.len(), 2, "Both panes should emit updates");

        // Aggregate: one active + one idle = Working
        let aggregate = watcher.get_session_status("wagner_multi-pane");
        assert_eq!(aggregate, SessionAggregateStatus::Working);

        // Check individual statuses
        let api_status = watcher.get_pane_status("wagner_multi-pane", "%10").unwrap();
        assert!(api_status.is_active(), "api pane should be Active");
        let web_status = watcher.get_pane_status("wagner_multi-pane", "%11").unwrap();
        assert!(web_status.is_idle(), "web pane should be Idle");
    }

    #[test]
    fn session_watcher_needs_attention_aggregate() {
        let config = MonitorConfig {
            approval_timeout_ms: 20,
            idle_threshold_ms: 5000,
            max_lines_per_poll: 1000,
            ..Default::default()
        };
        let fallback = StatusMonitor::with_detectors(vec![]);
        let mut watcher = SessionWatcher::new(fallback, &config);

        // Pane with pending tool (will timeout to Waiting)
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{}}}}]}}}}"#).unwrap();
        file.flush().unwrap();

        let task = make_task_with_pane("%20", file.path().to_path_buf());
        watcher.track_task(&task, "wagner_wait");

        let mock = crate::terminal::MockTerminal::new();
        let panes = vec![PaneHandle("%20".into(), "repo1".into())];
        watcher.poll_active(&mock, "wagner_wait", &panes);

        // Wait for approval timeout
        std::thread::sleep(Duration::from_millis(30));
        watcher.poll_active(&mock, "wagner_wait", &panes);

        let aggregate = watcher.get_session_status("wagner_wait");
        assert_eq!(
            aggregate,
            SessionAggregateStatus::NeedsAttention,
            "Waiting pane → NeedsAttention"
        );
    }

    #[test]
    fn realistic_multi_turn_claude_session() {
        let mut file = tempfile::NamedTempFile::new().unwrap();

        // Simulate a realistic multi-turn Claude session
        // Turn 1: user asks, agent responds with tool, tool runs, agent continues
        writeln!(file, r#"{{"type":"system","sessionId":"abc-123","message":{{"model":"claude-opus-4-20250514"}}}}"#).unwrap();
        writeln!(
            file,
            r#"{{"type":"user","message":{{"role":"user","content":"fix the tests"}}}}"#
        )
        .unwrap();
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"thinking","thinking":"Let me check the test files"}},{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"cargo test"}}}}]}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","is_error":false,"content":"test result: FAILED. 2 passed; 1 failed"}}]}}}}"#).unwrap();
        // Turn 2: agent reads a file, edits it
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t2","name":"Read","input":{{"path":"src/lib.rs"}}}}]}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t2","is_error":false,"content":"fn main() {{}}"}}]}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t3","name":"Edit","input":{{"path":"src/lib.rs"}}}}]}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t3","is_error":false,"content":"edited"}}]}}}}"#).unwrap();
        // Turn 3: agent reruns tests, reports success
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t4","name":"Bash","input":{{"command":"cargo test"}}}}]}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t4","is_error":false,"content":"test result: ok. 3 passed"}}]}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"All tests pass now."}}]}}}}"#).unwrap();
        file.flush().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        watcher.poll(100_000);

        let file_len = std::fs::metadata(file.path()).unwrap().len();
        assert_eq!(watcher.file_offset, file_len, "Should consume entire file");
        assert!(
            watcher.last_status.is_idle(),
            "Session ends idle after end_turn, got: {:?}",
            watcher.last_status
        );
    }

    #[test]
    fn max_lines_per_poll_respected() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        // Write 10 events
        for i in 0..10 {
            writeln!(
                file,
                r#"{{"type":"user","message":{{"role":"user","content":"msg {i}"}}}}"#
            )
            .unwrap();
        }
        file.flush().unwrap();

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            file.path().to_path_buf(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        // Poll with max_lines=3
        watcher.poll(3);
        let first_offset = watcher.file_offset;

        // Should not have read the entire file
        let file_len = std::fs::metadata(file.path()).unwrap().len();
        assert!(
            first_offset < file_len,
            "Should stop after max_lines (offset {} < file_len {})",
            first_offset,
            file_len
        );

        // Poll again to read more
        watcher.poll(3);
        assert!(
            watcher.file_offset > first_offset,
            "Should make progress on second poll"
        );
    }

    #[test]
    fn file_truncation_resets_offset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");

        // Write initial data
        {
            let mut file = File::create(&path).unwrap();
            writeln!(
                file,
                r#"{{"type":"user","message":{{"role":"user","content":"first session"}}}}"#
            )
            .unwrap();
            writeln!(file, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"done"}}]}}}}"#).unwrap();
        }

        let mut watcher = PaneWatcher::new(
            Engine::ClaudeCode,
            path.clone(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        );

        watcher.poll(1000);
        let old_offset = watcher.file_offset;
        assert!(old_offset > 0);
        assert!(watcher.last_status.is_idle());

        // Truncate and write shorter content (simulating new session)
        {
            let mut file = File::create(&path).unwrap();
            writeln!(
                file,
                r#"{{"type":"user","message":{{"role":"user","content":"new session"}}}}"#
            )
            .unwrap();
        }

        // File is now shorter than offset — should reset and read new data
        let status = watcher.poll(1000);
        assert!(status.is_some(), "Should detect change after truncation");
        assert!(status.unwrap().is_active(), "New session should be Active");
        assert!(watcher.file_offset < old_offset, "Offset should have reset");
    }

    #[test]
    fn session_watcher_get_pane_context() {
        let config = MonitorConfig {
            approval_timeout_ms: 5000,
            idle_threshold_ms: 5000,
            max_lines_per_poll: 1000,
            ..Default::default()
        };
        let fallback = StatusMonitor::with_detectors(vec![]);
        let mut watcher = SessionWatcher::new(fallback, &config);

        let mut file = tempfile::NamedTempFile::new().unwrap();
        // Tool proposed with input containing file_path
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Read","input":{{"file_path":"/src/main.rs"}}}}]}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let task = make_task_with_pane("%7", file.path().to_path_buf());
        watcher.track_task(&task, "wagner_ctx");

        let mock = crate::terminal::MockTerminal::new();
        watcher.poll_active(
            &mock,
            "wagner_ctx",
            &[PaneHandle("%7".into(), "repo1".into())],
        );

        let ctx = watcher.get_pane_context("%7");
        assert_eq!(ctx, Some("/src/main.rs".to_string()));

        // Untracked pane returns None
        assert_eq!(watcher.get_pane_context("%99"), None);
    }
}
