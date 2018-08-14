extern crate env_logger;
extern crate futures;
extern crate htmlescape;
extern crate itertools;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate matches;
extern crate percent_encoding;
extern crate regex;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate telegram_bot;
extern crate tokio_core;
extern crate unicode_width;

mod command;
mod utils;

use futures::future::Either;
use futures::unsync::oneshot;
use futures::{Future, IntoFuture, Stream};
use std::cell::RefCell;
use std::env;
use std::io::{Error as IOError, ErrorKind as IOErrorKind, Write};
use std::rc::Rc;
use telegram_bot::{
    Api, CanSendMessage, Error, GetMe, GetUpdates, MessageChat, MessageKind, ParseMode, Update,
    UpdateKind, UserId,
};
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
    static ref ADMIN_ID: Option<UserId> =
        env::var("BOT_ADMIN_ID")
            .ok()
            .and_then(|s| str::parse(&s).map(UserId::new).ok());
}

fn init_logger() {
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info");
    env_logger::Builder::from_env(env)
        .format(|buf, record| {
            let timestamp = buf.timestamp();
            let write_header = write!(buf, "{:>5} {}: ", record.level(), timestamp);
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

fn main() -> Result<(), Error> {
    init_logger();

    let mut core = Core::new()?;
    let token = env::var("TELEGRAM_TOKEN").expect("TELEGRAM_TOKEN must be set!");
    info!("Running as `{}`", USER_AGENT);

    let handle = core.handle();
    // Configure Telegram API and get user information of ourselves
    let api = Api::configure(token).build(&handle)?;
    let self_user = core.run(api.send(GetMe))?;
    let self_username = self_user.username.expect("No username?");
    info!("Authorized as @{}", self_username);
    // Build the command executor
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let executor = command::Executor::new(&handle, &self_username, shutdown_sender);
    if let Some(id) = &*ADMIN_ID {
        api.spawn(id.text(format!("Start version: {} @{}", VERSION, self_username)));
    }
    let counter = Rc::new(RefCell::new(0));
    let api_to_move = api.clone();
    let counter_to_move = counter.clone();
    let stream = api.stream().for_each(move |update| {
        debug!("{:?}", update);
        let api = api_to_move.clone();
        let future = handle_update(api, &executor, update);
        let counter = &counter_to_move;
        let counter_clone = counter.clone();
        *counter.borrow_mut() += 1;
        handle.spawn(future.then(move |result| {
            *counter_clone.borrow_mut() -= 1;
            result
        }));
        Ok(())
    });
    let shutdown_id = core.run(
        stream
            .select2(shutdown_receiver)
            .then(|result| match result {
                Ok(Either::A(((), _))) => Ok(None),
                Ok(Either::B((id, _))) => Ok(Some(id)),
                Err(Either::A((e, _))) => Err(e),
                Err(Either::B((e, _))) => Err(IOError::new(IOErrorKind::Other, e).into()),
            }),
    )?;
    let shutdown_id = shutdown_id.expect("Unexpected stop");
    // Waiting for any on-going futures.
    while *counter.borrow() > 0 {
        core.turn(None);
    }
    // Start exiting
    let mut get_updates = GetUpdates::new();
    get_updates.offset(shutdown_id + 1);
    info!("{}> confirming", shutdown_id);
    core.run(api.send(get_updates).and_then(move |_| {
        info!("{}> confirmed", shutdown_id);
        api.send(ADMIN_ID.unwrap().text("bye"))
    })).map(|_| ())
}

fn handle_update(
    api: Api,
    executor: &command::Executor,
    update: Update,
) -> impl Future<Item = (), Error = ()> {
    let message = match update.kind {
        UpdateKind::Message(message) => message,
        _ => return Either::A(Ok(()).into_future()),
    };
    let command = match message.kind {
        MessageKind::Text { ref data, .. } => data,
        _ => return Either::A(Ok(()).into_future()),
    };

    let id = update.id;
    let username = message.from.username.unwrap_or(String::new());
    let user_id = message.from.id;
    info!(
        "{}> received from {}({}): {}",
        id, username, user_id, command
    );
    let is_admin = ADMIN_ID.as_ref().map_or(false, |id| &user_id == id);
    let chat = message.chat;
    let is_private = matches!(chat, MessageChat::Private(..));
    let cmd = command::Command {
        id,
        command,
        is_admin,
        is_private,
    };
    Either::B(executor.execute(cmd).and_then(move |reply| {
        let reply = reply.trim_matches(utils::is_separator);
        info!("{}> sending: {}", id, reply);
        let mut msg = chat.text(reply);
        msg.parse_mode(ParseMode::Html);
        msg.disable_preview();
        api.send(msg)
            .and_then(move |_| {
                info!("{}> sent", id);
                Ok(())
            })
            .map_err(move |err| {
                info!("{}> error: {:?}", id, err);
            })
    }))
}
