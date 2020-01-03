use self::record::RecordService;
use crate::bot::Bot;
use crate::utils;
use futures::future::{self, FutureExt as _, TryFutureExt as _};
use log::{debug, info, warn};
use parking_lot::Mutex;
use reqwest::Client;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use telegram_types::bot::types::{Message, Update, UpdateContent, UpdateId};

mod execute;
mod parse;
mod record;

/// Eval bot.
pub struct EvalBot {
    bot: Bot,
    client: Client,
    records: Arc<Mutex<RecordService>>,
}

type PinBoxFuture = Pin<Box<dyn Future<Output = Result<(), ()>> + Send>>;

impl EvalBot {
    /// Create new eval bot instance.
    pub fn new(client: Client, bot: Bot) -> Self {
        let records = Arc::new(Mutex::new(RecordService::init()));
        info!("EvalBot authorized as @{}", bot.username);
        EvalBot {
            bot,
            client,
            records,
        }
    }

    /// Handle the update.
    pub fn handle_update(&self, update: Update) -> PinBoxFuture {
        let id = update.update_id;
        match update.content {
            UpdateContent::Message(message) => self.handle_message(id, &message),
            UpdateContent::EditedMessage(message) => self.handle_edit_message(id, &message),
            _ => Box::pin(future::ok(())),
        }
    }

    fn handle_message(&self, id: UpdateId, message: &Message) -> PinBoxFuture {
        self.records.lock().clear_old_records(&message.date);
        let future = match self.prepare_command(id, message) {
            Some(future) => future,
            None => return Box::pin(future::ok(())),
        };
        let msg_id = message.message_id;
        self.records
            .lock()
            .push_record(msg_id, message.date.clone());
        let bot = self.bot.clone();
        let chat_id = message.chat.id;

        // Send the placeholder reply.
        let records = self.records.clone();
        let placeholder_future = bot
            .send_message(chat_id, "<em>Processing...</em>")
            .execute()
            .map_ok(move |msg| {
                let reply_id = msg.message_id;
                debug!("{}> placeholder sent as {}", id.0, reply_id.0);
                records.lock().set_reply(msg_id, reply_id);
                reply_id
            })
            .map_err(move |err| warn!("{}> error: {:?}", id.0, err));

        // Update the reply to the real result.
        let future = future::try_join(
            future.then(|reply| future::ok(generate_reply(reply))),
            placeholder_future,
        );
        Box::pin(future.and_then(move |(reply, reply_id)| {
            let reply = reply.trim_matches(char::is_whitespace);
            debug!("{}> updating reply: {:?}", id.0, reply);
            bot.edit_message(chat_id, reply_id, reply)
                .execute()
                .map_ok(move |_| debug!("{}> reply sent", id.0))
                .map_err(move |err| warn!("{}> error: {:?}", id.0, err))
        }))
    }

    fn handle_edit_message(&self, id: UpdateId, message: &Message) -> PinBoxFuture {
        let msg_id = message.message_id;
        let reply_id = match self.records.lock().find_reply(msg_id) {
            Some(reply) => reply,
            None => return Box::pin(future::ok(())),
        };
        let chat_id = message.chat.id;
        let bot = self.bot.clone();
        let future = match self.prepare_command(id, message) {
            Some(future) => future,
            None => {
                // Delete reply if the new command is invalid.
                debug!("{}> deleting", id.0);
                self.records.lock().remove_reply(msg_id);
                return Box::pin(
                    bot.delete_message(chat_id, reply_id)
                        .execute()
                        .map_ok(move |_| debug!("{}> deleted", id.0))
                        .map_err(move |err| warn!("{}> error: {:?}", id.0, err)),
                );
            }
        };

        // Update the reply with a placeholder.
        let placeholder_future = bot
            .edit_message(chat_id, reply_id, "<em>Updating...</em>")
            .execute()
            .map_ok(move |_| debug!("{}> placeholder updated", id.0))
            .map_err(move |err| warn!("{}> error: {:?}", id.0, err));

        // Update the reply to the real result.
        let future = future::try_join(
            future.then(|reply| future::ok(generate_reply(reply))),
            placeholder_future,
        );
        Box::pin(future.and_then(move |(reply, _)| {
            let reply = reply.trim_matches(char::is_whitespace);
            debug!("{}> updating: {:?}", id.0, reply);
            bot.edit_message(chat_id, reply_id, reply)
                .execute()
                .map_ok(move |_| debug!("{}> updated", id.0))
                .map_err(move |err| warn!("{}> error: {:?}", id.0, err))
        }))
    }

    fn prepare_command(
        &self,
        id: UpdateId,
        message: &Message,
    ) -> Option<impl Future<Output = Result<String, &'static str>>> {
        // Don't care about messages not sent from a user.
        let from = message.from.as_ref()?;
        // Don't care about non-text messages.
        let command = message.text.as_ref()?;
        debug!(
            "{}> received from {}({}): [{}] {:?}",
            id.0,
            from.username
                .as_ref()
                .map_or("[no username]", |s| s.as_str()),
            from.id.0,
            message.message_id.0,
            command
        );
        let is_private = utils::is_message_from_private_chat(&message);
        let (flags, content) = parse::parse_command(&command)?;
        execute::execute(&self.client, content, flags, is_private)
    }
}

fn generate_reply(reply: Result<String, &str>) -> String {
    match reply {
        Ok(reply) => reply,
        Err(err) => format!("error: {}", err),
    }
}
