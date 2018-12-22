use crate::bot::{Bot, Error, UpdateStream};
use crate::shutdown::Shutdown;
use crate::utils;
use futures::future::Either;
use futures::sync::oneshot::{channel, Receiver};
use futures::{Async, Future, IntoFuture, Poll, Stream};
use log::{debug, error, info, warn};
use reqwest::r#async::Client;
use std::env::{self, VarError};
use std::sync::Arc;
use std::time::Duration;
use telegram_types::bot::types::{Update, UpdateContent};
use tokio_timer::Delay;

pub fn run<Impl, Creator, Handler, HandleResult>(
    name: &'static str,
    token_env: &'static str,
    client: &Client,
    shutdown: Arc<Shutdown>,
    create_impl: Creator,
    handle_update: Handler,
    report_error: fn(&Bot, &Error),
) -> (
    impl Future<Item = (), Error = ()> + Send,
    Receiver<Result<Option<Bot>, ()>>,
)
where
    Impl: Send + Sync + 'static,
    Creator: (FnOnce(Bot) -> Impl) + Send + 'static,
    Handler: (Fn(&Impl, Update) -> HandleResult) + Send + Sync + 'static,
    HandleResult: Future<Item = (), Error = ()> + Send + 'static,
{
    let (sender, receiver) = channel();
    let token = match env::var(token_env) {
        Ok(token) => Box::leak(token.into_boxed_str()),
        Err(VarError::NotPresent) => {
            info!("{} wouldn't start because {} is not set", name, token_env);
            sender.send(Ok(None)).unwrap();
            return (Either::A(Ok(()).into_future()), receiver);
        }
        Err(VarError::NotUnicode(s)) => {
            panic!("invalid value for {}: {:?}", token_env, s);
        }
    };
    let future = Bot::create(client.clone(), token)
        .then(move |bot_result| {
            let result = bot_result.map_err(|e| error!("failed to init bot for {}: {:?}", name, e));
            sender.send(result.clone().map(Some)).unwrap();
            result
        })
        .and_then(move |bot| BotRun {
            stream: bot.get_updates(shutdown.register()),
            bot_impl: create_impl(bot),
            handle_update,
            retried: 0,
            delay: None,
            shutdown,
            report_error,
        });
    (Either::B(future), receiver)
}

struct BotRun<Impl, Handler> {
    stream: UpdateStream,
    bot_impl: Impl,
    handle_update: Handler,
    retried: usize,
    delay: Option<Delay>,
    shutdown: Arc<Shutdown>,
    report_error: fn(&Bot, &Error),
}

impl<Impl, Handler> Future for BotRun<Impl, Handler>
where
    Self: UpdateHandler,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        loop {
            if let Some(delay) = &mut self.delay {
                match delay.poll() {
                    Ok(Async::NotReady) => break Ok(Async::NotReady),
                    Ok(Async::Ready(())) => {}
                    Err(err) => {
                        error!("timer error: {:?}", err);
                        break Err(());
                    }
                }
            }
            self.delay = None;

            match self.stream.poll() {
                Ok(result) => {
                    match result {
                        Async::NotReady => break Ok(Async::NotReady),
                        Async::Ready(resp) => {
                            self.retried = 0;
                            match resp {
                                Some(update) => {
                                    self.handle_update(update);
                                    // Go through the loop again to ensure that
                                    // we don't get stuck.
                                }
                                None => {
                                    break Ok(Async::Ready(()));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    self.report_error(&e);
                    warn!("({}) telegram error: {:?}", self.retried, e);
                    if self.retried >= 13 {
                        error!("retried too many times!");
                        break Err(());
                    } else {
                        let delay_duration = Duration::from_secs(1 << self.retried);
                        self.delay = Some(tokio_timer::sleep(delay_duration));
                        self.retried += 1;
                    }
                }
            }
        }
    }
}

trait UpdateHandler {
    fn handle_update(&self, update: Update);
}

impl<Impl, Handler, HandleResult> UpdateHandler for BotRun<Impl, Handler>
where
    Handler: Fn(&Impl, Update) -> HandleResult,
    HandleResult: Future<Item = (), Error = ()> + Send + 'static,
{
    fn handle_update(&self, update: Update) {
        debug!("{}> handling", update.update_id.0);
        if !self.may_handle_common_command(&update) {
            tokio::spawn((self.handle_update)(&self.bot_impl, update));
        }
    }
}

impl<Impl, Handler> BotRun<Impl, Handler> {
    fn may_handle_common_command(&self, update: &Update) -> bool {
        let message = match &update.content {
            UpdateContent::Message(message) => message,
            _ => return false,
        };
        if !utils::is_message_from_private_chat(message) {
            return false;
        }
        let command = match &message.text {
            Some(text) => text,
            _ => return false,
        };
        let chat_id = message.chat.id;
        let update_id = update.update_id;
        let bot = self.stream.bot();
        let send_reply = |text: &str| {
            let future = bot
                .send_message(chat_id, text)
                .execute()
                .map(move |msg| {
                    debug!(
                        "{}> sent about message as {}",
                        update_id.0, msg.message_id.0
                    );
                })
                .map_err(move |err| warn!("{}> error: {:?}", update_id.0, err));
            tokio::spawn(future);
        };
        match command.trim() {
            "/about" => {
                send_reply(&crate::ABOUT_MESSAGE);
            }
            "/shutdown" => {
                let is_admin = message
                    .from
                    .as_ref()
                    .map_or(false, |from| from.id == *crate::ADMIN_ID);
                if !is_admin {
                    return false;
                }
                send_reply("start shutting down...");
                self.shutdown.shutdown();
                tokio::spawn(bot.confirm_update(update_id).map_err(|e| {
                    error!("failed to confirm: {:?}", e);
                }));
            }
            _ => return false,
        }
        true
    }

    fn report_error(&self, error: &Error) {
        let bot = self.stream.bot();
        (self.report_error)(bot, error);
    }
}
