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
use tracing::{debug, info, warn};

use crate::config::TelegramConfig;
use crate::core::WagnerCore;
use crate::error::WagnerError;
use crate::model::Task;
use crate::monitor::status::PaneStatus;
use crate::monitor::strip_ansi;
use crate::store::Store;
use crate::terminal::{PaneHandle, SessionHandle, Terminal, session_name_for_task};
use crate::transport::adapter::Adapter;
use crate::transport::{CoreCommand, CoreEvent, CoreResponse};

use self::commands::{ParsedCommand, parse_command};
use self::outbox::Outbox;
use self::render::{render_event, render_response};

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
    pane_id: Option<String>,
    sticky: bool,
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
    // Authorization
    allowed_users: Vec<i64>,
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
            allowed_users: config.allowed_users.clone(),
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

    fn matches_focus(&self, task_name: &str, pane_id: &str) -> bool {
        match &self.focus {
            None => true,
            Some(f) => {
                if f.task_name != task_name {
                    return false;
                }
                match &f.pane_id {
                    Some(pid) => pid == pane_id,
                    None => true,
                }
            }
        }
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
                pane_id,
                pane_title,
                reason,
                output_tail,
            } => {
                if !self.matches_focus(task_name, pane_id) {
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
                    pane_id: pane_id.clone(),
                    pane_title: pane_title.clone(),
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
                pane_id,
                pane_title,
                ..
            } => {
                if !self.matches_focus(task_name, pane_id) {
                    self.suppressed_count += 1;
                    return Ok(());
                }

                if config.daemon.notify_idle {
                    let pane = PaneHandle(pane_id.clone(), String::new());
                    let output_tail = capture_tail(
                        terminal,
                        &pane,
                        config.daemon.default_output_lines,
                    );
                    let enriched = CoreEvent::AgentIdle {
                        task_name: task_name.clone(),
                        pane_id: pane_id.clone(),
                        pane_title: pane_title.clone(),
                        output_tail,
                    };
                    self.send_event_text(&enriched, &[]).await?;
                }
            }

            CoreEvent::AgentWorking { pane_id, .. } => {
                if let Some(msg_ref) = self.live_messages.remove(pane_id) {
                    self.message_to_pane.remove(&msg_ref.message_id);
                    self.edit_event_text(&msg_ref, event, &[]).await?;
                }
            }

            CoreEvent::SessionStatusChanged { task_name, .. } => {
                let tid = self.register_task(task_name);
                let buttons = vec![vec![ActionButton {
                    label: "Details".into(),
                    callback_data: format!("td:{tid}"),
                }]];
                self.send_event_text(event, &buttons).await?;
            }

            CoreEvent::AgentResumed { .. }
            | CoreEvent::DaemonStarted { .. }
            | CoreEvent::DaemonStopping => {
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

                        if let Some(reply_msg) = msg.reply_to_message() {
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
                        }

                        if let Some(cmd) = parse_command(text) {
                            inputs.push((TelegramInput::Command(cmd), msg_ref));
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
                (response, vec![])
            }

            CoreCommand::TaskStatus { task_name } => {
                let response = core.execute(terminal, store, cmd, tasks);

                let session_name = session_name_for_task(task_name);
                let session_panes = terminal
                    .list_panes(&SessionHandle(session_name.clone()))
                    .unwrap_or_default();

                let mut buttons = vec![];
                for p in &session_panes {
                    let eid = self.register_entity(task_name, &p.0);
                    let status = core
                        .status_engine
                        .get_pane_status(&session_name, &p.0)
                        .cloned()
                        .unwrap_or(PaneStatus::Unknown);
                    let mut row = vec![];
                    if status.is_waiting() {
                        row.push(ActionButton {
                            label: format!("Approve {}", p.1),
                            callback_data: format!("a:{eid}"),
                        });
                    }
                    row.push(ActionButton {
                        label: format!("Output {}", p.1),
                        callback_data: format!("o:{eid}"),
                    });
                    if !row.is_empty() {
                        buttons.push(row);
                    }
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
                buttons.push(vec![ActionButton {
                    label: "Back".into(),
                    callback_data: "bk".into(),
                }]);

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

            CoreCommand::Approve { task_name, .. } => {
                if task_name.is_empty() {
                    return self.smart_approve_with_buttons(core, terminal, tasks);
                }
                let response = core.execute(terminal, store, cmd, tasks);
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

    fn handle_reply(
        &self,
        reply_to_message_id: i32,
        text: &str,
        terminal: &dyn Terminal,
    ) -> (CoreResponse, Vec<Vec<ActionButton>>) {
        match self.message_to_pane.get(&reply_to_message_id) {
            Some((task_name, pane_id)) => {
                let pane = PaneHandle(pane_id.clone(), String::new());
                if let Err(e) = terminal.send_literal(&pane, text) {
                    return (
                        CoreResponse::Error {
                            message: format!("Failed to send: {e}"),
                        },
                        vec![],
                    );
                }
                if let Err(e) = terminal.send_key(&pane, "Enter") {
                    return (
                        CoreResponse::Error {
                            message: format!("Failed to send Enter: {e}"),
                        },
                        vec![],
                    );
                }
                (
                    CoreResponse::Confirmation {
                        message: format!("Sent to {task_name}"),
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
        pane_id: Option<&str>,
        sticky: bool,
    ) -> (CoreResponse, Vec<Vec<ActionButton>>) {
        self.focus = Some(FocusTarget {
            task_name: task_name.to_string(),
            pane_id: pane_id.map(String::from),
            sticky,
        });
        self.suppressed_count = 0;
        let target = match pane_id {
            Some(p) => format!("{task_name}/{p}"),
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
                    Some((task, pane)) => {
                        let task = task.to_string();
                        let pane = pane.to_string();
                        let handle = PaneHandle(pane, String::new());
                        if let Err(e) = terminal.send_key(&handle, "y") {
                            return (
                                CoreResponse::Error {
                                    message: format!("Failed to approve: {e}"),
                                },
                                vec![],
                            );
                        }
                        let _ = terminal.send_key(&handle, "Enter");
                        (
                            CoreResponse::Confirmation {
                                message: format!("Approved {task}"),
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
                    Some((task, pane)) => {
                        let task = task.to_string();
                        let pane = pane.to_string();
                        let handle = PaneHandle(pane, String::new());
                        if let Err(e) = terminal.send_key(&handle, "n") {
                            return (
                                CoreResponse::Error {
                                    message: format!("Failed to reject: {e}"),
                                },
                                vec![],
                            );
                        }
                        let _ = terminal.send_key(&handle, "Enter");
                        (
                            CoreResponse::Confirmation {
                                message: format!("Rejected {task}"),
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
                    Some((task, pane)) => {
                        let task = task.to_string();
                        let pane = pane.to_string();
                        let handle = PaneHandle(pane.clone(), String::new());
                        let lines = core.config.daemon.default_output_lines;
                        let content = capture_tail(terminal, &handle, lines);
                        (
                            CoreResponse::Output {
                                task_name: task,
                                pane_id: pane,
                                content,
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
                    Some((task, pane)) => {
                        let task = task.to_string();
                        let pane = pane.to_string();
                        self.handle_focus(&task, Some(&pane), false)
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

            "sr" => self.handle_command(&CoreCommand::FullStatus, core, terminal, store, tasks),
            "bk" => self.handle_command(&CoreCommand::FullStatus, core, terminal, store, tasks),
            "uf" => self.handle_unfocus(),

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
                    waiting_panes.push((task.name.clone(), pane.0.clone(), pane.1.clone()));
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
                let (task_name, pane_id, _) = &waiting_panes[0];
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
                (
                    CoreResponse::Confirmation {
                        message: format!("Approved {task_name}"),
                    },
                    vec![],
                )
            }
            _ => {
                let buttons: Vec<Vec<ActionButton>> = waiting_panes
                    .iter()
                    .map(|(task, pane_id, pane_title)| {
                        let eid = self.register_entity(task, pane_id);
                        vec![ActionButton {
                            label: format!("Approve {task}/{pane_title}"),
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
    Command(ParsedCommand),
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
                TelegramInput::Command(ParsedCommand::Core(cmd)) => {
                    self.handle_command(&cmd, core, terminal, store, tasks)
                }
                TelegramInput::Command(ParsedCommand::Focus {
                    task_name,
                    pane_id,
                    sticky,
                }) => self.handle_focus(&task_name, pane_id.as_deref(), sticky),
                TelegramInput::Command(ParsedCommand::Unfocus) => self.handle_unfocus(),
                TelegramInput::Command(ParsedCommand::Unknown { text }) => (
                    CoreResponse::Error {
                        message: format!("Unknown command: {text}. /help for available commands."),
                    },
                    vec![],
                ),
                TelegramInput::Reply {
                    reply_to_message_id,
                    text,
                } => self.handle_reply(reply_to_message_id, &text, terminal),
                TelegramInput::Callback { data } => {
                    self.handle_callback(&data, core, terminal, store, tasks)
                }
            };

            if let Err(e) = self
                .send_response_text(&response, &buttons, Some(&msg_ref))
                .await
            {
                warn!(%e, "telegram response send error");
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
