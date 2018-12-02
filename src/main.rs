extern crate combine;
extern crate dotenv;
extern crate env_logger;
extern crate fst;
extern crate fst_subseq_ascii_caseless;
extern crate futures;
extern crate htmlescape;
extern crate itertools;
extern crate lazy_static;
extern crate log;
extern crate matches;
extern crate notify;
extern crate parking_lot;
extern crate percent_encoding;
extern crate regex;
extern crate reqwest;
extern crate rustdoc_seeker;
extern crate serde;
extern crate serde_json;
extern crate signal_hook;
#[cfg(test)]
extern crate string_cache;
extern crate telegram_types;
extern crate tokio;
extern crate tokio_timer;
extern crate unicode_width;
extern crate url;

mod bot;
mod bot_runner;
mod cratesio;
mod eval;
mod shutdown;
mod upgrade;
mod utils;

use crate::bot::Bot;
use crate::shutdown::Shutdown;
use futures::future::join_all;
use futures::sync::oneshot::Receiver;
use futures::{Future, IntoFuture};
use itertools::Itertools;
use lazy_static::lazy_static;
use log::{error, info};
use reqwest::r#async::Client;
use signal_hook::iterator::Signals;
use signal_hook::SIGTERM;
use std::env;
use std::fmt::Write as FmtWrite;
use std::io::Write as IOWrite;
use std::sync::Arc;
use std::thread;
use telegram_types::bot::types::{ChatId, UserId};
use tokio::runtime::Runtime;

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("VERSION"),
    " / ",
    env!("BUILD_DATE"),
    ")",
);
const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " - ",
    env!("CARGO_PKG_HOMEPAGE"),
);

lazy_static! {
    static ref ADMIN_ID: UserId = env::var("BOT_ADMIN_ID")
        .ok()
        .and_then(|s| str::parse(&s).map(UserId).ok())
        .expect("BOT_ADMIN_ID must be a valid user id");
}

fn main() {
    // We don't care if we fail to load .env file.
    let _ = dotenv::from_path(std::env::current_dir().unwrap().join(".env"));
    let shutdown = Shutdown::new();
    init_logger();
    init_signal_handler(shutdown.clone());
    upgrade::init(shutdown.clone());
    eval::init();

    info!("Running as `{}`", USER_AGENT);

    let mut runtime = Runtime::new().unwrap();
    let client = build_client();

    // Kick off eval bot.
    let client_clone = client.clone();
    let shutdown_clone = shutdown.clone();
    let (eval_future, eval_receiver) = bot_runner::run(
        "eval",
        "EVAL_TELEGRAM_TOKEN",
        &client,
        shutdown.clone(),
        move |bot| eval::EvalBot::new(client_clone, bot, shutdown_clone),
        |eval_bot, update| eval_bot.handle_update(update),
        |eval_bot| eval_bot.shutdown(),
    );
    runtime.spawn(eval_future);

    // Kick off cratesio bot.
    let client_clone = client.clone();
    let (cratesio_future, cratesio_receiver) = bot_runner::run(
        "cratesio",
        "CRATESIO_TELEGRAM_TOKEN",
        &client,
        shutdown.clone(),
        move |bot| cratesio::CratesioBot::new(client_clone, bot),
        |cratesio_bot, update| cratesio_bot.handle_update(update),
        |_| (Ok(()) as Result<(), ()>).into_future(),
    );
    runtime.spawn(cratesio_future);

    // Drop the client otherwise shutdown_on_idle below may be blocked
    // by its connection pool.
    drop(client);

    fn send_message_to_admin(bot: &Bot, msg: String) -> impl Future<Item = (), Error = ()> {
        let chat_id = ChatId(ADMIN_ID.0);
        bot.send_message(chat_id, msg)
            .execute()
            .map(|_| ())
            .map_err(|e| error!("failed to send message to admin: {:?}", e))
    }

    fn bind_name(
        receiver: Receiver<Result<Option<Bot>, ()>>,
        name: &'static str,
    ) -> impl Future<Item = Option<(&'static str, Bot)>, Error = ()> {
        receiver
            .map_err(|_| ())
            .and_then(|b| b)
            .map(move |b| b.map(move |b| (name, b)))
    }
    let (bot, start_msg) = join_all(vec![
        bind_name(eval_receiver, "eval"),
        bind_name(cratesio_receiver, "cratesio"),
    ])
    .map(|bots| {
        let bots = bots.into_iter().filter_map(|info| info).collect_vec();
        let mut start_msg = format!("Start version: {}", VERSION);
        for (name, bot) in bots.iter() {
            write!(&mut start_msg, "\nbot {} @{}", name, bot.username);
        }
        let (_, first_bot) = bots.into_iter().next().expect("no bot configured?");
        (first_bot, start_msg)
    })
    .wait()
    .unwrap();

    // This message will be sent with the original client we created above,
    // so use runtime to run it.
    runtime.spawn(send_message_to_admin(&bot, start_msg));

    // Replace the client inside the bot with a new one so that it doesn't
    // block the shutdown.
    let bot = bot.with_client(build_client());
    // Wait for the runtime to shutdown.
    runtime.shutdown_on_idle().wait().unwrap();

    // Send the final message with the new client.
    let bye = send_message_to_admin(&bot, "bye".to_string());
    // Drop the bot (and its client) so that nothing blocks tokio::run to finish.
    drop(bot);
    tokio::run(bye);
}

fn init_logger() {
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info");
    env_logger::Builder::from_env(env)
        .format(|buf, record| {
            let timestamp = buf.timestamp();
            let level = record.level();
            let level_style = buf.default_level_style(level);
            let write_header = write!(buf, "{:>5} {}: ", level_style.value(level), timestamp);
            let write_module_path = match record.module_path() {
                None => Ok(()),
                Some(mut module_path) => {
                    const THIS_MODULE: &str = module_path!();
                    if module_path.starts_with(THIS_MODULE) {
                        let stripped = &module_path[THIS_MODULE.len()..];
                        if stripped.is_empty() || stripped.starts_with("::") {
                            module_path = stripped;
                        }
                    }
                    if module_path.is_empty() {
                        Ok(())
                    } else {
                        write!(buf, "{}: ", module_path)
                    }
                }
            };
            let write_args = writeln!(buf, "{}", record.args());
            write_header.and(write_module_path).and(write_args)
        })
        .init();
}

fn init_signal_handler(shutdown: Arc<Shutdown>) {
    let signals = Signals::new(&[SIGTERM]).expect("failed to init signal handler");
    thread::spawn(move || {
        for signal in signals.forever() {
            match signal {
                SIGTERM => {
                    info!("SIGTERM");
                    shutdown.shutdown();
                    break;
                }
                _ => unreachable!(),
            }
        }
    });
}

fn build_client() -> Client {
    use reqwest::header::{HeaderMap, USER_AGENT};
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, crate::USER_AGENT.parse().unwrap());
    Client::builder().default_headers(headers).build().unwrap()
}
