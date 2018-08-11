extern crate htmlescape;
extern crate itertools;
extern crate futures;
#[macro_use]
extern crate lazy_static;
extern crate percent_encoding;
extern crate regex;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate telegram_bot;
extern crate tokio_core;

mod command;
mod utils;

use futures::{Future, Stream};
use futures::future::Either;
use futures::unsync::oneshot;
use reqwest::header::{Headers, UserAgent};
use reqwest::unstable::async::Client;
use std::cell::{Cell, RefCell};
use std::env;
use std::io::{Error as IOError, ErrorKind as IOErrorKind};
use std::rc::Rc;
use tokio_core::reactor::{Core, Handle};
use telegram_bot::{
    Api, CanSendMessage, Error, GetUpdates, MessageKind,
    ParseMode, UpdateKind, UserId
};

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("VERSION"),
    ")",
);
const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " - ",
    env!("CARGO_PKG_HOMEPAGE"),
);

fn build_client(handle: &Handle) -> Client {
    let mut headers = Headers::new();
    headers.set(UserAgent::new(USER_AGENT));
    Client::builder()
        .default_headers(headers)
        .build(handle)
        .unwrap()
}

fn main() -> Result<(), Error> {
    let mut core = Core::new()?;
    let token = env::var("TELEGRAM_TOKEN")
        .expect("TELEGRAM_TOKEN must be set!");
    println!("Running as `{}`", USER_AGENT);
    let admin_id = env::var("BOT_ADMIN_ID").ok()
        .and_then(|user_id| str::parse(&user_id).map(UserId::new).ok());

    let handle = core.handle();
    let api = Api::configure(token).build(&handle)?;
    let client = build_client(&handle);
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let executor = command::Executor {
        client: &client,
        shutdown: Cell::new(Some(shutdown_sender)),
    };
    if let Some(admin_id) = &admin_id {
        api.spawn(admin_id.text(format!("Start version: {}", VERSION)));
    }
    let counter = Rc::new(RefCell::new(0));
    let api_to_move = api.clone();
    let counter_to_move = counter.clone();
    let stream = api.stream().for_each(move |update| {
        let message = match update.kind {
            UpdateKind::Message(message) => message,
            _ => return Ok(()),
        };
        let command = match message.kind {
            MessageKind::Text { data, .. } => data,
            _ => return Ok(()),
        };

        let id = update.id;
        let username = message.from.username.unwrap_or(String::new());
        let user_id = message.from.id;
        println!(
            "{}> received from {}({}): {}",
            id, username, user_id, command
        );
        let is_admin = admin_id.as_ref()
            .map_or(false, |admin_id| &user_id == admin_id);
        let chat = message.chat;
        let api = api_to_move.clone();
        let future = executor.execute(id, &command, is_admin)
            .and_then(move |reply| {
                let reply = reply.trim_matches(utils::is_separator);
                println!("{}> sending: {}", id, reply);
                let mut msg = chat.text(reply);
                msg.parse_mode(ParseMode::Html);
                msg.disable_preview();
                api.send(msg)
                    .and_then(move |_| {
                        println!("{}> sent", id);
                        Ok(())
                    }).map_err(move |err| {
                        println!("{}> error: {:?}", id, err);
                    }).then(|result| {
                        result
                    })
            });
        let counter = &counter_to_move;
        let counter_clone = counter.clone();
        *counter.borrow_mut() += 1;
        handle.spawn(future.inspect(move |_| {
            *counter_clone.borrow_mut() -= 1;
        }));
        Ok(())
    });
    let shutdown_id = core.run(
        stream.select2(shutdown_receiver).then(|result| {
            match result {
                Ok(Either::A(((), _))) => Ok(None),
                Ok(Either::B((id, _))) => Ok(Some(id)),
                Err(Either::A((e, _))) => Err(e),
                Err(Either::B((e, _))) =>
                    Err(IOError::new(IOErrorKind::Other, e).into()),
            }
        })
    )?;
    let shutdown_id = shutdown_id.expect("Unexpected stop");
    // Waiting for any on-going futures.
    while *counter.borrow() > 0 {
        core.turn(None);
    }
    // Start exiting
    let mut get_updates = GetUpdates::new();
    get_updates.offset(shutdown_id + 1);
    println!("{}> confirming", shutdown_id);
    core.run(
        api.send(get_updates)
            .and_then(move |_| {
                println!("{}> confirmed", shutdown_id);
                api.send(admin_id.unwrap().text("bye"))
            })
    ).map(|_| ())
}
