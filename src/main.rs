extern crate combine;
extern crate dotenv;
extern crate env_logger;
extern crate fst;
extern crate fst_subseq_ascii_caseless;
extern crate futures;
extern crate futures_retry;
extern crate htmlescape;
extern crate itertools;
extern crate lazy_static;
extern crate log;
extern crate matches;
extern crate notify;
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
extern crate tokio_core;
extern crate tokio_timer;
extern crate unicode_width;
extern crate url;

mod bot;
mod eval;
mod shutdown;
mod upgrade;
mod utils;

use crate::bot::{Bot, Error};
use crate::eval::EvalBot;
use crate::shutdown::Shutdown;
use futures::{Future, Stream};
use futures_retry::{RetryPolicy, StreamRetryExt};
use lazy_static::lazy_static;
use log::{debug, error, info, warn};
use reqwest::r#async::Client;
use signal_hook::iterator::Signals;
use signal_hook::SIGTERM;
use std::borrow::Cow;
use std::cell::Cell;
use std::env;
use std::io::Write;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use telegram_types::bot::types::{ChatId, UserId};
use tokio_core::reactor::Core;

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
    static ref TOKEN: &'static str = Box::leak(
        env::var("TELEGRAM_TOKEN")
            .expect("TELEGRAM_TOKEN must be set!")
            .into_boxed_str()
    );
    static ref ADMIN_ID: Option<UserId> = env::var("BOT_ADMIN_ID")
        .ok()
        .and_then(|s| str::parse(&s).map(UserId).ok());
}

fn main() -> Result<(), Error> {
    // We don't care if we fail to load .env file.
    let _ = dotenv::from_path(std::env::current_dir().unwrap().join(".env"));
    let shutdown = Shutdown::new();
    init_logger();
    init_signal_handler(shutdown.clone());
    upgrade::init(shutdown.clone());
    eval::init();

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    info!("Running as `{}`", USER_AGENT);

    let client = build_client();
    let bot = core.run(Bot::create(client.clone(), &*TOKEN))?;
    let eval_bot = EvalBot::new(client.clone(), bot.clone(), shutdown.clone());

    send_message_to_admin(
        &mut core,
        &bot,
        format!("Start version: {} @{}", VERSION, bot.username),
    )?;
    {
        let counter = Rc::new(Cell::new(0));
        let retried = Cell::new(0);
        let mut handle_update = |update| {
            debug!("{:?}", update);
            let future = eval_bot.handle_update(update);
            counter.set(counter.get() + 1);
            let counter = counter.clone();
            handle.spawn(future.then(move |result| {
                counter.set(counter.get() - 1);
                result
            }));
            // Reset retried counter
            retried.set(0);
            Ok(())
        };
        let future = bot
            .get_updates(shutdown.register())
            .retry(|e| {
                let cur_retried = retried.get();
                warn!("({}) telegram error: {:?}", cur_retried, e);
                if cur_retried >= 13 {
                    error!("retried too many times!");
                    RetryPolicy::ForwardError(e)
                } else {
                    retried.set(cur_retried + 1);
                    RetryPolicy::WaitRetry(Duration::from_secs(1 << cur_retried))
                }
            }).for_each(&mut handle_update);
        core.run(future)?;
        // Waiting for any on-going futures.
        while counter.get() > 0 {
            core.turn(None);
        }
    }
    // Start exiting
    core.run(eval_bot.shutdown())?;
    send_message_to_admin(&mut core, &bot, "bye")?;
    Ok(())
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
        }).init();
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

fn send_message_to_admin<'a>(
    core: &mut Core,
    bot: &Bot,
    text: impl Into<Cow<'a, str>>,
) -> Result<(), Error> {
    let admin_id = match *ADMIN_ID {
        Some(id) => id,
        None => return Ok(()),
    };
    let chat_id = ChatId(admin_id.0);
    core.run(bot.send_message(chat_id, text).execute())
        .map(|_| ())
}
