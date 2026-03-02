mod commands;
mod outbox;
mod render;

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};

use teloxide::prelude::*;
use teloxide::types::{
    AllowedUpdate, BotCommand, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, MessageId,
    ParseMode, ReplyParameters, UpdateKind,
};
use tracing::{debug, warn};

use crate::config::TelegramConfig;
use crate::core::WagnerCore;
use crate::error::WagnerError;
use crate::model::{Engine, Task};
use crate::monitor::status::PaneStatus;
use crate::monitor::strip_ansi;
use crate::store::Store;
use crate::terminal::{PaneHandle, SessionHandle, Terminal, session_name_for_task};
use crate::transport::adapter::Adapter;
use crate::transport::{CoreCommand, CoreEvent, CoreResponse, PaneOutputMode};

use self::commands::{ParsedCommand, parse_command};
use self::outbox::Outbox;
use self::render::{render_event, render_response, render_progress, render_progress_done, render_agent_response};

const MAX_MESSAGE_LEN: usize = 4000;

#[derive(Debug, Clone)]
struct ActionButton {
    label: String,
    callback_data: String,
}

#[derive(Debug, Clone)]
struct MessageRef {
    chat_id: i64,
    message_id: i32,
    edit_in_place: bool,
}

struct FocusTarget {
    task_name: String,
    pane_name: Option<String>,
    sticky: bool,
}

struct ProgressMsgState {
    message_id: i32,
    last_rendered: String,
    last_edit: std::time::Instant,
    last_steps: Vec<crate::transport::ProgressStep>,
    last_step_count: usize,
}

pub struct TelegramAdapter {
    bot: Bot,
    chat_id: ChatId,
    outbox: Outbox,
    offset: AtomicI32,
    // Live message tracking
    live_messages: HashMap<String, MessageRef>,
    message_to_pane: HashMap<i32, (String, String)>,
    // ID registries for callback data (64-byte limit)
    entity_registry: HashMap<u16, (String, String)>,
    entity_reverse: HashMap<(String, String), u16>,
    task_registry: HashMap<u16, String>,
    task_reverse: HashMap<String, u16>,
    next_entity_id: u16,
    next_task_id: u16,
    // Focus mode
    focus: Option<FocusTarget>,
    suppressed_count: u32,
    // Pending reply-route: set by handlers so the response message gets tracked for replies
    reply_route_pending: Option<(String, String)>,
    // Pending rename: set by rn callback so poll_and_handle stores it keyed by message_id
    rename_route_pending: Option<(String, String, String)>,
    // Active renames: message_id → (task_name, pane_name, pane_id)
    pending_rename: HashMap<i32, (String, String, String)>,
    // Authorization
    allowed_users: Vec<i64>,
    // Per-pane output mode
    pane_modes: HashMap<String, PaneOutputMode>,
    // Progress message state for Stream mode
    progress_messages: HashMap<String, ProgressMsgState>,
    // Panes that received user input — stores (task_name, baseline_line_count, engine)
    awaiting_response: HashMap<String, (String, usize, Engine)>,
}

impl TelegramAdapter {
    pub fn new(config: &TelegramConfig) -> crate::Result<Self> {
        let bot = Bot::new(&config.bot_token);
        let chat_id = ChatId(config.chat_id);
        let outbox = Outbox::new(config.rate_limit_ms);

        let bot_clone = bot.clone();
        tokio::spawn(async move {
            let commands = vec![
                BotCommand::new("status", "Full status overview"),
                BotCommand::new("tasks", "List all tasks"),
                BotCommand::new("approve", "Approve waiting pane"),
                BotCommand::new("reject", "Reject waiting pane"),
                BotCommand::new("send", "Send message to pane"),
                BotCommand::new("output", "Capture pane output"),
                BotCommand::new("resume", "Resume dead agent session"),
                BotCommand::new("add", "Add pane to task"),
                BotCommand::new("rename", "Rename pane"),
                BotCommand::new("kill", "Kill pane"),
                BotCommand::new("mode", "Set pane output mode"),
                BotCommand::new("focus", "Focus on pane/task"),
                BotCommand::new("unfocus", "Exit focus mode"),
                BotCommand::new("help", "Show commands"),
            ];
            if let Err(e) = bot_clone.set_my_commands(commands).await {
                warn!(%e, "failed to set bot commands");
            }
        });

        Ok(Self {
            bot,
            chat_id,
            outbox,
            offset: AtomicI32::new(0),
            live_messages: HashMap::new(),
            message_to_pane: HashMap::new(),
            entity_registry: HashMap::new(),
            entity_reverse: HashMap::new(),
            task_registry: HashMap::new(),
            task_reverse: HashMap::new(),
            next_entity_id: 1,
            next_task_id: 1,
            focus: None,
            suppressed_count: 0,
            reply_route_pending: None,
            rename_route_pending: None,
            pending_rename: HashMap::new(),
            allowed_users: config.allowed_users.clone(),
            pane_modes: HashMap::new(),
            progress_messages: HashMap::new(),
            awaiting_response: HashMap::new(),
        })
    }

    // --- Registry methods ---

    fn register_entity(&mut self, task: &str, pane: &str) -> u16 {
        let key = (task.to_string(), pane.to_string());
        if let Some(&id) = self.entity_reverse.get(&key) {
            return id;
        }
        let id = self.next_entity_id;
        self.next_entity_id = self.next_entity_id.wrapping_add(1);
        self.entity_registry.insert(id, key.clone());
        self.entity_reverse.insert(key, id);
        id
    }

    fn register_task(&mut self, task: &str) -> u16 {
        if let Some(&id) = self.task_reverse.get(task) {
            return id;
        }
        let id = self.next_task_id;
        self.next_task_id = self.next_task_id.wrapping_add(1);
        self.task_registry.insert(id, task.to_string());
        self.task_reverse.insert(task.to_string(), id);
        id
    }

    fn resolve_entity(&self, id: u16) -> Option<(&str, &str)> {
        self.entity_registry
            .get(&id)
            .map(|(t, p)| (t.as_str(), p.as_str()))
    }

    fn resolve_task(&self, id: u16) -> Option<&str> {
        self.task_registry.get(&id).map(|s| s.as_str())
    }

    fn matches_focus(&self, task_name: &str, pane_name: &str) -> bool {
        match &self.focus {
            None => true,
            Some(f) => {
                if f.task_name != task_name {
                    return false;
                }
                match &f.pane_name {
                    Some(name) => name == pane_name,
                    None => true,
                }
            }
        }
    }

    fn get_pane_mode(&self, pane_id: &str) -> PaneOutputMode {
        self.pane_modes
            .get(pane_id)
            .copied()
            .unwrap_or(PaneOutputMode::Alerts)
    }

    // --- Telegram API methods ---

    async fn do_send(
        &self,
        text: &str,
        keyboard: Option<InlineKeyboardMarkup>,
    ) -> crate::Result<Option<MessageRef>> {
        let text = truncate_message(text);
        let mut req = self
            .bot
            .send_message(self.chat_id, &text)
            .parse_mode(ParseMode::MarkdownV2);
        if let Some(kb) = keyboard {
            req = req.reply_markup(kb);
        }
        match req.await {
            Ok(msg) => Ok(Some(MessageRef {
                chat_id: self.chat_id.0,
                message_id: msg.id.0,
                edit_in_place: false,
            })),
            Err(e) => {
                warn!(%e, "telegram send failed");
                Err(WagnerError::Transport(format!("Telegram send error: {e}")))
            }
        }
    }

    async fn do_send_html(
        &self,
        text: &str,
    ) -> crate::Result<Option<MessageRef>> {
        let text = truncate_message(text);
        let req = self
            .bot
            .send_message(self.chat_id, &text)
            .parse_mode(ParseMode::Html);
        match req.await {
            Ok(msg) => Ok(Some(MessageRef {
                chat_id: self.chat_id.0,
                message_id: msg.id.0,
                edit_in_place: false,
            })),
            Err(e) => {
                warn!(%e, "telegram send html failed");
                Err(WagnerError::Transport(format!("Telegram send error: {e}")))
            }
        }
    }

    async fn do_edit(
        &self,
        msg_ref: &MessageRef,
        text: &str,
        keyboard: Option<InlineKeyboardMarkup>,
    ) -> crate::Result<Option<MessageRef>> {
        let text = truncate_message(text);
        let mut req = self
            .bot
            .edit_message_text(
                ChatId(msg_ref.chat_id),
                MessageId(msg_ref.message_id),
                &text,
            )
            .parse_mode(ParseMode::MarkdownV2);
        let kb_fallback = keyboard.clone();
        if let Some(kb) = keyboard {
            req = req.reply_markup(kb);
        }
        match req.await {
            Ok(_) => Ok(Some(msg_ref.clone())),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("message is not modified") {
                    return Ok(Some(msg_ref.clone()));
                }
                warn!(%e, "telegram edit failed, sending new message");
                self.do_send(&text, kb_fallback).await
            }
        }
    }

    async fn do_reply(
        &self,
        text: &str,
        reply_to: Option<&MessageRef>,
        keyboard: Option<InlineKeyboardMarkup>,
    ) -> crate::Result<Option<MessageRef>> {
        let text = truncate_message(text);
        let mut req = self
            .bot
            .send_message(self.chat_id, &text)
            .parse_mode(ParseMode::MarkdownV2);
        if let Some(r) = reply_to {
            req = req
                .reply_parameters(ReplyParameters::new(MessageId(r.message_id)));
        }
        if let Some(kb) = keyboard {
            req = req.reply_markup(kb);
        }
        match req.await {
            Ok(msg) => Ok(Some(MessageRef {
                chat_id: self.chat_id.0,
                message_id: msg.id.0,
                edit_in_place: false,
            })),
            Err(e) => {
                warn!(%e, "telegram reply failed");
                Err(WagnerError::Transport(format!("Telegram send error: {e}")))
            }
        }
    }

    async fn send_event_text(
        &self,
        event: &CoreEvent,
        buttons: &[Vec<ActionButton>],
    ) -> crate::Result<Option<MessageRef>> {
        let text = render_event(event);
        let keyboard = build_keyboard(buttons);
        self.outbox.throttle().await;
        self.do_send(&text, keyboard).await
    }

    async fn edit_event_text(
        &self,
        msg_ref: &MessageRef,
        event: &CoreEvent,
        buttons: &[Vec<ActionButton>],
    ) -> crate::Result<Option<MessageRef>> {
        let text = render_event(event);
        let keyboard = build_keyboard(buttons);
        self.outbox.throttle().await;
        self.do_edit(msg_ref, &text, keyboard).await
    }

    async fn send_response_text(
        &self,
        response: &CoreResponse,
        buttons: &[Vec<ActionButton>],
        reply_to: Option<&MessageRef>,
    ) -> crate::Result<Option<MessageRef>> {
        let text = render_response(response);
        let keyboard = build_keyboard(buttons);
        self.outbox.throttle().await;
        if let Some(r) = reply_to.filter(|r| r.edit_in_place) {
            self.do_edit(r, &text, keyboard).await
        } else {
            self.do_reply(&text, reply_to, keyboard).await
        }
    }

    // --- Event handling ---

    async fn dispatch_event(
        &mut self,
        event: &CoreEvent,
        terminal: &dyn Terminal,
        config: &crate::config::Config,
    ) -> crate::Result<()> {
        match event {
            CoreEvent::NeedsAttention {
                task_name,
                pane_name,
                pane_id,
                reason,
                output_tail,
            } => {
                if !self.matches_focus(task_name, pane_name) {
                    self.suppressed_count += 1;
                    return Ok(());
                }

                let output_tail = if output_tail.is_empty() {
                    let pane = PaneHandle(pane_id.clone(), String::new());
                    capture_tail(terminal, &pane, 5)
                } else {
                    output_tail.clone()
                };

                let enriched = CoreEvent::NeedsAttention {
                    task_name: task_name.clone(),
                    pane_name: pane_name.clone(),
                    pane_id: pane_id.clone(),
                    reason: *reason,
                    output_tail,
                };

                let eid = self.register_entity(task_name, pane_id);
                let buttons = build_attention_actions(
                    eid,
                    reason,
                    self.suppressed_count,
                    self.focus.is_some(),
                );

                let msg_ref = self.send_event_text(&enriched, &buttons).await?;
                if let Some(r) = msg_ref {
                    self.message_to_pane
                        .insert(r.message_id, (task_name.clone(), pane_id.clone()));
                    self.live_messages.insert(pane_id.clone(), r);
                }
            }

            CoreEvent::AgentIdle {
                task_name,
                pane_name,
                pane_id,
                response_text,
                ..
            } => {
                if !self.matches_focus(task_name, pane_name) {
                    self.suppressed_count += 1;
                    return Ok(());
                }

                // Suppress status labels when we're awaiting a reply — the response
                // message itself provides the feedback the user needs.
                let awaiting = self.awaiting_response.contains_key(pane_id);
                if !awaiting {
                    let mode = self.get_pane_mode(pane_id);
                    if mode == PaneOutputMode::Stream {
                        if let Some(progress_state) = self.progress_messages.remove(pane_id) {
                            let done_text = render_progress_done(
                                task_name,
                                pane_name,
                                &progress_state.last_steps,
                                progress_state.last_step_count,
                            );
                            let msg_ref = MessageRef {
                                chat_id: self.chat_id.0,
                                message_id: progress_state.message_id,
                                edit_in_place: true,
                            };
                            self.outbox.throttle().await;
                            let _ = self.do_edit(&msg_ref, &done_text, None).await;
                        }
                    } else if config.daemon.notify_idle {
                        let pane = PaneHandle(pane_id.clone(), String::new());
                        let output_tail = capture_tail(
                            terminal,
                            &pane,
                            config.daemon.default_output_lines,
                        );
                        let enriched = CoreEvent::AgentIdle {
                            task_name: task_name.clone(),
                            pane_name: pane_name.clone(),
                            pane_id: pane_id.clone(),
                            output_tail,
                            response_text: None,
                        };
                        let msg_ref = self.send_event_text(&enriched, &[]).await?;
                        if let Some(r) = msg_ref {
                            self.message_to_pane
                                .insert(r.message_id, (task_name.clone(), pane_id.clone()));
                        }
                    }
                }

                // Send response — from JSONL if available, otherwise capture terminal output.
                // Claude Code has JSONL monitoring so we never use terminal capture for it
                // (TUI output is garbled). For engines without JSONL (Codex), capture terminal.
                let response_content = match response_text {
                    Some(text) if !text.is_empty() => {
                        self.awaiting_response.remove(pane_id);
                        Some(text.clone())
                    }
                    _ if self.awaiting_response.contains_key(pane_id) => {
                        let &(_, _, engine) = self.awaiting_response.get(pane_id).unwrap();
                        if engine == Engine::ClaudeCode {
                            // JSONL will deliver response on a later poll — wait for it
                            None
                        } else {
                            let (_, baseline, engine) = self.awaiting_response.remove(pane_id).unwrap();
                            let pane = PaneHandle(pane_id.clone(), String::new());
                            let full_output = terminal
                                .capture(&pane, 500)
                                .map(|s| strip_ansi(&s))
                                .unwrap_or_default();
                            let new_lines = full_output
                                .lines()
                                .skip(baseline)
                                .collect::<Vec<_>>()
                                .join("\n");
                            let new_lines = strip_tui_chrome(new_lines.trim(), engine);
                            if new_lines.is_empty() { None } else { Some(new_lines) }
                        }
                    }
                    _ => None,
                };
                if let Some(text) = response_content {
                    let html = render_agent_response(task_name, pane_name, &text);
                    self.outbox.throttle().await;
                    if let Ok(Some(r)) = self.do_send_html(&html).await {
                        self.message_to_pane
                            .insert(r.message_id, (task_name.clone(), pane_id.clone()));
                    }
                }
            }

            CoreEvent::AgentWorking { pane_id, .. } => {
                if self.awaiting_response.contains_key(pane_id) {
                    // Suppress — response message provides the feedback
                } else {
                    let mode = self.get_pane_mode(pane_id);
                    if mode == PaneOutputMode::Stream {
                        // Progress messages handle the working state in stream mode
                    } else if let Some(msg_ref) = self.live_messages.remove(pane_id) {
                        self.message_to_pane.remove(&msg_ref.message_id);
                        self.edit_event_text(&msg_ref, event, &[]).await?;
                    }
                }
            }

            CoreEvent::AgentProgress {
                task_name,
                pane_name,
                pane_id,
                steps,
                pending,
                step_count,
            } => {
                let mode = self.get_pane_mode(pane_id);
                if mode != PaneOutputMode::Stream {
                    return Ok(());
                }

                let text = render_progress(
                    task_name,
                    pane_name,
                    steps,
                    pending.as_ref(),
                    *step_count,
                );

                match self.progress_messages.get(pane_id) {
                    None => {
                        // Send new progress message
                        self.outbox.throttle().await;
                        let keyboard = build_keyboard(&[]);
                        if let Ok(Some(r)) = self.do_send(&text, keyboard).await {
                            self.message_to_pane
                                .insert(r.message_id, (task_name.clone(), pane_id.clone()));
                            self.progress_messages.insert(
                                pane_id.clone(),
                                ProgressMsgState {
                                    message_id: r.message_id,
                                    last_rendered: text,
                                    last_edit: std::time::Instant::now(),
                                    last_steps: steps.clone(),
                                    last_step_count: *step_count,
                                },
                            );
                        }
                    }
                    Some(state) => {
                        let elapsed = state.last_edit.elapsed();
                        if elapsed >= std::time::Duration::from_secs(2)
                            && text != state.last_rendered
                        {
                            let msg_ref = MessageRef {
                                chat_id: self.chat_id.0,
                                message_id: state.message_id,
                                edit_in_place: true,
                            };
                            self.outbox.throttle().await;
                            let _ = self.do_edit(&msg_ref, &text, None).await;
                            if let Some(state) = self.progress_messages.get_mut(pane_id) {
                                state.last_rendered = text;
                                state.last_edit = std::time::Instant::now();
                                state.last_steps = steps.clone();
                                state.last_step_count = *step_count;
                            }
                        }
                    }
                }
            }

            CoreEvent::SessionStatusChanged { task_name, .. } => {
                let any_awaiting = self
                    .awaiting_response
                    .values()
                    .any(|(tn, _, _)| tn == task_name);
                if !any_awaiting {
                    let tid = self.register_task(task_name);
                    let buttons = vec![vec![ActionButton {
                        label: "Details".into(),
                        callback_data: format!("td:{tid}"),
                    }]];
                    self.send_event_text(event, &buttons).await?;
                }
            }

            CoreEvent::DaemonStarted { .. } | CoreEvent::DaemonStopping => {
                self.send_event_text(event, &[]).await?;
            }
        }

        Ok(())
    }

    // --- Command handling ---

    async fn poll_telegram(&self) -> crate::Result<Vec<(TelegramInput, MessageRef)>> {
        let offset = self.offset.load(Ordering::Relaxed);

        let updates = self
            .bot
            .get_updates()
            .offset(offset)
            .timeout(0)
            .limit(10)
            .allowed_updates(vec![
                AllowedUpdate::Message,
                AllowedUpdate::CallbackQuery,
            ])
            .await
            .map_err(|e| WagnerError::Transport(format!("Telegram poll error: {e}")))?;

        let mut inputs = Vec::new();

        for update in updates {
            let new_offset = update.id.as_offset();
            self.offset.store(new_offset, Ordering::Relaxed);

            match &update.kind {
                UpdateKind::Message(msg) => {
                    if msg.chat.id != self.chat_id {
                        debug!(
                            chat_id = msg.chat.id.0,
                            "ignoring message from unknown chat"
                        );
                        continue;
                    }

                    if !self.is_authorized(msg.from.as_ref().map(|u| u.id.0 as i64)) {
                        debug!("ignoring message from unauthorized user");
                        continue;
                    }

                    if let Some(text) = msg.text() {
                        let msg_ref = MessageRef {
                            chat_id: msg.chat.id.0,
                            message_id: msg.id.0,
                            edit_in_place: false,
                        };

                        let reply_to_id = if let Some(reply_msg) = msg.reply_to_message() {
                            if !text.starts_with('/') {
                                let lower = text.trim().to_lowercase();
                                let reply_text = if matches!(lower.as_str(), "y" | "yes") {
                                    "y".to_string()
                                } else if matches!(lower.as_str(), "n" | "no") {
                                    "n".to_string()
                                } else {
                                    text.to_string()
                                };
                                inputs.push((
                                    TelegramInput::Reply {
                                        reply_to_message_id: reply_msg.id.0,
                                        text: reply_text,
                                    },
                                    msg_ref,
                                ));
                                continue;
                            }
                            Some(reply_msg.id.0)
                        } else {
                            None
                        };

                        if let Some(cmd) = parse_command(text) {
                            inputs.push((TelegramInput::Command(cmd, reply_to_id), msg_ref));
                        }
                    }
                }

                UpdateKind::CallbackQuery(query) => {
                    if !self.is_authorized(Some(query.from.id.0 as i64)) {
                        debug!("ignoring callback from unauthorized user");
                        continue;
                    }

                    let query_id = query.id.clone();
                    let bot = self.bot.clone();
                    tokio::spawn(async move {
                        let _ = bot.answer_callback_query(query_id).await;
                    });

                    if let Some(data) = &query.data {
                        let source_msg_id = query
                            .message
                            .as_ref()
                            .map(|m| m.id().0)
                            .unwrap_or(0);

                        let chat_id = query
                            .message
                            .as_ref()
                            .map(|m| m.chat().id.0)
                            .unwrap_or(self.chat_id.0);

                        let msg_ref = MessageRef {
                            chat_id,
                            message_id: source_msg_id,
                            edit_in_place: true,
                        };

                        inputs.push((
                            TelegramInput::Callback {
                                data: data.clone(),
                            },
                            msg_ref,
                        ));
                    }
                }

                _ => {}
            }
        }

        Ok(inputs)
    }

    fn is_authorized(&self, user_id: Option<i64>) -> bool {
        if self.allowed_users.is_empty() {
            return true;
        }
        match user_id {
            Some(id) => self.allowed_users.contains(&id),
            None => false,
        }
    }

    fn handle_command(
        &mut self,
        cmd: &CoreCommand,
        core: &WagnerCore,
        terminal: &dyn Terminal,
        store: &Store,
        tasks: &[Task],
    ) -> (CoreResponse, Vec<Vec<ActionButton>>) {
        match cmd {
            CoreCommand::ListTasks => {
                let response = core.execute(terminal, store, cmd, tasks);

                let mut buttons: Vec<Vec<ActionButton>> = vec![];
                let mut detail_row = vec![];
                for t in tasks {
                    let tid = self.register_task(&t.name);
                    detail_row.push(ActionButton {
                        label: format!("{} Details", t.name),
                        callback_data: format!("td:{tid}"),
                    });
                    if detail_row.len() >= 2 {
                        buttons.push(std::mem::take(&mut detail_row));
                    }
                }
                if !detail_row.is_empty() {
                    buttons.push(detail_row);
                }

                (response, buttons)
            }

            CoreCommand::TaskStatus { task_name } => {
                let response = core.execute(terminal, store, cmd, tasks);

                let session_name = session_name_for_task(task_name);
                let session_panes = terminal
                    .list_panes(&SessionHandle(session_name.clone()))
                    .unwrap_or_default();

                let task = tasks.iter().find(|t| t.name == *task_name);
                let mut buttons = vec![];

                // Pane name buttons — drill-down to pane detail view
                let mut pane_row = vec![];
                for p in &session_panes {
                    let eid = self.register_entity(task_name, &p.0);
                    let display_name = task
                        .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == p.0))
                        .map(|tp| tp.name.as_str())
                        .unwrap_or(&p.1);
                    pane_row.push(ActionButton {
                        label: display_name.to_string(),
                        callback_data: format!("pd:{eid}"),
                    });
                    if pane_row.len() >= 3 {
                        buttons.push(std::mem::take(&mut pane_row));
                    }
                }
                if !pane_row.is_empty() {
                    buttons.push(pane_row);
                }

                let tid = self.register_task(task_name);
                if let CoreResponse::Status { panes, .. } = &response {
                    if panes.iter().any(|(_, s)| s.is_waiting()) {
                        buttons.push(vec![ActionButton {
                            label: "Approve All".into(),
                            callback_data: format!("aa:{tid}"),
                        }]);
                    }
                }
                buttons.push(vec![
                    ActionButton {
                        label: "Add Pane".into(),
                        callback_data: format!("ap:{tid}"),
                    },
                    ActionButton {
                        label: "Back".into(),
                        callback_data: "bk".into(),
                    },
                ]);

                (response, buttons)
            }

            CoreCommand::FullStatus => {
                let response = core.execute(terminal, store, cmd, tasks);

                let mut buttons: Vec<Vec<ActionButton>> = vec![];
                let mut detail_row = vec![];
                for t in tasks {
                    let tid = self.register_task(&t.name);
                    detail_row.push(ActionButton {
                        label: format!("{} Details", t.name),
                        callback_data: format!("td:{tid}"),
                    });
                    if detail_row.len() >= 2 {
                        buttons.push(std::mem::take(&mut detail_row));
                    }
                }
                if !detail_row.is_empty() {
                    buttons.push(detail_row);
                }
                buttons.push(vec![ActionButton {
                    label: "Refresh".into(),
                    callback_data: "sr".into(),
                }]);

                (response, buttons)
            }

            CoreCommand::Approve { task_name, pane_name } => {
                if task_name.is_empty() {
                    return self.smart_approve_with_buttons(core, terminal, tasks);
                }
                let response = core.execute(terminal, store, cmd, tasks);
                // Set up awaiting_response for the approved pane
                if matches!(&response, CoreResponse::Confirmation { .. }) {
                    let session_name = session_name_for_task(task_name);
                    let target = if let Some(name) = pane_name {
                        tasks.iter().find(|t| t.name == *task_name)
                            .and_then(|t| t.find_pane_by_name(name))
                            .map(|tp| (tp.pane_id.clone(), tp.engine))
                    } else {
                        // Find the first waiting pane (matches resolve_pane logic)
                        let panes = terminal
                            .list_panes(&SessionHandle(session_name.clone()))
                            .unwrap_or_default();
                        panes.iter().find(|p| {
                            core.status_engine
                                .get_pane_status(&session_name, &p.0)
                                .is_some_and(|s| s.is_waiting())
                        }).and_then(|p| {
                            let engine = tasks.iter().flat_map(|t| &t.panes)
                                .find(|tp| tp.pane_id == p.0)
                                .map(|tp| tp.engine)
                                .unwrap_or(Engine::ClaudeCode);
                            Some((p.0.clone(), engine))
                        })
                    };
                    if let Some((pane_id, engine)) = target {
                        let handle = PaneHandle(pane_id.clone(), String::new());
                        let baseline = terminal
                            .capture(&handle, 500)
                            .map(|s: String| s.lines().count())
                            .unwrap_or(0);
                        self.reply_route_pending = Some((task_name.clone(), pane_id.clone()));
                        self.awaiting_response.insert(pane_id, (task_name.clone(), baseline, engine));
                    }
                }
                (response, vec![])
            }

            CoreCommand::AddPane { task_name, .. } => {
                let response = core.execute(terminal, store, cmd, tasks);
                let tid = self.register_task(task_name);
                let buttons = vec![vec![ActionButton {
                    label: "Back to task".into(),
                    callback_data: format!("td:{tid}"),
                }]];
                (response, buttons)
            }

            CoreCommand::RenamePane { task_name, .. } => {
                let response = core.execute(terminal, store, cmd, tasks);
                let tid = self.register_task(task_name);
                let buttons = vec![vec![ActionButton {
                    label: "Back to task".into(),
                    callback_data: format!("td:{tid}"),
                }]];
                (response, buttons)
            }

            CoreCommand::KillPane { task_name, pane_name } => {
                // Clean up adapter state for the killed pane
                if let Some(task) = tasks.iter().find(|t| t.name == *task_name) {
                    if let Some(tp) = task.find_pane_by_name(pane_name) {
                        self.pane_modes.remove(&tp.pane_id);
                        self.progress_messages.remove(&tp.pane_id);
                    }
                }
                let response = core.execute(terminal, store, cmd, tasks);
                let tid = self.register_task(task_name);
                let buttons = vec![vec![ActionButton {
                    label: "Back to task".into(),
                    callback_data: format!("td:{tid}"),
                }]];
                (response, buttons)
            }

            CoreCommand::SetPaneMode {
                task_name,
                pane_name,
                mode,
            } => {
                // Infer task name if empty (from focus or single task)
                let resolved_task = if task_name.is_empty() {
                    if let Some(ref f) = self.focus {
                        f.task_name.clone()
                    } else if tasks.len() == 1 {
                        tasks[0].name.clone()
                    } else {
                        return (
                            CoreResponse::Error {
                                message: "Multiple tasks — specify which: /mode <task> <alerts|stream>".into(),
                            },
                            vec![],
                        );
                    }
                } else {
                    task_name.clone()
                };

                let resolved_cmd = CoreCommand::SetPaneMode {
                    task_name: resolved_task.clone(),
                    pane_name: pane_name.clone(),
                    mode: *mode,
                };

                // Store mode locally for all matching panes
                let task = tasks.iter().find(|t| t.name == resolved_task);
                if let Some(task) = task {
                    match pane_name {
                        Some(name) => {
                            if let Some(tp) = task.find_pane_by_name(name) {
                                self.pane_modes.insert(tp.pane_id.clone(), *mode);
                            }
                        }
                        None => {
                            for tp in &task.panes {
                                self.pane_modes.insert(tp.pane_id.clone(), *mode);
                            }
                        }
                    }
                }
                let response = core.execute(terminal, store, &resolved_cmd, tasks);
                (response, vec![])
            }

            CoreCommand::SendMessage { .. }
            | CoreCommand::Reject { .. }
            | CoreCommand::Resume { .. }
            | CoreCommand::CaptureOutput { .. }
            | CoreCommand::PluginList { .. }
            | CoreCommand::PluginGet { .. }
            | CoreCommand::Help => {
                let response = core.execute(terminal, store, cmd, tasks);
                (response, vec![])
            }
        }
    }

    fn handle_command_with_context(
        &mut self,
        cmd: &CoreCommand,
        reply_to_id: Option<i32>,
        core: &WagnerCore,
        terminal: &dyn Terminal,
        store: &Store,
        tasks: &[Task],
    ) -> (CoreResponse, Vec<Vec<ActionButton>>) {
        // If the command was sent as a reply, inject pane context from message_to_pane
        if let Some(reply_id) = reply_to_id {
            if let Some((task_name, pane_id)) = self.message_to_pane.get(&reply_id).cloned() {
                let pane_name = tasks
                    .iter()
                    .flat_map(|t| &t.panes)
                    .find(|p| p.pane_id == pane_id)
                    .map(|p| p.name.clone());

                let enriched = match cmd {
                    CoreCommand::CaptureOutput { lines, .. } => {
                        Some(CoreCommand::CaptureOutput {
                            task_name: task_name.clone(),
                            pane_name,
                            lines: *lines,
                        })
                    }
                    CoreCommand::Approve { .. } => {
                        Some(CoreCommand::Approve {
                            task_name: task_name.clone(),
                            pane_name,
                        })
                    }
                    CoreCommand::Reject { .. } => {
                        Some(CoreCommand::Reject {
                            task_name: task_name.clone(),
                            pane_name,
                        })
                    }
                    CoreCommand::Resume { .. } => {
                        Some(CoreCommand::Resume {
                            task_name: task_name.clone(),
                            pane_name,
                        })
                    }
                    _ => None,
                };

                if let Some(enriched_cmd) = enriched {
                    return self.handle_command(&enriched_cmd, core, terminal, store, tasks);
                }
            }
        }

        // If command has empty task_name and no reply context, return usage error
        match cmd {
            CoreCommand::CaptureOutput { task_name, .. } if task_name.is_empty() => {
                return (
                    CoreResponse::Error {
                        message: "Usage: /output <task> [lines] — or reply to a pane message".into(),
                    },
                    vec![],
                );
            }
            _ => {}
        }

        self.handle_command(cmd, core, terminal, store, tasks)
    }

    fn handle_reply(
        &mut self,
        reply_to_message_id: i32,
        text: &str,
        terminal: &dyn Terminal,
        core: &WagnerCore,
        store: &Store,
        tasks: &[Task],
    ) -> (CoreResponse, Vec<Vec<ActionButton>>) {
        if let Some((task_name, pane_name, _pane_id)) =
            self.pending_rename.remove(&reply_to_message_id)
        {
            let new_name = text.trim().to_string();
            if new_name.is_empty() || new_name.contains(char::is_whitespace) {
                return (
                    CoreResponse::Error {
                        message: "Pane name must be a single word with no spaces.".into(),
                    },
                    vec![],
                );
            }
            return self.handle_command(
                &CoreCommand::RenamePane {
                    task_name,
                    pane_name,
                    new_name,
                },
                core,
                terminal,
                store,
                tasks,
            );
        }

        match self.message_to_pane.get(&reply_to_message_id) {
            Some((task_name, pane_id)) => {
                let task_name = task_name.clone();
                let pane_id = pane_id.clone();
                let pane = PaneHandle(pane_id.clone(), String::new());
                let baseline = terminal
                    .capture(&pane, 500)
                    .map(|s: String| s.lines().count())
                    .unwrap_or(0);
                if let Err(e) = terminal.send_keys(&pane, text) {
                    return (
                        CoreResponse::Error {
                            message: format!("Failed to send: {e}"),
                        },
                        vec![],
                    );
                }
                let engine = tasks
                    .iter()
                    .flat_map(|t| &t.panes)
                    .find(|p| p.pane_id == pane_id)
                    .map(|p| p.engine)
                    .unwrap_or(Engine::ClaudeCode);
                let pane_name = tasks
                    .iter()
                    .flat_map(|t| &t.panes)
                    .find(|p| p.pane_id == pane_id)
                    .map(|p| p.name.as_str())
                    .unwrap_or("?");
                self.reply_route_pending = Some((task_name.clone(), pane_id.clone()));
                self.awaiting_response.insert(pane_id, (task_name.clone(), baseline, engine));
                (
                    CoreResponse::Confirmation {
                        message: format!("Sent to {task_name} | {pane_name}"),
                    },
                    vec![],
                )
            }
            None => (
                CoreResponse::Error {
                    message: "Cannot route reply — message not found. Use /send <task> <message> instead.".into(),
                },
                vec![],
            ),
        }
    }

    fn handle_focus(
        &mut self,
        task_name: &str,
        pane_name: Option<&str>,
        sticky: bool,
    ) -> (CoreResponse, Vec<Vec<ActionButton>>) {
        self.focus = Some(FocusTarget {
            task_name: task_name.to_string(),
            pane_name: pane_name.map(String::from),
            sticky,
        });
        self.suppressed_count = 0;
        let target = match pane_name {
            Some(p) => format!("{task_name} | {p}"),
            None => task_name.to_string(),
        };
        let sticky_note = if sticky { " (sticky)" } else { "" };
        (
            CoreResponse::Confirmation {
                message: format!("Focused on {target}{sticky_note}"),
            },
            vec![vec![ActionButton {
                label: "Unfocus".into(),
                callback_data: "uf".into(),
            }]],
        )
    }

    fn handle_unfocus(&mut self) -> (CoreResponse, Vec<Vec<ActionButton>>) {
        let count = self.suppressed_count;
        self.focus = None;
        self.suppressed_count = 0;
        (
            CoreResponse::Confirmation {
                message: format!("Focus cleared. {count} notifications were suppressed."),
            },
            vec![vec![ActionButton {
                label: "Status".into(),
                callback_data: "sr".into(),
            }]],
        )
    }

    fn handle_callback(
        &mut self,
        data: &str,
        core: &WagnerCore,
        terminal: &dyn Terminal,
        store: &Store,
        tasks: &[Task],
    ) -> (CoreResponse, Vec<Vec<ActionButton>>) {
        let parts: Vec<&str> = data.splitn(2, ':').collect();
        let action = parts[0];
        let id_str = parts.get(1).unwrap_or(&"");

        match action {
            "a" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_entity(id) {
                    Some((task, pane_id)) => {
                        let task = task.to_string();
                        let pane_id = pane_id.to_string();
                        let handle = PaneHandle(pane_id.clone(), String::new());
                        if let Err(e) = terminal.send_key(&handle, "y") {
                            return (
                                CoreResponse::Error {
                                    message: format!("Failed to approve: {e}"),
                                },
                                vec![],
                            );
                        }
                        let _ = terminal.send_key(&handle, "Enter");
                        let display_name = tasks
                            .iter()
                            .find(|t| t.name == task)
                            .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
                            .map(|tp| tp.name.as_str())
                            .unwrap_or("?");
                        let engine = tasks
                            .iter()
                            .flat_map(|t| &t.panes)
                            .find(|p| p.pane_id == pane_id)
                            .map(|p| p.engine)
                            .unwrap_or(Engine::ClaudeCode);
                        let baseline = terminal
                            .capture(&handle, 500)
                            .map(|s: String| s.lines().count())
                            .unwrap_or(0);
                        self.reply_route_pending = Some((task.clone(), pane_id.clone()));
                        self.awaiting_response.insert(pane_id, (task.clone(), baseline, engine));
                        (
                            CoreResponse::Confirmation {
                                message: format!("Approved {task} | {display_name}"),
                            },
                            vec![],
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — entity no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "r" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_entity(id) {
                    Some((task, pane_id)) => {
                        let task = task.to_string();
                        let pane_id = pane_id.to_string();
                        let handle = PaneHandle(pane_id.clone(), String::new());
                        if let Err(e) = terminal.send_key(&handle, "n") {
                            return (
                                CoreResponse::Error {
                                    message: format!("Failed to reject: {e}"),
                                },
                                vec![],
                            );
                        }
                        let _ = terminal.send_key(&handle, "Enter");
                        let display_name = tasks
                            .iter()
                            .find(|t| t.name == task)
                            .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
                            .map(|tp| tp.name.as_str())
                            .unwrap_or("?");
                        (
                            CoreResponse::Confirmation {
                                message: format!("Rejected {task} | {display_name}"),
                            },
                            vec![],
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — entity no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "o" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_entity(id) {
                    Some((task, pane_id)) => {
                        let task = task.to_string();
                        let pane_id = pane_id.to_string();
                        let handle = PaneHandle(pane_id.clone(), String::new());
                        let lines = core.config.daemon.default_output_lines;
                        let content = capture_tail(terminal, &handle, lines);
                        let display_name = tasks
                            .iter()
                            .find(|t| t.name == task)
                            .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
                            .map(|tp| tp.name.clone())
                            .unwrap_or_else(|| pane_id.clone());

                        let tid = self.register_task(&task);
                        let buttons = vec![
                            vec![
                                ActionButton {
                                    label: "Refresh".into(),
                                    callback_data: format!("o:{id}"),
                                },
                                ActionButton {
                                    label: "Resume".into(),
                                    callback_data: format!("rs:{id}"),
                                },
                            ],
                            vec![ActionButton {
                                label: "Back".into(),
                                callback_data: format!("td:{tid}"),
                            }],
                        ];

                        self.reply_route_pending = Some((task.clone(), pane_id.clone()));
                        (
                            CoreResponse::Output {
                                task_name: task,
                                pane_name: display_name,
                                content,
                            },
                            buttons,
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — entity no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "fp" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_entity(id) {
                    Some((task, pane_id)) => {
                        let task = task.to_string();
                        let pane_name = tasks
                            .iter()
                            .find(|t| t.name == task)
                            .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
                            .map(|tp| tp.name.clone())
                            .unwrap_or_else(|| pane_id.to_string());
                        self.handle_focus(&task, Some(&pane_name), false)
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — entity no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "ft" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_task(id) {
                    Some(task) => {
                        let task = task.to_string();
                        self.handle_focus(&task, None, false)
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — task no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "td" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_task(id) {
                    Some(task_name) => {
                        let task_name = task_name.to_string();
                        // Register for reply routing: pick first waiting pane, or first pane
                        if let Some(task) = tasks.iter().find(|t| t.name == task_name) {
                            let session_name = session_name_for_task(&task_name);
                            let target_pane = task.panes.iter().find(|tp| {
                                core.status_engine
                                    .get_pane_status(&session_name, &tp.pane_id)
                                    .is_some_and(|s| s.is_waiting())
                            }).or_else(|| task.panes.first());
                            if let Some(tp) = target_pane {
                                self.reply_route_pending =
                                    Some((task_name.clone(), tp.pane_id.clone()));
                            }
                        }
                        self.handle_command(
                            &CoreCommand::TaskStatus { task_name },
                            core,
                            terminal,
                            store,
                            tasks,
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — task no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "aa" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_task(id) {
                    Some(task_name) => {
                        let task_name = task_name.to_string();
                        let session_name = session_name_for_task(&task_name);
                        let panes = terminal
                            .list_panes(&SessionHandle(session_name.clone()))
                            .unwrap_or_default();

                        let mut approved = 0;
                        for pane in &panes {
                            let status = core
                                .status_engine
                                .get_pane_status(&session_name, &pane.0)
                                .cloned()
                                .unwrap_or(PaneStatus::Unknown);
                            if status.is_waiting() {
                                let _ = terminal.send_key(pane, "y");
                                let _ = terminal.send_key(pane, "Enter");
                                approved += 1;
                            }
                        }
                        (
                            CoreResponse::Confirmation {
                                message: format!("Approved {approved} panes in {task_name}"),
                            },
                            vec![],
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — task no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "pd" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_entity(id) {
                    Some((task_name, pane_id)) => {
                        let task_name = task_name.to_string();
                        let pane_id = pane_id.to_string();
                        let session_name = session_name_for_task(&task_name);
                        let task = tasks.iter().find(|t| t.name == task_name);
                        let display_name = task
                            .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
                            .map(|tp| tp.name.clone())
                            .unwrap_or_else(|| pane_id.clone());
                        let status = core
                            .status_engine
                            .get_pane_status(&session_name, &pane_id)
                            .cloned()
                            .unwrap_or(PaneStatus::Unknown);

                        self.reply_route_pending =
                            Some((task_name.clone(), pane_id.clone()));
                        let tid = self.register_task(&task_name);
                        let mut buttons = vec![];

                        if status.is_waiting() {
                            buttons.push(vec![
                                ActionButton {
                                    label: "Approve".into(),
                                    callback_data: format!("a:{id}"),
                                },
                                ActionButton {
                                    label: "Reject".into(),
                                    callback_data: format!("r:{id}"),
                                },
                            ]);
                        }

                        let current_mode = self.get_pane_mode(&pane_id);
                        let mode_label = match current_mode {
                            PaneOutputMode::Alerts => "Stream",
                            PaneOutputMode::Stream => "Alerts",
                        };
                        buttons.push(vec![
                            ActionButton {
                                label: "Output".into(),
                                callback_data: format!("o:{id}"),
                            },
                            ActionButton {
                                label: "Resume".into(),
                                callback_data: format!("rs:{id}"),
                            },
                        ]);
                        buttons.push(vec![
                            ActionButton {
                                label: "Rename".into(),
                                callback_data: format!("rn:{id}"),
                            },
                            ActionButton {
                                label: "Kill".into(),
                                callback_data: format!("kp:{id}"),
                            },
                            ActionButton {
                                label: format!("Mode: {mode_label}"),
                                callback_data: format!("mt:{id}"),
                            },
                        ]);
                        buttons.push(vec![ActionButton {
                            label: "Back to task".into(),
                            callback_data: format!("td:{tid}"),
                        }]);

                        let icon = status.icon();
                        let label = status.label();
                        (
                            CoreResponse::Confirmation {
                                message: format!(
                                    "{task_name} | {display_name} — {icon} {label}"
                                ),
                            },
                            buttons,
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — entity no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "ap" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_task(id) {
                    Some(task_name) => {
                        let task_name = task_name.to_string();
                        let tid = self.register_task(&task_name);
                        let buttons = vec![
                            vec![
                                ActionButton {
                                    label: "Claude".into(),
                                    callback_data: format!("apc:{tid}"),
                                },
                                ActionButton {
                                    label: "Codex".into(),
                                    callback_data: format!("apx:{tid}"),
                                },
                                ActionButton {
                                    label: "Terminal".into(),
                                    callback_data: format!("apt:{tid}"),
                                },
                            ],
                            vec![ActionButton {
                                label: "Cancel".into(),
                                callback_data: format!("td:{tid}"),
                            }],
                        ];
                        (
                            CoreResponse::Confirmation {
                                message: format!("Select agent type for new pane in {task_name}"),
                            },
                            buttons,
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — task no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "apc" | "apx" | "apt" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                let agent = match action {
                    "apc" => Some("claude".to_string()),
                    "apx" => Some("codex".to_string()),
                    "apt" => Some("terminal".to_string()),
                    _ => unreachable!(),
                };
                match self.resolve_task(id) {
                    Some(task_name) => {
                        let task_name = task_name.to_string();
                        self.handle_command(
                            &CoreCommand::AddPane {
                                task_name,
                                pane_name: None,
                                agent,
                            },
                            core,
                            terminal,
                            store,
                            tasks,
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — task no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "kp" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_entity(id) {
                    Some((task, pane_id)) => {
                        let task = task.to_string();
                        let pane_id = pane_id.to_string();
                        let pane_name = tasks
                            .iter()
                            .find(|t| t.name == task)
                            .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
                            .map(|tp| tp.name.clone())
                            .unwrap_or_else(|| pane_id.clone());
                        self.handle_command(
                            &CoreCommand::KillPane {
                                task_name: task,
                                pane_name,
                            },
                            core,
                            terminal,
                            store,
                            tasks,
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — entity no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "rn" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_entity(id) {
                    Some((task, pane_id)) => {
                        let task = task.to_string();
                        let pane_id = pane_id.to_string();
                        let pane_name = tasks
                            .iter()
                            .find(|t| t.name == task)
                            .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
                            .map(|tp| tp.name.clone())
                            .unwrap_or_else(|| pane_id.clone());
                        let display = pane_name.clone();
                        self.rename_route_pending =
                            Some((task, pane_name, pane_id));
                        (
                            CoreResponse::Confirmation {
                                message: format!(
                                    "Reply to this message with the new name for '{display}'"
                                ),
                            },
                            vec![],
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — entity no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "sr" => self.handle_command(&CoreCommand::FullStatus, core, terminal, store, tasks),
            "bk" => self.handle_command(&CoreCommand::FullStatus, core, terminal, store, tasks),
            "uf" => self.handle_unfocus(),

            "rs" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_entity(id) {
                    Some((task, pane_id)) => {
                        let task = task.to_string();
                        let pane_id = pane_id.to_string();
                        let pane_name = tasks
                            .iter()
                            .find(|t| t.name == task)
                            .and_then(|t| t.panes.iter().find(|tp| tp.pane_id == pane_id))
                            .map(|tp| tp.name.clone());
                        self.handle_command(
                            &CoreCommand::Resume {
                                task_name: task,
                                pane_name,
                            },
                            core,
                            terminal,
                            store,
                            tasks,
                        )
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — entity no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            "mt" => {
                let id: u16 = match id_str.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        return (
                            CoreResponse::Error {
                                message: "Invalid callback data.".into(),
                            },
                            vec![],
                        )
                    }
                };
                match self.resolve_entity(id) {
                    Some((_task_name, pane_id)) => {
                        let pane_id = pane_id.to_string();
                        let current = self.get_pane_mode(&pane_id);
                        let new_mode = match current {
                            PaneOutputMode::Alerts => PaneOutputMode::Stream,
                            PaneOutputMode::Stream => PaneOutputMode::Alerts,
                        };
                        self.pane_modes.insert(pane_id.clone(), new_mode);
                        // Re-render pane detail with updated mode button
                        self.handle_callback(&format!("pd:{id}"), core, terminal, store, tasks)
                    }
                    None => (
                        CoreResponse::Error {
                            message: "Stale button — entity no longer tracked.".into(),
                        },
                        vec![],
                    ),
                }
            }

            _ => {
                warn!(%data, "unknown callback action");
                (
                    CoreResponse::Error {
                        message: "Unknown action.".into(),
                    },
                    vec![],
                )
            }
        }
    }

    fn smart_approve_with_buttons(
        &mut self,
        core: &WagnerCore,
        terminal: &dyn Terminal,
        tasks: &[Task],
    ) -> (CoreResponse, Vec<Vec<ActionButton>>) {
        let mut waiting_panes: Vec<(String, String, String)> = vec![];

        for task in tasks {
            let session_name = session_name_for_task(&task.name);
            let panes = terminal
                .list_panes(&SessionHandle(session_name.clone()))
                .unwrap_or_default();
            for pane in &panes {
                let status = core
                    .status_engine
                    .get_pane_status(&session_name, &pane.0)
                    .cloned()
                    .unwrap_or(PaneStatus::Unknown);
                if status.is_waiting() {
                    let name = task
                        .panes
                        .iter()
                        .find(|tp| tp.pane_id == pane.0)
                        .map(|tp| tp.name.clone())
                        .unwrap_or_else(|| pane.1.clone());
                    waiting_panes.push((task.name.clone(), pane.0.clone(), name));
                }
            }
        }

        match waiting_panes.len() {
            0 => (
                CoreResponse::Error {
                    message: "No panes are waiting for approval.".into(),
                },
                vec![],
            ),
            1 => {
                let (task_name, pane_id, pane_name) = &waiting_panes[0];
                let handle = PaneHandle(pane_id.clone(), String::new());
                if let Err(e) = terminal.send_key(&handle, "y") {
                    return (
                        CoreResponse::Error {
                            message: format!("Failed to approve: {e}"),
                        },
                        vec![],
                    );
                }
                let _ = terminal.send_key(&handle, "Enter");
                let engine = tasks
                    .iter()
                    .flat_map(|t| &t.panes)
                    .find(|p| p.pane_id == *pane_id)
                    .map(|p| p.engine)
                    .unwrap_or(Engine::ClaudeCode);
                let baseline = terminal
                    .capture(&handle, 500)
                    .map(|s: String| s.lines().count())
                    .unwrap_or(0);
                self.reply_route_pending = Some((task_name.clone(), pane_id.clone()));
                self.awaiting_response.insert(pane_id.clone(), (task_name.clone(), baseline, engine));
                (
                    CoreResponse::Confirmation {
                        message: format!("Approved {task_name} | {pane_name}"),
                    },
                    vec![],
                )
            }
            _ => {
                let buttons: Vec<Vec<ActionButton>> = waiting_panes
                    .iter()
                    .map(|(task, pane_id, pane_name)| {
                        let eid = self.register_entity(task, pane_id);
                        vec![ActionButton {
                            label: format!("Approve {task} | {pane_name}"),
                            callback_data: format!("a:{eid}"),
                        }]
                    })
                    .collect();
                (
                    CoreResponse::Confirmation {
                        message: format!(
                            "{} panes waiting. Choose one:",
                            waiting_panes.len()
                        ),
                    },
                    buttons,
                )
            }
        }
    }
}

enum TelegramInput {
    Command(ParsedCommand, Option<i32>), // optional reply_to_message_id for context
    Reply {
        reply_to_message_id: i32,
        text: String,
    },
    Callback {
        data: String,
    },
}

impl Adapter for TelegramAdapter {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn handle_events(
        &mut self,
        events: &[CoreEvent],
        core: &WagnerCore,
        terminal: &dyn Terminal,
        _store: &Store,
        _tasks: &[Task],
    ) -> crate::Result<()> {
        for event in events {
            if let Err(e) = self.dispatch_event(event, terminal, &core.config).await {
                warn!(%e, "telegram event dispatch error");
            }
        }
        Ok(())
    }

    async fn poll_and_handle(
        &mut self,
        core: &WagnerCore,
        terminal: &dyn Terminal,
        store: &Store,
        tasks: &[Task],
    ) -> crate::Result<()> {
        let inputs = self.poll_telegram().await?;

        for (input, msg_ref) in inputs {
            let (response, buttons) = match input {
                TelegramInput::Command(ParsedCommand::Core(cmd), reply_ctx) => {
                    self.handle_command_with_context(&cmd, reply_ctx, core, terminal, store, tasks)
                }
                TelegramInput::Command(ParsedCommand::Focus {
                    task_name,
                    pane_name,
                    sticky,
                }, _) => self.handle_focus(&task_name, pane_name.as_deref(), sticky),
                TelegramInput::Command(ParsedCommand::Unfocus, _) => self.handle_unfocus(),
                TelegramInput::Command(ParsedCommand::UsageError { usage }, _) => (
                    CoreResponse::Error {
                        message: format!("Usage: {usage}"),
                    },
                    vec![],
                ),
                TelegramInput::Command(ParsedCommand::Unknown { text }, _) => (
                    CoreResponse::Error {
                        message: format!("Unknown command: {text}. /help for available commands."),
                    },
                    vec![],
                ),
                TelegramInput::Reply {
                    reply_to_message_id,
                    text,
                } => self.handle_reply(reply_to_message_id, &text, terminal, core, store, tasks),
                TelegramInput::Callback { data } => {
                    self.handle_callback(&data, core, terminal, store, tasks)
                }
            };

            let pane_assoc = self.reply_route_pending.take();
            let rename_assoc = self.rename_route_pending.take();
            match self
                .send_response_text(&response, &buttons, Some(&msg_ref))
                .await
            {
                Ok(Some(sent)) => {
                    if let Some((task, pane_id)) = pane_assoc {
                        self.message_to_pane
                            .insert(sent.message_id, (task, pane_id));
                    }
                    if let Some(rename_info) = rename_assoc {
                        self.pending_rename.insert(sent.message_id, rename_info);
                    }
                }
                Err(e) => {
                    warn!(%e, "telegram response send error");
                }
                _ => {}
            }
        }

        Ok(())
    }
}

fn truncate_message(text: &str) -> String {
    if text.len() <= MAX_MESSAGE_LEN {
        return text.to_string();
    }

    if let Some(last_open) = text.rfind("```\n") {
        let before_code = &text[..last_open];
        let remaining_budget = MAX_MESSAGE_LEN
            .saturating_sub(before_code.len())
            .saturating_sub("```\n".len())
            .saturating_sub("\n\\.\\.\\.\n```".len());
        if remaining_budget > 100 {
            let code_start = last_open + "```\n".len();
            let code_content = &text[code_start..];
            let code_body = code_content.strip_suffix("\n```").unwrap_or(code_content);
            let truncated_code = &code_body[..remaining_budget.min(code_body.len())];
            return format!("{before_code}```\n{truncated_code}\n\\.\\.\\.\n```");
        }
    }

    let mut end = MAX_MESSAGE_LEN;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n\\.\\.\\.", &text[..end])
}

fn build_keyboard(buttons: &[Vec<ActionButton>]) -> Option<InlineKeyboardMarkup> {
    if buttons.is_empty() || buttons.iter().all(|row| row.is_empty()) {
        return None;
    }
    let rows: Vec<Vec<InlineKeyboardButton>> = buttons
        .iter()
        .filter(|row| !row.is_empty())
        .map(|row| {
            row.iter()
                .map(|b| InlineKeyboardButton::callback(&b.label, &b.callback_data))
                .collect()
        })
        .collect();
    Some(InlineKeyboardMarkup::new(rows))
}

fn build_attention_actions(
    entity_id: u16,
    reason: &crate::monitor::status::WaitReason,
    suppressed_count: u32,
    focused: bool,
) -> Vec<Vec<ActionButton>> {
    use crate::monitor::status::WaitReason;

    let mut row1 = vec![];

    match reason {
        WaitReason::Approval | WaitReason::Permission => {
            row1.push(ActionButton {
                label: "Approve".into(),
                callback_data: format!("a:{entity_id}"),
            });
            row1.push(ActionButton {
                label: "Reject".into(),
                callback_data: format!("r:{entity_id}"),
            });
        }
        WaitReason::Question | WaitReason::Input => {}
    }

    row1.push(ActionButton {
        label: "Output".into(),
        callback_data: format!("o:{entity_id}"),
    });

    let mut row2 = vec![];
    if focused {
        let label = if suppressed_count > 0 {
            format!("Unfocus ({suppressed_count} suppressed)")
        } else {
            "Unfocus".into()
        };
        row2.push(ActionButton {
            label,
            callback_data: "uf".into(),
        });
    } else {
        row2.push(ActionButton {
            label: "Focus".into(),
            callback_data: format!("fp:{entity_id}"),
        });
    }

    vec![row1, row2]
}

fn capture_tail(terminal: &dyn Terminal, pane: &PaneHandle, lines: usize) -> String {
    terminal
        .capture(pane, lines)
        .map(|s| strip_ansi(&s))
        .unwrap_or_default()
}

fn strip_tui_chrome(output: &str, engine: Engine) -> String {
    match engine {
        Engine::Codex => {
            let lines: Vec<&str> = output.lines().collect();
            let mut end = lines.len();
            // Strip from bottom: empty lines, status bar, suggestion prompts
            while end > 0 {
                let trimmed = lines[end - 1].trim();
                if trimmed.is_empty()
                    || trimmed.contains("% left")
                    || trimmed.starts_with('\u{203a}') // ›
                    || trimmed.starts_with('>')
                {
                    end -= 1;
                } else {
                    break;
                }
            }
            lines[..end].join("\n")
        }
        _ => output.to_string(),
    }
}
