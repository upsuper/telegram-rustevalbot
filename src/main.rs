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
extern crate percent_encoding;
extern crate regex;
extern crate reqwest;
extern crate rustdoc_seeker;
extern crate serde;
extern crate serde_json;
extern crate signal_hook;
#[cfg(test)]
extern crate string_cache;
extern crate telegram_bot;
extern crate tokio_core;
extern crate unicode_width;
extern crate url;

mod command;
mod processor;
mod record;
mod shutdown;
mod upgrade;
mod utils;

use futures::future::Either;
use futures::{Future, Stream};
use lazy_static::lazy_static;
use log::{debug, error, info, warn};
use signal_hook::iterator::Signals;
use signal_hook::SIGTERM;
use std::cell::RefCell;
use std::env;
use std::io::Write;
use std::rc::Rc;
use std::thread;
use std::time::Duration;
use telegram_bot::{Api, CanSendMessage, Error, GetMe, GetUpdates, UserId};
use tokio_core::reactor::Core;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("VERSION"), ")",);
const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " - ",
    env!("CARGO_PKG_HOMEPAGE"),
);

lazy_static! {
    static ref ADMIN_ID: Option<UserId> = env::var("BOT_ADMIN_ID")
        .ok()
        .and_then(|s| str::parse(&s).map(UserId::new).ok());
    static ref SHUTDOWN: shutdown::Shutdown = Default::default();
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

fn init_signal_handler() {
    let signals = Signals::new(&[SIGTERM]).expect("failed to init signal handler");
    thread::spawn(move || {
        for signal in signals.forever() {
            match signal {
                SIGTERM => {
                    info!("SIGTERM");
                    SHUTDOWN.shutdown(None);
                    break;
                }
                _ => unreachable!(),
            }
        }
    });
}

fn main() -> Result<(), Error> {
    // We don't care if we fail to load .env file.
    let _ = dotenv::from_path(std::env::current_dir()?.join(".env"));
    init_logger();
    init_signal_handler();
    upgrade::init();
    command::init();

    let mut core = Core::new()?;
    let token = env::var("TELEGRAM_TOKEN").expect("TELEGRAM_TOKEN must be set!");
    info!("Running as `{}`", USER_AGENT);

    let handle = core.handle();
    // Configure Telegram API and get user information of ourselves
    let api = Api::configure(token).build(&handle)?;
    let self_user = core.run(api.send(GetMe))?;
    let self_username = self_user.username.expect("No username?");
    let self_username: &'static str = Box::leak(self_username.into_boxed_str());
    info!("Authorized as @{}", self_username);
    // Build the command executor
    let executor = command::Executor::new(self_username);
    let processor = processor::Processor::new(api.clone(), executor);
    if let Some(id) = &*ADMIN_ID {
        api.spawn(id.text(format!("Start version: {} @{}", VERSION, self_username)));
    }
    let counter = Rc::new(RefCell::new(0));
    let retried = RefCell::new(0);
    let mut handle_update = |update| {
        debug!("{:?}", update);
        let future = processor.handle_update(update);
        let counter_clone = counter.clone();
        *counter.borrow_mut() += 1;
        handle.spawn(future.then(move |result| {
            *counter_clone.borrow_mut() -= 1;
            result
        }));
        // Reset retried counter
        *retried.borrow_mut() = 0;
        Ok(())
    };
    let shutdown_id = loop {
        let future = api
            .stream()
            .for_each(&mut handle_update)
            .select2(SHUTDOWN.renew())
            .then(|result| match result {
                Ok(Either::A(((), _))) => panic!("unexpected stop"),
                Ok(Either::B((id, _))) => Ok(id),
                Err(Either::A((e, _))) => Err(e),
                Err(Either::B((e, _))) => panic!("shutdown canceled? {}", e),
            });
        match core.run(future) {
            Ok(id) => break id,
            Err(e) => {
                let mut retried = retried.borrow_mut();
                warn!("({}) telegram error: {:?}", retried, e);
                if *retried >= 13 {
                    error!("retried too many times!");
                    panic!();
                }
                thread::sleep(Duration::new(1 << *retried, 0));
                *retried += 1;
            }
        }
    };
    // Waiting for any on-going futures.
    while *counter.borrow() > 0 {
        core.turn(None);
    }
    // Start exiting
    if let Some(shutdown_id) = shutdown_id {
        let mut get_updates = GetUpdates::new();
        get_updates.offset(shutdown_id + 1);
        debug!("{}> confirming", shutdown_id);
        core.run(api.send(get_updates).map(move |_| {
            debug!("{}> confirmed", shutdown_id);
        }))?;
    }
    core.run(api.send(ADMIN_ID.unwrap().text("bye")))?;
    Ok(())
}
