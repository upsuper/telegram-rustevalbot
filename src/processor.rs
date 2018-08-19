use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use futures::{Future, IntoFuture};
use telegram_bot::{Api, CanSendMessage, DeleteMessage, EditMessageText};
use telegram_bot::{Message, MessageChat, MessageId, MessageKind, ParseMode, Update, UpdateKind};

use super::ADMIN_ID;
use command::{Command, Executor};
use utils;

/// Processor for handling updates from Telegram.
pub struct Processor<'a> {
    api: Api,
    executor: Executor<'a>,
    records: Rc<RefCell<VecDeque<Record>>>,
}

type BoxFuture = Box<dyn Future<Item = (), Error = ()>>;

impl<'a> Processor<'a> {
    /// Create new Processor.
    pub fn new(api: Api, executor: Executor<'a>) -> Self {
        Processor {
            api,
            executor,
            records: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    /// Handle the update.
    pub fn handle_update(&self, update: Update) -> BoxFuture {
        let id = update.id;
        match update.kind {
            UpdateKind::Message(message) => self.handle_message(id, message),
            UpdateKind::EditedMessage(message) => self.handle_edit_message(id, message),
            _ => Box::new(Ok(()).into_future()),
        }
    }

    fn handle_message(&self, id: i64, message: Message) -> BoxFuture {
        self.clean_old_records(message.date);
        let cmd = match Self::build_command(id, &message) {
            Ok(cmd) => cmd,
            Err(()) => return Box::new(Ok(()).into_future()),
        };
        let mut record = Record {
            msg: message.id,
            reply: None,
            date: message.date,
        };
        let chat = message.chat.clone();
        let api = self.api.clone();
        let records = self.records.clone();
        match self.executor.execute(cmd) {
            Some(future) => Box::new(future.then(move |reply| {
                let reply = reply.unwrap();
                let reply = reply.trim_matches(utils::is_separator);
                info!("{}> sending: {:?}", id, reply);
                let mut msg = chat.text(reply);
                msg.parse_mode(ParseMode::Html);
                msg.disable_preview();
                api.send(msg)
                    .map(move |reply| {
                        info!("{}> sent as {}", id, reply.id);
                        record.reply = Some(reply.id);
                        records.borrow_mut().push_back(record);
                    })
                    .map_err(move |err| warn!("{}> error: {:?}", id, err))
            })),
            None => Box::new(Err(()).into_future()),
        }
    }

    fn handle_edit_message(&self, id: i64, message: Message) -> BoxFuture {
        let cmd = match Self::build_command(id, &message) {
            Ok(cmd) => cmd,
            // XXX Can this happen at all? Can a text message becomes other types?
            Err(()) => return Box::new(Ok(()).into_future()),
        };
        let msg_id = message.id;
        let reply_id = self
            .records
            .borrow()
            .iter()
            .rev()
            .find(|r| r.msg == msg_id)
            .and_then(|r| r.reply);
        let reply_id = match reply_id {
            Some(reply) => reply,
            None => {
                warn!("{}> reply not found", id);
                return Box::new(Ok(()).into_future());
            }
        };

        let chat = message.chat.clone();
        let api = self.api.clone();
        let records = self.records.clone();
        match self.executor.execute(cmd) {
            Some(future) => Box::new(future.then(move |reply| {
                let reply = reply.unwrap();
                let reply = reply.trim_matches(utils::is_separator);
                info!("{}> updating: {:?}", id, reply);
                let mut msg = EditMessageText::new(chat, reply_id, reply);
                msg.parse_mode(ParseMode::Html);
                msg.disable_preview();
                api.send(msg)
                    .map(move |_| info!("{}> updated", id))
                    .map_err(move |err| warn!("{}> error: {:?}", id, err))
            })),
            None => {
                let delete = DeleteMessage::new(chat, reply_id);
                info!("{}> deleting", id);
                records
                    .borrow_mut()
                    .iter_mut()
                    .rev()
                    .find(|r| r.msg == msg_id)
                    .map(|r| r.reply = None);
                Box::new(
                    api.send(delete)
                        .map(move |_| info!("{}> deleted", id))
                        .map_err(move |err| warn!("{}> error: {:?}", id, err)),
                )
            }
        }
    }

    fn build_command(id: i64, message: &Message) -> Result<Command, ()> {
        let command = match message.kind {
            MessageKind::Text { ref data, .. } => data,
            _ => return Err(()),
        };

        let username = message
            .from
            .username
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("");
        let user_id = message.from.id;
        info!(
            "{}> received from {}({}): [{}] {:?}",
            id, username, user_id, message.id, command
        );
        let is_admin = ADMIN_ID.as_ref().map_or(false, |id| &user_id == id);
        let is_private = matches!(message.chat, MessageChat::Private(..));
        Ok(Command {
            id,
            command,
            is_admin,
            is_private,
        })
    }

    fn clean_old_records(&self, current_date: i64) {
        // We can clean up records up to 48hrs ago, because messages before that cannot be
        // edited anymore.
        let date_to_clean = current_date - 48 * 3600;
        let mut records = self.records.borrow_mut();
        while let Some(record) = records.pop_front() {
            if record.date > date_to_clean {
                records.push_front(record);
                break;
            }
        }
    }
}

struct Record {
    msg: MessageId,
    reply: Option<MessageId>,
    /// Same as Message::date, a UNIX epoch in seconds.
    date: i64,
}
