mod commands;
mod outbox;
mod render;

use std::sync::atomic::{AtomicI32, Ordering};

use teloxide::prelude::*;
use teloxide::types::{ChatId, ParseMode, ReplyParameters, UpdateKind};
use tracing::{debug, warn};

use crate::config::TelegramConfig;
use crate::error::WagnerError;
use crate::transport::{
    CommandResponse, MessageRef, RemoteCommand, Transport, TransportEvent,
};

use self::commands::parse_command;
use self::outbox::Outbox;
use self::render::{render_event, render_response};

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

        Ok(Self {
            bot,
            chat_id,
            outbox,
            offset: AtomicI32::new(0),
        })
    }

    async fn do_send(&self, text: &str) -> crate::Result<Option<MessageRef>> {
        let result = self
            .bot
            .send_message(self.chat_id, text)
            .parse_mode(ParseMode::MarkdownV2)
            .await;

        match result {
            Ok(msg) => Ok(Some(MessageRef {
                chat_id: self.chat_id.0,
                message_id: msg.id.0,
            })),
            Err(e) => {
                warn!(%e, "telegram send failed");
                Err(WagnerError::Transport(format!(
                    "Telegram send error: {e}"
                )))
            }
        }
    }

    async fn do_edit(&self, msg_ref: &MessageRef, text: &str) -> crate::Result<Option<MessageRef>> {
        let result = self
            .bot
            .edit_message_text(
                ChatId(msg_ref.chat_id),
                teloxide::types::MessageId(msg_ref.message_id),
                text,
            )
            .parse_mode(ParseMode::MarkdownV2)
            .await;

        match result {
            Ok(_) => Ok(Some(msg_ref.clone())),
            Err(e) => {
                warn!(%e, "telegram edit failed, sending new message");
                self.do_send(text).await
            }
        }
    }

    async fn do_reply(
        &self,
        text: &str,
        reply_to: Option<&MessageRef>,
    ) -> crate::Result<Option<MessageRef>> {
        let mut req = self
            .bot
            .send_message(self.chat_id, text)
            .parse_mode(ParseMode::MarkdownV2);

        if let Some(r) = reply_to {
            req = req.reply_parameters(
                ReplyParameters::new(teloxide::types::MessageId(r.message_id)),
            );
        }

        match req.await {
            Ok(msg) => Ok(Some(MessageRef {
                chat_id: self.chat_id.0,
                message_id: msg.id.0,
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
        let text = render_event(event);
        self.outbox.throttle().await;
        self.do_send(&text).await
    }

    async fn edit_message(
        &self,
        msg_ref: &MessageRef,
        event: &TransportEvent,
    ) -> crate::Result<Option<MessageRef>> {
        let text = render_event(event);
        self.outbox.throttle().await;
        self.do_edit(msg_ref, &text).await
    }

    async fn send_response(
        &self,
        response: &CommandResponse,
        reply_to: Option<&MessageRef>,
    ) -> crate::Result<Option<MessageRef>> {
        let text = render_response(response);
        self.outbox.throttle().await;
        self.do_reply(&text, reply_to).await
    }

    async fn poll_commands(&self) -> crate::Result<Vec<(RemoteCommand, MessageRef)>> {
        let offset = self.offset.load(Ordering::Relaxed);

        let updates = self
            .bot
            .get_updates()
            .offset(offset)
            .timeout(0)
            .limit(10)
            .allowed_updates(vec![teloxide::types::AllowedUpdate::Message])
            .await
            .map_err(|e| WagnerError::Transport(format!("Telegram poll error: {e}")))?;

        let mut commands = Vec::new();

        for update in updates {
            let new_offset = update.id.as_offset();
            self.offset.store(new_offset, Ordering::Relaxed);

            if let UpdateKind::Message(msg) = &update.kind {
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
                    };

                    if let Some(reply_msg) = msg.reply_to_message() {
                        if !text.starts_with('/') {
                            commands.push((
                                RemoteCommand::ReplyInput {
                                    reply_to_message_id: reply_msg.id.0,
                                    text: text.to_string(),
                                },
                                msg_ref,
                            ));
                            continue;
                        }
                    }

                    if let Some(cmd) = parse_command(text) {
                        commands.push((cmd, msg_ref));
                    }
                }
            }
        }

        Ok(commands)
    }
}
