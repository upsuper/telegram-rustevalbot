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
use futures::unsync::oneshot;
use reqwest::header::{Headers, UserAgent};
use reqwest::unstable::async::Client;
use std::env;
use std::io::{Error as IOError, ErrorKind as IOErrorKind};
use tokio_core::reactor::{Core, Handle};
use telegram_bot::{
    Api, CanSendMessage, Error, MessageKind,
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
    let mut executor = command::Executor {
        client: &client,
        shutdown: Some(shutdown_sender),
    };
    let mut count = 0;
    if let Some(admin_id) = &admin_id {
        api.spawn(admin_id.text(format!("Start version: {}", VERSION)));
    }
    let future = api.stream().for_each(move |update| {
        let message = match update.kind {
            UpdateKind::Message(message) => message,
            _ => return Ok(()),
        };
        let command = match message.kind {
            MessageKind::Text { data, .. } => data,
            _ => return Ok(()),
        };
        count += 1;

        let id = count;
        let username = message.from.username.unwrap_or(String::new());
        let user_id = message.from.id;
        println!(
            "{}> received from {}({}): {}",
            id, username, user_id, command
        );
        let is_admin = admin_id.as_ref()
            .map_or(false, |admin_id| &user_id == admin_id);
        let chat = message.chat;
        let api = api.clone();
        let future = executor.execute(&command, is_admin)
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
                    })
                    .map_err(move |err| {
                        println!("{}> error: {:?}", id, err);
                    })
            });
        handle.spawn(future);
        Ok(())
    }).select(shutdown_receiver.map_err(|e| {
        Error::from(IOError::new(IOErrorKind::UnexpectedEof, e))
    })).map_err(|(e, _)| e).and_then(|_| Ok(()));
    core.run(future)
}
