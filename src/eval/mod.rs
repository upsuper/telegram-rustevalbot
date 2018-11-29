use self::processor::Processor;
use crate::bot::{Bot, Error};
use futures::{Future, IntoFuture};
use log::{debug, info};
use reqwest::r#async::Client;
use std::cell::Cell;
use std::rc::Rc;
use telegram_types::bot::types::{Update, UpdateId};

mod command;
mod processor;
mod record;

pub use self::command::init;

pub struct EvalBot {
    processor: Processor,
    shutdown_id: Rc<Cell<Option<UpdateId>>>,
}

impl EvalBot {
    pub fn new(client: Client, bot: Bot) -> Self {
        let shutdown_id = Rc::new(Cell::new(None));
        let executor = command::Executor::new(client, bot.username, shutdown_id.clone());
        let processor = Processor::new(bot, executor);
        info!("EvalBot authorized as @{}", processor.bot().username);
        EvalBot {
            processor,
            shutdown_id,
        }
    }

    pub fn handle_update(&self, update: Update) -> Box<dyn Future<Item = (), Error = ()>> {
        self.processor.handle_update(update)
    }

    pub fn shutdown(self) -> Box<dyn Future<Item = (), Error = Error>> {
        if let Some(shutdown_id) = self.shutdown_id.take() {
            debug!("{}> confirming", shutdown_id.0);
            let bot = self.processor.bot();
            return Box::new(bot.confirm_update(shutdown_id).map(move |_| {
                debug!("{}> confirmed", shutdown_id.0);
            }));
        }
        return Box::new(Ok(()).into_future());
    }
}
