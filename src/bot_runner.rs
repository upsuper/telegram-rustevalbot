use crate::bot::{Bot, Error};
use crate::shutdown::Shutdown;
use futures::{Async, Future, Poll, Stream};
use futures::sync::oneshot::{channel, Receiver};
use log::{error, warn};
use reqwest::r#async::Client;
use std::env;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use telegram_types::bot::types::Update;
use tokio_timer::Delay;

pub fn run<Impl, Creator, Handler, HandleResult, BotShutdown, BotShutdownResult, BotShutdownError>(
    name: &'static str,
    token_env: &'static str,
    client: &Client,
    shutdown: Arc<Shutdown>,
    create_impl: Creator,
    handle_update: Handler,
    bot_shutdown: BotShutdown,
) -> (impl Future<Item = (), Error = ()> + Send, Receiver<Result<Bot, ()>>)
where
    Impl: Send + Sync + 'static,
    Creator: (FnOnce(Bot) -> Impl) + Send + 'static,
    Handler: (Fn(&Impl, Update) -> HandleResult) + Send + Sync + 'static,
    HandleResult: Future<Item = (), Error = ()> + Send + 'static,
    BotShutdown: (FnOnce(Impl) -> BotShutdownResult) + Send + 'static,
    BotShutdownResult: Future<Item = (), Error = BotShutdownError> + Send + 'static,
    BotShutdownError: Debug,
{
    let token = Box::leak(
        env::var(token_env)
            .unwrap_or_else(|e| panic!("{} must be set for {}: {:?}", token_env, name, e))
            .into_boxed_str()
    );
    let (sender, receiver) = channel();
    let future = Bot::create(client.clone(), token)
        .then(move |bot_result| {
            let result = bot_result
                .map_err(|e| error!("failed to init bot for {}: {:?}", name, e));
            sender.send(result.clone()).unwrap();
            result
        })
        .and_then(move |bot| {
            let stream = bot.get_updates(shutdown.register());
            let bot_impl = Some(create_impl(bot));
            BotRun {
                stream,
                bot_impl,
                handle_update,
                retried: 0,
                delay: None,
            }
        })
        .and_then(move |bot_impl| bot_shutdown(bot_impl).map_err(move |e| {
            error!("failed to shutdown {}: {:?}", name, e);
        }));
    (future, receiver)
}

struct BotRun<Updates, Impl, Handler> {
    stream: Updates,
    bot_impl: Option<Impl>,
    handle_update: Handler,
    retried: usize,
    delay: Option<Delay>,
}

impl<Updates, Impl, Handler, HandleResult> Future for BotRun<Updates, Impl, Handler>
where
    Updates: Stream<Item = Update, Error = Error>,
    Handler: Fn(&Impl, Update) -> HandleResult,
    HandleResult: Future<Item = (), Error = ()> + Send + 'static,
{
    type Item = Impl;
    type Error = ();

    fn poll(&mut self) -> Poll<Impl, ()> {
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
                    self.retried = 0;
                    match result {
                        Async::NotReady => break Ok(Async::NotReady),
                        Async::Ready(None) => {
                            break Ok(Async::Ready(self.bot_impl.take().unwrap()));
                        }
                        Async::Ready(Some(update)) => {
                            let bot_impl = self.bot_impl.as_ref().unwrap();
                            tokio::spawn((self.handle_update)(bot_impl, update));
                        }
                    }
                }
                Err(e) => {
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
