mod commands;
mod outbox;
mod render;

use std::sync::atomic::{AtomicI32, Ordering};

use teloxide::prelude::*;
use teloxide::types::{
    AllowedUpdate, BotCommand, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, MessageId,
    ParseMode, ReplyParameters, UpdateKind,
};
use tracing::{debug, warn};

use crate::config::TelegramConfig;
use crate::error::WagnerError;
use crate::transport::{
    ActionButton, CommandResponse, MessageRef, RemoteCommand, Transport,
    TransportEvent,
};

use self::commands::parse_command;
use self::outbox::Outbox;
use self::render::{render_event, render_response};

const MAX_MESSAGE_LEN: usize = 4000;

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

pub struct TelegramTransport {
    bot: Bot,
    chat_id: ChatId,
    outbox: Outbox,
    offset: AtomicI32,
}

impl TelegramTransport {
    pub fn new(config: &TelegramConfig) -> crate::Result<Self> {
        let bot = Bot::new(&config.bot_token);
        let chat_id = ChatId(config.chat_id);
        let outbox = Outbox::new(config.rate_limit_ms);

        // Register bot commands for autocomplete menu (fire and forget)
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
        })
    }

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
                Err(WagnerError::Transport(format!(
                    "Telegram send error: {e}"
                )))
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
            req = req.reply_parameters(
                ReplyParameters::new(MessageId(r.message_id)),
            );
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
                Err(WagnerError::Transport(format!(
                    "Telegram send error: {e}"
                )))
            }
        }
    }
}

impl Transport for TelegramTransport {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn send_event(
        &self,
        event: &TransportEvent,
    ) -> crate::Result<Option<MessageRef>> {
        let rendered = render_event(event);
        let keyboard = build_keyboard(&rendered.buttons);
        self.outbox.throttle().await;
        self.do_send(&rendered.text, keyboard).await
    }

    async fn edit_message(
        &self,
        msg_ref: &MessageRef,
        event: &TransportEvent,
    ) -> crate::Result<Option<MessageRef>> {
        let rendered = render_event(event);
        let keyboard = build_keyboard(&rendered.buttons);
        self.outbox.throttle().await;
        self.do_edit(msg_ref, &rendered.text, keyboard).await
    }

    async fn send_response(
        &self,
        response: &CommandResponse,
        reply_to: Option<&MessageRef>,
    ) -> crate::Result<Option<MessageRef>> {
        let rendered = render_response(response);
        let keyboard = build_keyboard(&rendered.buttons);
        self.outbox.throttle().await;
        if let Some(r) = reply_to.filter(|r| r.edit_in_place) {
            self.do_edit(r, &rendered.text, keyboard).await
        } else {
            self.do_reply(&rendered.text, reply_to, keyboard).await
        }
    }

    async fn poll_commands(&self) -> crate::Result<Vec<(RemoteCommand, MessageRef)>> {
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

        let mut commands = Vec::new();

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

                    if let Some(text) = msg.text() {
                        let msg_ref = MessageRef {
                            chat_id: msg.chat.id.0,
                            message_id: msg.id.0,
                            edit_in_place: false,
                        };

                        if let Some(reply_msg) = msg.reply_to_message() {
                            if !text.starts_with('/') {
                                // Check for y/n shorthand as approve/reject
                                let lower = text.trim().to_lowercase();
                                if matches!(lower.as_str(), "y" | "yes") {
                                    commands.push((
                                        RemoteCommand::ReplyInput {
                                            reply_to_message_id: reply_msg.id.0,
                                            text: "y".to_string(),
                                        },
                                        msg_ref,
                                    ));
                                } else if matches!(lower.as_str(), "n" | "no") {
                                    commands.push((
                                        RemoteCommand::ReplyInput {
                                            reply_to_message_id: reply_msg.id.0,
                                            text: "n".to_string(),
                                        },
                                        msg_ref,
                                    ));
                                } else {
                                    commands.push((
                                        RemoteCommand::ReplyInput {
                                            reply_to_message_id: reply_msg.id.0,
                                            text: text.to_string(),
                                        },
                                        msg_ref,
                                    ));
                                }
                                continue;
                            }
                        }

                        if let Some(cmd) = parse_command(text) {
                            commands.push((cmd, msg_ref));
                        }
                    }
                }

                UpdateKind::CallbackQuery(query) => {
                    // Answer callback immediately to dismiss the spinner
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

                        commands.push((
                            RemoteCommand::Callback {
                                data: data.clone(),
                                source_message_id: source_msg_id,
                            },
                            msg_ref,
                        ));
                    }
                }

                _ => {}
            }
        }

        Ok(commands)
    }
}
