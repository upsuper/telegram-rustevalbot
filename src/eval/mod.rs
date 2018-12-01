use self::command::{Command, Executor};
use self::record::RecordService;
use crate::bot::{Bot, Error};
use crate::shutdown::Shutdown;
use crate::ADMIN_ID;
use futures::{Future, IntoFuture};
use log::{debug, info, warn};
use matches::matches;
use parking_lot::Mutex;
use reqwest::r#async::Client;
use std::sync::Arc;
use telegram_types::bot::types::{ChatType, Message, Update, UpdateContent, UpdateId};

mod command;
mod record;

pub use self::command::init;

/// Eval bot.
pub struct EvalBot {
    bot: Bot,
    executor: Executor,
    records: Arc<Mutex<RecordService>>,
    shutdown_id: Arc<Mutex<Option<UpdateId>>>,
}

type BoxFuture = Box<dyn Future<Item = (), Error = ()> + Send>;

impl EvalBot {
    /// Create new eval bot instance.
    pub fn new(client: Client, bot: Bot, shutdown: Arc<Shutdown>) -> Self {
        let shutdown_id = Arc::new(Mutex::new(None));
        let executor = Executor::new(client, bot.username, shutdown, shutdown_id.clone());
        let records = Arc::new(Mutex::new(RecordService::init()));
        info!("EvalBot authorized as @{}", bot.username);
        EvalBot {
            bot,
            executor,
            records,
            shutdown_id,
        }
    }

    /// Borrow the underlying bot.
    pub fn bot(&self) -> &Bot {
        &self.bot
    }

    /// Handle the update.
    pub fn handle_update(&self, update: Update) -> BoxFuture {
        let id = update.update_id;
        match update.content {
            UpdateContent::Message(message) => self.handle_message(id, &message),
            UpdateContent::EditedMessage(message) => self.handle_edit_message(id, &message),
            _ => Box::new(Ok(()).into_future()),
        }
    }

    pub fn shutdown(self) -> Box<dyn Future<Item = (), Error = Error> + Send> {
        if let Some(shutdown_id) = self.shutdown_id.lock().take() {
            debug!("{}> confirming", shutdown_id.0);
            let bot = self.bot();
            return Box::new(bot.confirm_update(shutdown_id).map(move |_| {
                debug!("{}> confirmed", shutdown_id.0);
            }));
        }
        Box::new(Ok(()).into_future())
    }

    fn handle_message(&self, id: UpdateId, message: &Message) -> BoxFuture {
        self.records.lock().clear_old_records(&message.date);
        let cmd = match Self::build_command(id, message) {
            Ok(cmd) => cmd,
            Err(()) => return Box::new(Ok(()).into_future()),
        };
        let msg_id = message.message_id;
        self.records
            .lock()
            .push_record(msg_id, message.date.clone());
        let chat_id = message.chat.id;
        let bot = self.bot.clone();
        let records = self.records.clone();
        match self.executor.execute(&cmd) {
            Some(future) => Box::new(future.then(move |reply| {
                let reply = reply.unwrap();
                let reply = reply.trim_matches(char::is_whitespace);
                debug!("{}> sending: {:?}", id.0, reply);
                bot.send_message(chat_id, reply)
                    .execute()
                    .map(move |msg| {
                        let reply_id = msg.message_id;
                        debug!("{}> sent as {}", id.0, reply_id.0);
                        records.lock().set_reply(msg_id, reply_id);
                    })
                    .map_err(move |err| warn!("{}> error: {:?}", id.0, err))
            })),
            None => Box::new(Err(()).into_future()),
        }
    }

    fn handle_edit_message(&self, id: UpdateId, message: &Message) -> BoxFuture {
        let cmd = match Self::build_command(id, message) {
            Ok(cmd) => cmd,
            // XXX Can this happen at all? Can a text message becomes other types?
            Err(()) => return Box::new(Ok(()).into_future()),
        };
        let msg_id = message.message_id;
        let reply_id = match self.records.lock().find_reply(msg_id) {
            Some(reply) => reply,
            None => return Box::new(Ok(()).into_future()),
        };

        let chat_id = message.chat.id;
        let bot = self.bot.clone();
        let records = self.records.clone();
        match self.executor.execute(&cmd) {
            Some(future) => Box::new(future.then(move |reply| {
                let reply = reply.unwrap();
                let reply = reply.trim_matches(char::is_whitespace);
                debug!("{}> updating: {:?}", id.0, reply);
                bot.edit_message(chat_id, reply_id, reply)
                    .execute()
                    .map(move |_| debug!("{}> updated", id.0))
                    .map_err(move |err| warn!("{}> error: {:?}", id.0, err))
            })),
            None => Box::new({
                debug!("{}> deleting", id.0);
                records.lock().remove_reply(msg_id);
                bot.delete_message(chat_id, reply_id)
                    .execute()
                    .map(move |_| debug!("{}> deleted", id.0))
                    .map_err(move |err| warn!("{}> error: {:?}", id.0, err))
            }),
        }
    }

    fn build_command(id: UpdateId, message: &Message) -> Result<Command, ()> {
        // Don't care about messages not sent from a user.
        let from = match message.from.as_ref() {
            Some(from) => from,
            _ => return Err(()),
        };
        // Don't care about non-text messages.
        let command = match &message.text {
            Some(text) => text,
            _ => return Err(()),
        };
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
        let is_admin = *ADMIN_ID == from.id;
        let is_private = matches!(message.chat.kind, ChatType::Private { .. });
        Ok(Command {
            id,
            command,
            is_admin,
            is_private,
        })
    }
}
