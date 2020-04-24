use self::record::RecordService;
use crate::bot::Bot;
use crate::eval::parse::Command;
use crate::utils;
use futures::future;
use log::{debug, info, warn};
use reqwest::Client;
use std::future::Future;
use std::sync::Arc;
use telegram_types::bot::types::{Message, Update, UpdateContent, UpdateId};
use tokio::sync::Mutex;

mod execute;
mod parse;
mod record;

/// Eval bot.
pub struct EvalBot {
    bot: Bot,
    client: Client,
    records: Mutex<RecordService>,
}

impl EvalBot {
    /// Create new eval bot instance.
    pub fn new(client: Client, bot: Bot) -> Self {
        let records = Mutex::new(RecordService::init());
        info!("EvalBot authorized as @{}", bot.username);
        EvalBot {
            bot,
            client,
            records,
        }
    }

    /// Handle the update.
    pub async fn handle_update(self: Arc<Self>, update: Update) {
        let id = update.update_id;
        match update.content {
            UpdateContent::Message(message) => self.handle_message(id, &message).await,
            UpdateContent::EditedMessage(message) => self.handle_edit_message(id, &message).await,
            _ => {}
        }
    }

    async fn handle_message(&self, id: UpdateId, message: &Message) {
        self.records.lock().await.clear_old_records(&message.date);
        let reply_future = match self.prepare_command(id, message) {
            Some(future) => async { generate_reply(future.await) },
            None => return,
        };
        let msg_id = message.message_id;
        let date = message.date.clone();
        self.records.lock().await.push_record(msg_id, date);
        let chat_id = message.chat.id;

        // Send the placeholder reply.
        let placeholder_future = async {
            let text = "<em>Processing...</em>";
            let request = self.bot.send_message(chat_id, text);
            match request.execute().await {
                Ok(msg) => {
                    let reply_id = msg.message_id;
                    debug!("{}> placeholder sent as {}", id.0, reply_id.0);
                    self.records.lock().await.set_reply(msg_id, reply_id);
                    Ok(reply_id)
                }
                Err(err) => Err(warn!("{}> error sending: {:?}", id.0, err)),
            }
        };

        // Update the reply to the real result.
        let (placeholder, reply) = future::join(placeholder_future, reply_future).await;
        let reply_id = match placeholder {
            Ok(reply_id) => reply_id,
            Err(()) => return,
        };

        let reply = reply.trim_matches(char::is_whitespace);
        debug!("{}> updating reply: {:?}", id.0, reply);
        let request = self.bot.edit_message(chat_id, reply_id, reply);
        match request.execute().await {
            Ok(_) => debug!("{}> reply sent", id.0),
            Err(err) => warn!("{}> error updating: {:?}", id.0, err),
        }
    }

    async fn handle_edit_message(&self, id: UpdateId, message: &Message) {
        let msg_id = message.message_id;
        let reply_id = match self.records.lock().await.find_reply(msg_id) {
            Some(reply) => reply,
            None => return,
        };
        let chat_id = message.chat.id;
        let reply_future = match self.prepare_command(id, message) {
            Some(future) => async { generate_reply(future.await) },
            None => {
                // Delete reply if the new command is invalid.
                debug!("{}> deleting", id.0);
                self.records.lock().await.remove_reply(msg_id);
                let request = self.bot.delete_message(chat_id, reply_id);
                match request.execute().await {
                    Ok(_) => debug!("{}> deleted", id.0),
                    Err(err) => warn!("{}> error deleting: {:?}", id.0, err),
                }
                return;
            }
        };

        // Update the reply with a placeholder.
        let placeholder_future = async {
            let text = "<em>Updating...</em>";
            let request = self.bot.edit_message(chat_id, reply_id, text);
            match request.execute().await {
                Ok(_) => debug!("{}> placeholder updated", id.0),
                Err(err) => warn!("{}> error updating placeholder: {:?}", id.0, err),
            }
        };

        // Update the reply to the real result.
        let (_placeholder, reply) = future::join(placeholder_future, reply_future).await;
        let reply = reply.trim_matches(char::is_whitespace);
        debug!("{}> updating: {:?}", id.0, reply);
        let request = self.bot.edit_message(chat_id, reply_id, reply);
        match request.execute().await {
            Ok(_) => debug!("{}> updated", id.0),
            Err(err) => warn!("{}> error updating: {:?}", id.0, err),
        }
    }

    fn prepare_command<'p>(
        &'p self,
        id: UpdateId,
        message: &'p Message,
    ) -> Option<impl Future<Output = Result<String, reqwest::Error>> + 'p> {
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
        let Command {
            bot_name,
            flags,
            content,
        } = parse::parse_command(&command)?;
        if let Some(name) = bot_name {
            if name != self.bot.username {
                return None;
            }
        }
        execute::execute(&self.client, content, flags, is_private)
    }
}

fn generate_reply(reply: Result<String, reqwest::Error>) -> String {
    match reply {
        Ok(reply) => reply,
        Err(err) => {
            if err.is_builder() {
                "error: builder error".into()
            } else if err.is_redirect() {
                "error: failed to request".into()
            } else if err.is_timeout() {
                "error: timeout".into()
            } else if let Some(status) = err.status() {
                format!("error: status code: {}", status)
            } else {
                "error: unknown error".into()
            }
        }
    }
}
