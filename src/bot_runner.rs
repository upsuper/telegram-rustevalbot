use crate::bot::{Bot, Error};
use crate::shutdown::Shutdown;
use crate::task_tracker::TaskSpawner;
use crate::utils;
use futures::channel::oneshot::{channel, Receiver};
use futures::future;
use futures::pin_mut;
use futures::stream::{Stream, StreamExt as _};
use log::{debug, error, info, warn};
use reqwest::Client;
use std::env::{self, VarError};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use telegram_types::bot::types::{Update, UpdateContent, UpdateId};
use tokio::time::sleep;

pub struct BotRunner<'a> {
    pub client: &'a Client,
    pub spawner: &'a Arc<TaskSpawner>,
    pub shutdown: &'a Arc<Shutdown>,
    pub report_error: fn(&Bot, &Error),
}

impl<'a> BotRunner<'a> {
    pub fn run<Impl, Creator, Handler, HandleResult>(
        &self,
        name: &'static str,
        token_env: &'static str,
        create_impl: Creator,
        handle_update: Handler,
    ) -> Receiver<Result<Option<Bot>, ()>>
    where
        Impl: Send + Sync + 'static,
        Creator: (FnOnce(Bot) -> Impl) + Send + 'static,
        Handler: (Fn(Arc<Impl>, UpdateId, UpdateContent) -> HandleResult) + Send + Sync + 'static,
        HandleResult: Future<Output = ()> + Send + 'static,
    {
        let (sender, receiver) = channel();
        let token = match env::var(token_env) {
            Ok(token) => Box::leak(token.into_boxed_str()),
            Err(VarError::NotPresent) => {
                info!("{} wouldn't start because {} is not set", name, token_env);
                sender.send(Ok(None)).unwrap();
                return receiver;
            }
            Err(VarError::NotUnicode(s)) => {
                panic!("invalid value for {token_env}: {s:?}");
            }
        };
        let client = self.client.clone();
        let spawner = self.spawner.clone();
        let shutdown = self.shutdown.clone();
        let report_error = self.report_error;
        self.spawner.spawn(async move {
            let bot = match Bot::create(client, token).await {
                Ok(bot) => bot,
                Err(e) => {
                    error!("failed to init bot for {}: {:?}", name, e);
                    sender.send(Err(())).unwrap();
                    return;
                }
            };
            sender.send(Ok(Some(bot.clone()))).unwrap();
            let stop_signal = shutdown.register();
            let bot_runner = run_bot(
                &bot,
                bot.get_updates(),
                Arc::new(create_impl(bot.clone())),
                handle_update,
                spawner,
                shutdown,
                report_error,
            );
            pin_mut!(bot_runner);
            future::select(stop_signal, bot_runner).await;
        });
        receiver
    }
}

async fn run_bot<Impl, Handler, HandleResult>(
    bot: &Bot,
    stream: impl Stream<Item = Result<Option<Update>, Error>>,
    bot_impl: Arc<Impl>,
    handle_update: Handler,
    spawner: Arc<TaskSpawner>,
    shutdown: Arc<Shutdown>,
    report_error: fn(&Bot, &Error),
) where
    Handler: Fn(Arc<Impl>, UpdateId, UpdateContent) -> HandleResult,
    HandleResult: Future<Output = ()> + Send + 'static,
{
    pin_mut!(stream);
    let mut retried = 0;
    let mut delay = None;
    loop {
        if let Some(delay) = delay.take() {
            delay.await;
        }
        match stream.next().await {
            None => unreachable!("update stream never ends"),
            Some(Ok(maybe_update)) => {
                retried = 0;
                if let Some(Update { update_id, content }) = maybe_update {
                    debug!("{}> handling", update_id.0);
                    let content = content.unwrap_or_default();
                    if !may_handle_common_command(update_id, &content, bot, &spawner, &shutdown) {
                        spawner.spawn((handle_update)(bot_impl.clone(), update_id, content));
                    }
                }
            }
            Some(Err(e)) => {
                (report_error)(bot, &e);
                warn!(
                    "{}: telegram error ({} retries): {:?}",
                    bot.username, retried, e,
                );
                if retried >= 13 {
                    error!("{}: retried too many times!", bot.username);
                    break;
                } else {
                    let delay_duration = Duration::from_secs(1 << retried);
                    delay = Some(sleep(delay_duration));
                    retried += 1;
                }
            }
        }
    }
}

fn may_handle_common_command(
    update_id: UpdateId,
    content: &UpdateContent,
    bot: &Bot,
    spawner: &Arc<TaskSpawner>,
    shutdown: &Shutdown,
) -> bool {
    let message = match &content {
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
    let send_reply = |text: &str| {
        let future = bot.send_message(chat_id, text).execute();
        spawner.spawn(async move {
            match future.await {
                Ok(msg) => debug!(
                    "{}> sent about message as {}",
                    update_id.0, msg.message_id.0
                ),
                Err(err) => warn!("{}> error: {:?}", update_id.0, err),
            }
        });
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
            let bot = bot.clone();
            spawner.spawn(async move {
                let result = bot.confirm_update(update_id).await;
                if let Err(e) = result {
                    error!("failed to confirm: {:?}", e);
                }
            });
        }
        _ => return false,
    }
    true
}
