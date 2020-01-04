use crate::bot::{Bot, Error, UpdateStream};
use crate::shutdown::Shutdown;
use crate::utils;
use futures::channel::oneshot::{channel, Receiver};
use futures::future::{self, Either, TryFutureExt as _};
use futures::stream::StreamExt as _;
use log::{debug, error, info, warn};
use reqwest::Client;
use std::env::{self, VarError};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use telegram_types::bot::types::{Update, UpdateContent};
use tokio::time::delay_for;

pub fn run<Impl, Creator, Handler, HandleResult>(
    name: &'static str,
    token_env: &'static str,
    client: &Client,
    shutdown: Arc<Shutdown>,
    create_impl: Creator,
    handle_update: Handler,
    report_error: fn(&Bot, &Error),
) -> (
    impl Future<Output = Result<(), ()>> + Send,
    Receiver<Result<Option<Bot>, ()>>,
)
where
    Impl: Send + Sync + 'static,
    Creator: (FnOnce(Bot) -> Impl) + Send + 'static,
    Handler: (Fn(Arc<Impl>, Update) -> HandleResult) + Send + Sync + 'static,
    HandleResult: Future<Output = Result<(), ()>> + Send + 'static,
{
    let (sender, receiver) = channel();
    let token = match env::var(token_env) {
        Ok(token) => Box::leak(token.into_boxed_str()),
        Err(VarError::NotPresent) => {
            info!("{} wouldn't start because {} is not set", name, token_env);
            sender.send(Ok(None)).unwrap();
            return (Either::Left(future::ok(())), receiver);
        }
        Err(VarError::NotUnicode(s)) => {
            panic!("invalid value for {}: {:?}", token_env, s);
        }
    };
    let client = client.clone();
    let future = async move {
        let bot = match Bot::create(client, token).await {
            Ok(bot) => bot,
            Err(e) => {
                error!("failed to init bot for {}: {:?}", name, e);
                sender.send(Err(())).unwrap();
                return Err(());
            }
        };
        sender.send(Ok(Some(bot.clone()))).unwrap();
        let stop_signal = shutdown.register();
        let result = future::select(
            stop_signal,
            Box::pin(run_bot(
                bot.get_updates(),
                Arc::new(create_impl(bot)),
                handle_update,
                shutdown,
                report_error,
            )),
        );
        match result.await {
            Either::Left((result, _)) => match result {
                Ok(()) => Ok(()),
                Err(err) => unreachable!("shutdown signal dies: {:?}", err),
            },
            Either::Right(((), _)) => Err(()),
        }
    };
    (Either::Right(future), receiver)
}

async fn run_bot<Impl, Handler, HandleResult>(
    mut stream: UpdateStream,
    bot_impl: Arc<Impl>,
    handle_update: Handler,
    shutdown: Arc<Shutdown>,
    report_error: fn(&Bot, &Error),
) where
    Handler: Fn(Arc<Impl>, Update) -> HandleResult,
    HandleResult: Future<Output = Result<(), ()>> + Send + 'static,
{
    let mut retried = 0;
    let mut delay = None;
    loop {
        if let Some(delay) = &mut delay {
            delay.await;
        }
        delay = None;

        match stream.next().await {
            None => unreachable!("update stream never ends"),
            Some(Ok(update)) => {
                retried = 0;
                debug!("{}> handling", update.update_id.0);
                if !may_handle_common_command(&update, stream.bot(), &shutdown) {
                    tokio::spawn((handle_update)(bot_impl.clone(), update));
                }
            }
            Some(Err(e)) => {
                (report_error)(stream.bot(), &e);
                warn!("({}) telegram error: {:?}", retried, e);
                if retried >= 13 {
                    error!("retried too many times!");
                    break;
                } else {
                    let delay_duration = Duration::from_secs(1 << retried);
                    delay = Some(delay_for(delay_duration));
                    retried += 1;
                }
            }
        }
    }
}

fn may_handle_common_command(update: &Update, bot: &Bot, shutdown: &Shutdown) -> bool {
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
    let send_reply = |text: &str| {
        let future = bot
            .send_message(chat_id, text)
            .execute()
            .map_ok(move |msg| {
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
            shutdown.shutdown();
            tokio::spawn(bot.confirm_update(update_id).map_err(|e| {
                error!("failed to confirm: {:?}", e);
            }));
        }
        _ => return false,
    }
    true
}
