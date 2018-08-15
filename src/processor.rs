use futures::{Future, IntoFuture};
use telegram_bot::{Api, CanSendMessage, Message, MessageChat, MessageKind};
use telegram_bot::{ParseMode, Update, UpdateKind};

use super::ADMIN_ID;
use command::{Command, Executor};
use utils;

/// Processor for handling updates from Telegram.
pub struct Processor<'a> {
    api: Api,
    executor: Executor<'a>,
}

impl<'a> Processor<'a> {
    /// Create new Processor.
    pub fn new(api: Api, executor: Executor<'a>) -> Self {
        Processor { api, executor }
    }

    /// Handle the update.
    pub fn handle_update(&mut self, update: Update) -> Box<dyn Future<Item = (), Error = ()>> {
        let id = update.id;
        match update.kind {
            UpdateKind::Message(message) => self.handle_message(id, message),
            _ => Box::new(Ok(()).into_future()),
        }
    }

    fn handle_message(
        &mut self,
        id: i64,
        message: Message,
    ) -> Box<dyn Future<Item = (), Error = ()>> {
        let command = match message.kind {
            MessageKind::Text { ref data, .. } => data,
            _ => return Box::new(Ok(()).into_future()),
        };

        let username = message.from.username.unwrap_or(String::new());
        let user_id = message.from.id;
        info!(
            "{}> received from {}({}): {:?}",
            id, username, user_id, command
        );
        let is_admin = ADMIN_ID.as_ref().map_or(false, |id| &user_id == id);
        let chat = message.chat;
        let is_private = matches!(chat, MessageChat::Private(..));
        let cmd = Command {
            id,
            command,
            is_admin,
            is_private,
        };
        let api = self.api.clone();
        Box::new(self.executor.execute(cmd).and_then(move |reply| {
            let reply = reply.trim_matches(utils::is_separator);
            info!("{}> sending: {:?}", id, reply);
            let mut msg = chat.text(reply);
            msg.parse_mode(ParseMode::Html);
            msg.disable_preview();
            api.send(msg)
                .and_then(move |_| {
                    info!("{}> sent", id);
                    Ok(())
                })
                .map_err(move |err| {
                    info!("{}> error: {:?}", id, err);
                })
        }))
    }
}
