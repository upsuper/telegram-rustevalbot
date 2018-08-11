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
use reqwest::header::{Headers, UserAgent};
use reqwest::unstable::async::Client;
use std::env;
use tokio_core::reactor::{Core, Handle};
use telegram_bot::{Api, CanSendMessage, Error, MessageKind, ParseMode, UpdateKind};

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

    let handle = core.handle();
    let api = Api::configure(token).build(&handle)?;
    let client = build_client(&handle);
    let executor = command::Executor {
        client: &client,
    };
    let mut count = 0;
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
        println!("{}> received from {}: {}", id, username, command);
        let chat = message.chat;
        let api = api.clone();
        let future = executor.execute(&command)
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
    });
    core.run(future)
}
